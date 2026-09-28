//! #1347: scheduled liveness check for `[lsp.acquire]`/`[dap.acquire]`
//! download URLs.
//!
//! `tests/extensions.rs`'s #919 registry conformance gate only checks that
//! every manifest *resolves a non-empty install command* per platform — it
//! never touches the network, so a manifest whose `[lsp.acquire]` points at a
//! release asset that has never existed (the terraform `install_linux` bug
//! this issue is named after) sails through green. This file closes that gap
//! the other direction: for every manifest that declares `[lsp.acquire]` /
//! `[dap.acquire]`, resolve the concrete download URL for every
//! `Platform::ALL` × `Arch::ALL` combination — reusing
//! `tool_acquire::resolve_asset`, the *same* URL-construction code path
//! `Engine::ext_install_from_registry` calls at real install time, not a
//! reimplementation — and HEAD-request it. It never downloads or installs
//! anything, so it can't collide with code-coordinator#3319's reasoning for
//! rejecting real installs (6 manifests `brew install`, no Homebrew in CI by
//! policy).
//!
//! This file is deliberately **not** part of the default `cargo test` run:
//! the one test that fetches the live `registry.json` and HEAD-requests real
//! upstream URLs is `#[ignore]`d (see [`registry_liveness_check_against_live_registry`]),
//! run only by `.github/workflows/registry-liveness.yml` on a weekly
//! schedule / `workflow_dispatch` — network flakes there must never block a
//! PR. The rest of this file's tests run in every `cargo test` invocation and
//! touch no network at all: they exercise the same classification pipeline
//! against fixture manifests, HEAD-requesting a local loopback-only HTTP
//! server this file spins up itself rather than any real host.

use std::io::{Read, Write};
use std::net::TcpListener;

use vimcode_core::core::extensions::{DapConfig, ExtensionManifest, LspConfig, Platform};
use vimcode_core::core::registry;
use vimcode_core::core::tool_acquire::{self, AcquireConfig, AcquireError, AcquireKind, Arch};

// ─── Classification ─────────────────────────────────────────────────────────

/// Outcome of HEAD-requesting one manifest × lsp/dap × platform × arch
/// resolved download URL.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    /// The URL responded with a 2xx status.
    Ok,
    /// The URL resolved but didn't respond OK (`HTTP 404`), or the resolve
    /// step itself failed for a reason other than "no build for this
    /// platform" (network error, bad JSON, …) — either way, something a
    /// manifest author needs to fix.
    Missing(String),
    /// `resolve_asset` reported `NoMatchingAsset`: the upstream kind
    /// genuinely publishes no build for this platform/arch (e.g. no Windows
    /// ARM64 release) — not a bug in the manifest.
    Unsupported,
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Outcome::Ok => write!(f, "OK"),
            Outcome::Missing(detail) => write!(f, "MISSING ({detail})"),
            Outcome::Unsupported => write!(f, "UNSUPPORTED"),
        }
    }
}

/// What kind of check (if any) applies to one `[lsp]`/`[dap]` config.
enum Coverage {
    /// Nothing declared for this half of the manifest (no `binary`/`adapter`
    /// at all) — no line is emitted.
    None,
    /// `[lsp.acquire]`/`[dap.acquire]` names one of the three archive-download
    /// kinds (#1345) — this is the thing this check actually verifies.
    /// Boxed: `AcquireConfig` is far larger than this enum's other variants
    /// (clippy's `large_enum_variant`).
    Acquire(Box<AcquireConfig>),
    /// `[lsp.acquire]`/`[dap.acquire]` names one of the five package-manager
    /// kinds (#1346, `npm`/`pip`/`go`/`cargo`/`dotnet-tool`) — no fixed
    /// download URL to HEAD-request (the package manager resolves its own),
    /// so it's reported, not silently skipped.
    PackageManagerAcquire,
    /// Only `install`/`install_linux`/`install_macos`/`install_windows` shell
    /// strings are declared — exactly the class of bug #1347 is about
    /// (a shell one-liner that silently rots), and exactly what this check
    /// *cannot* verify statically. Reported so the coverage gap is visible.
    ShellInstall,
}

fn lsp_coverage(lsp: &LspConfig) -> Coverage {
    if lsp.binary.is_empty() {
        return Coverage::None;
    }
    if let Some(acquire) = &lsp.acquire {
        return if tool_acquire::is_package_manager_kind(acquire.kind) {
            Coverage::PackageManagerAcquire
        } else {
            Coverage::Acquire(Box::new(acquire.clone()))
        };
    }
    if lsp.install_cmd_for(Platform::Linux).is_empty()
        && lsp.install_cmd_for(Platform::MacOS).is_empty()
        && lsp.install_cmd_for(Platform::Windows).is_empty()
    {
        return Coverage::None;
    }
    Coverage::ShellInstall
}

fn dap_coverage(dap: &DapConfig) -> Coverage {
    if dap.adapter.is_empty() {
        return Coverage::None;
    }
    if let Some(acquire) = &dap.acquire {
        return if tool_acquire::is_package_manager_kind(acquire.kind) {
            Coverage::PackageManagerAcquire
        } else {
            Coverage::Acquire(Box::new(acquire.clone()))
        };
    }
    if dap.install_cmd_for(Platform::Linux).is_empty()
        && dap.install_cmd_for(Platform::MacOS).is_empty()
        && dap.install_cmd_for(Platform::Windows).is_empty()
    {
        return Coverage::None;
    }
    Coverage::ShellInstall
}

/// HEAD-request `url`, returning the HTTP status code. Uses `curl` — the
/// same runtime dependency `tool_acquire`'s own `http_get_bytes`/
/// `download_asset` and `registry::fetch_registry` already require on every
/// platform vimcode ships for (see `tool_acquire.rs`'s module doc) — rather
/// than adding an HTTP client crate just for this check.
fn head_status(url: &str) -> Result<u16, String> {
    let null_device = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let output = vimcode_core::core::git::hidden_command("curl")
        .args([
            "-sS",
            "-o",
            null_device,
            "-w",
            "%{http_code}",
            "--max-time",
            "15",
            "-I",
            "-L",
            url,
        ])
        .output()
        .map_err(|e| e.to_string())?;
    let code_str = String::from_utf8_lossy(&output.stdout);
    code_str
        .trim()
        .parse::<u16>()
        .map_err(|e| format!("curl produced no usable status code ({code_str:?}): {e}"))
}

/// Resolve + HEAD-request one manifest × lsp/dap × platform × arch
/// combination. The one function that reuses `tool_acquire::resolve_asset`
/// end to end — everything above/below this is orchestration and reporting.
fn check_one(cfg: &AcquireConfig, platform: Platform, arch: Arch) -> Outcome {
    match tool_acquire::resolve_asset(cfg, platform, arch) {
        Ok(asset) => match head_status(&asset.url) {
            Ok(code) if (200..300).contains(&code) => Outcome::Ok,
            Ok(code) => Outcome::Missing(format!("HTTP {code}")),
            Err(e) => Outcome::Missing(format!("HEAD request failed: {e}")),
        },
        Err(AcquireError::NoMatchingAsset) => Outcome::Unsupported,
        Err(e) => Outcome::Missing(format!("resolve error: {e}")),
    }
}

/// Runs the full sweep over `manifests`, returning one report line per
/// checked row plus whether any row came back `MISSING`.
fn run_check(manifests: &[ExtensionManifest]) -> (Vec<String>, bool) {
    let mut lines = Vec::new();
    let mut any_missing = false;
    for manifest in manifests {
        for (label, coverage) in [
            ("lsp", lsp_coverage(&manifest.lsp)),
            ("dap", dap_coverage(&manifest.dap)),
        ] {
            match coverage {
                Coverage::None => {}
                Coverage::ShellInstall => {
                    lines.push(format!(
                        "{} [{label}]: UNCHECKED (shell install)",
                        manifest.name
                    ));
                }
                Coverage::PackageManagerAcquire => {
                    lines.push(format!(
                        "{} [{label}]: UNCHECKED (package manager)",
                        manifest.name
                    ));
                }
                Coverage::Acquire(cfg) => {
                    for platform in Platform::ALL {
                        for arch in Arch::ALL {
                            let outcome = check_one(&cfg, platform, arch);
                            if matches!(outcome, Outcome::Missing(_)) {
                                any_missing = true;
                            }
                            lines.push(format!(
                                "{} [{label}] {platform}/{arch}: {outcome}",
                                manifest.name
                            ));
                        }
                    }
                }
            }
        }
    }
    (lines, any_missing)
}

// ─── Fixture HTTP server (loopback only — no outbound network) ─────────────

/// Minimal HTTP/1.1 server for this file's own fixture tests: binds an
/// ephemeral loopback port, answers up to `expected_requests` HEAD requests
/// with a status decided by `status_for` (keyed by the request path), then
/// stops. Real sockets, but 127.0.0.1-only — nothing here reaches an
/// external host, so this stays within the "fixture API responses, no
/// network" acceptance bar for the resolver-half unit tests despite
/// exercising the exact same `curl`-over-the-wire code path `head_status`
/// uses against a real upstream.
///
/// Deliberately bounded by a wall-clock deadline, not just a request count:
/// a regression that makes `check_one` stop calling `head_status` (e.g. the
/// exact bug [`bad_fixture_asset_name_produces_missing`] guards against)
/// would otherwise mean the server thread's `accept()` never sees the
/// expected connections and blocks forever — turning a test *failure* into
/// a `cargo test` *hang*, which is worse than either passing or failing.
/// Capping the loop means the server thread always returns within the
/// deadline (and the caller's assertions still correctly fail), never later.
fn spawn_head_fixture_server(
    expected_requests: usize,
    status_for: impl Fn(&str) -> u16 + Send + 'static,
) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral loopback port");
    listener
        .set_nonblocking(true)
        .expect("set fixture listener nonblocking");
    let addr = listener.local_addr().expect("resolve bound local_addr");
    let base_url = format!("http://{addr}");
    let handle = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut served = 0;
        while served < expected_requests && std::time::Instant::now() < deadline {
            let (mut stream, _) = match listener.accept() {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                }
                Err(_) => break,
            };
            let _ = stream.set_nonblocking(false);
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/")
                .to_string();
            let code = status_for(&path);
            let reason = if code == 200 { "OK" } else { "Not Found" };
            let response = format!(
                "HTTP/1.1 {code} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            served += 1;
        }
    });
    (base_url, handle)
}

fn url_template_manifest(name: &str, url: &str) -> ExtensionManifest {
    ExtensionManifest {
        name: name.to_string(),
        display_name: name.to_string(),
        lsp: LspConfig {
            binary: name.to_string(),
            acquire: Some(AcquireConfig {
                kind: AcquireKind::UrlTemplate,
                url: url.to_string(),
                version: "1.0.0".to_string(),
                binary_path: name.to_string(),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    }
}

// ─── Resolver-half unit tests: fixture manifests, no network ───────────────

#[test]
fn resolves_urls_for_every_platform_arch_combination_without_network() {
    // The resolver half, in isolation: `resolve_asset` (reused, not
    // reimplemented) must build a distinct, correctly-substituted URL for
    // every one of the 3 platforms × 2 arches this check sweeps — with zero
    // network access, since `url-template` resolution is pure template
    // substitution.
    let manifest = url_template_manifest(
        "fixture-tool",
        "https://example.invalid/{version}/tool_{os}_{arch}.zip",
    );
    let cfg = manifest
        .lsp
        .acquire
        .as_ref()
        .expect("fixture declares acquire");

    let mut seen = std::collections::HashSet::new();
    for platform in Platform::ALL {
        for arch in Arch::ALL {
            let asset = tool_acquire::resolve_asset(cfg, platform, arch)
                .expect("url-template always resolves");
            assert_eq!(asset.version, "1.0.0");
            assert!(
                asset.url.starts_with("https://example.invalid/1.0.0/tool_"),
                "unexpected url: {}",
                asset.url
            );
            assert!(
                seen.insert(asset.url.clone()),
                "platform/arch combination produced a duplicate URL: {}",
                asset.url
            );
        }
    }
    assert_eq!(seen.len(), 6, "3 platforms x 2 arches = 6 distinct URLs");
}

#[test]
fn coverage_classifies_acquire_shell_install_package_manager_and_none() {
    let acquire_lsp = LspConfig {
        binary: "terraform-ls".to_string(),
        acquire: Some(AcquireConfig {
            kind: AcquireKind::HashicorpRelease,
            product: "terraform-ls".to_string(),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(matches!(lsp_coverage(&acquire_lsp), Coverage::Acquire(_)));

    let package_manager_lsp = LspConfig {
        binary: "pyright".to_string(),
        acquire: Some(AcquireConfig {
            kind: AcquireKind::Npm,
            package: "pyright".to_string(),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(matches!(
        lsp_coverage(&package_manager_lsp),
        Coverage::PackageManagerAcquire
    ));

    let shell_install_lsp = LspConfig {
        binary: "clangd".to_string(),
        install_linux: "sudo apt-get install -y clangd".to_string(),
        ..Default::default()
    };
    assert!(matches!(
        lsp_coverage(&shell_install_lsp),
        Coverage::ShellInstall
    ));

    // Nothing declared at all (e.g. the "git-insights" manifest, which has no
    // LSP/DAP) — nothing to check, and not a coverage gap either.
    assert!(matches!(
        lsp_coverage(&LspConfig::default()),
        Coverage::None
    ));
    assert!(matches!(
        dap_coverage(&DapConfig::default()),
        Coverage::None
    ));
}

/// #1347 acceptance: "a deliberately bad fixture (asset name that doesn't
/// exist) produces MISSING and exits non-zero." Confirmed RED first: with
/// `check_one`'s `Ok(code) if (200..300).contains(&code) => Outcome::Ok`
/// arm changed to always return `Outcome::Ok` regardless of status code,
/// this test fails (`assert!(matches!(.., Outcome::Missing(_)))` sees `Ok`
/// instead) — so this is a real regression guard, not a test that would
/// stay green with the bug reinstated.
#[test]
fn bad_fixture_asset_name_produces_missing() {
    // The fixture server only serves the "amd64" path with 200; every other
    // path (standing in for an asset name that was never actually
    // published, matching the real terraform `install_linux` bug) gets a 404.
    let (base_url, server) =
        spawn_head_fixture_server(2, |path| if path.contains("amd64") { 200 } else { 404 });

    let cfg = AcquireConfig {
        kind: AcquireKind::UrlTemplate,
        url: format!("{base_url}/tool_{{os}}_{{arch}}.zip"),
        version: "1.0.0".to_string(),
        binary_path: "tool".to_string(),
        ..Default::default()
    };

    let ok_outcome = check_one(&cfg, Platform::Linux, Arch::Amd64);
    assert_eq!(
        ok_outcome,
        Outcome::Ok,
        "the amd64 asset exists on the fixture server"
    );

    let missing_outcome = check_one(&cfg, Platform::Linux, Arch::Arm64);
    assert!(
        matches!(&missing_outcome, Outcome::Missing(detail) if detail.contains("404")),
        "the arm64 asset is the deliberately-bad fixture and should be MISSING (HTTP 404), got {missing_outcome:?}"
    );

    server
        .join()
        .expect("fixture server thread should not panic");
}

/// Same bad fixture, exercised through the full `run_check` sweep (all 3
/// platforms x 2 arches) rather than `check_one` directly, confirming the
/// aggregate `any_missing` flag — what the real binary/test uses to decide
/// its process exit code — actually flips to `true`.
#[test]
fn manifest_with_missing_asset_flags_any_missing_across_full_sweep() {
    let total_requests = Platform::ALL.len() * Arch::ALL.len();
    let (base_url, server) = spawn_head_fixture_server(total_requests, |path| {
        if path.contains("amd64") {
            200
        } else {
            404
        }
    });

    let manifest = url_template_manifest(
        "bad-fixture-tool",
        &format!("{base_url}/tool_{{os}}_{{arch}}.zip"),
    );
    let (lines, any_missing) = run_check(std::slice::from_ref(&manifest));

    assert_eq!(lines.len(), total_requests, "one line per platform x arch");
    assert!(
        any_missing,
        "at least one arm64 row is a deliberately-bad fixture (404) — the sweep must flag it:\n{}",
        lines.join("\n")
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("MISSING") && l.contains("404")),
        "expected a MISSING (HTTP 404) line, got:\n{}",
        lines.join("\n")
    );
    // The amd64 rows are genuinely fine — a real bug in `check_one` that
    // marked *everything* missing regardless of status would still pass the
    // `any_missing` assertion above, so pin the positive case too.
    assert!(
        lines
            .iter()
            .any(|l| l.contains("amd64") && l.contains(": OK")),
        "expected an OK line for the amd64 rows, got:\n{}",
        lines.join("\n")
    );

    server
        .join()
        .expect("fixture server thread should not panic");
}

/// The mirror-image positive control: every asset resolves and HEAD-requests
/// 200, so nothing is flagged missing. Guards against a `check_one` that's
/// been made to over-report (e.g. always `Missing`), which the two tests
/// above alone wouldn't catch.
#[test]
fn manifest_with_every_asset_present_reports_no_missing() {
    let total_requests = Platform::ALL.len() * Arch::ALL.len();
    let (base_url, server) = spawn_head_fixture_server(total_requests, |_path| 200);

    let manifest = url_template_manifest(
        "good-fixture-tool",
        &format!("{base_url}/tool_{{os}}_{{arch}}.zip"),
    );
    let (lines, any_missing) = run_check(std::slice::from_ref(&manifest));

    assert!(
        !any_missing,
        "every asset resolved OK — nothing should be flagged missing:\n{}",
        lines.join("\n")
    );
    assert!(
        lines.iter().all(|l| l.contains(": OK")),
        "{}",
        lines.join("\n")
    );

    server
        .join()
        .expect("fixture server thread should not panic");
}

// ─── Live, scheduled check ──────────────────────────────────────────────────

/// Not run by `cargo test` — this is the network-touching test
/// `.github/workflows/registry-liveness.yml` invokes explicitly
/// (`cargo test --test registry_liveness -- --ignored --nocapture`) on a
/// weekly schedule / `workflow_dispatch`. Fetches the *live* `registry.json`
/// from vimcode-ext's default branch (`registry::DEFAULT_REGISTRY_URL` — the
/// same URL vimcode itself fetches at runtime) and HEAD-requests every
/// resolved `[lsp.acquire]`/`[dap.acquire]` download URL for real. Prints one
/// line per checked row (plus one line per `UNCHECKED` manifest) and fails
/// the test — non-zero process exit — if any row came back `MISSING`.
#[test]
#[ignore]
fn registry_liveness_check_against_live_registry() {
    let manifests = registry::fetch_registry(registry::DEFAULT_REGISTRY_URL)
        .expect("should fetch the live vimcode-ext registry.json");
    assert!(
        !manifests.is_empty(),
        "live registry.json should not be empty"
    );

    let (lines, any_missing) = run_check(&manifests);
    for line in &lines {
        println!("{line}");
    }
    assert!(
        !any_missing,
        "one or more [lsp.acquire]/[dap.acquire] download URLs are missing — see the lines above"
    );
}

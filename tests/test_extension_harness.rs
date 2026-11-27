//! Black-box coverage for `vimcode --test-extension <ext-dir> <spec.lua>`
//! (`src/test_extension.rs`) — spawns the actual compiled `vimcode` binary,
//! mirroring `tests/ext_install_cli.rs`'s real-binary-spawn pattern, since
//! the harness itself is a CLI entry point, not an in-process API.
//!
//! Covers both fixture extensions under `tests/fixtures/extensions/`:
//! `view_demo` (the current `vimcode.ui.register_view` View API, driven
//! and asserted via rendered output, not just structural state) and
//! `stub_demo` (`t.stub_command` wrapping a fake `kubectl`). Also pins the
//! two acceptance requirements that aren't covered by either fixture
//! passing: a failing assertion exits non-zero and prints the screen
//! dump, and a spec with no extension/spec at all fails cleanly instead
//! of hanging or panicking.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// `tests/fixtures/extensions/<name>`, as an absolute path — the harness
/// itself `canonicalize`s both CLI arguments before any `chdir` happens
/// (see `test_extension::run`'s own doc), but passing already-absolute
/// paths here means this test's assertions don't depend on that.
fn fixture_ext(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/extensions")
        .join(name)
}

/// Run `vimcode --test-extension <ext_dir> <spec_path>` as a real
/// subprocess and return its captured output.
fn run_test_extension(ext_dir: &Path, spec_path: &Path) -> Output {
    let exe = env!("CARGO_BIN_EXE_vimcode");
    Command::new(exe)
        .arg("--test-extension")
        .arg(ext_dir)
        .arg(spec_path)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("failed to run vimcode --test-extension")
}

/// The `view_demo` fixture (current View API: register, open, click
/// through `t.click_text`, assert both the structural widget tree via
/// `t.view` and the re-rendered screen) must pass.
#[test]
fn view_demo_fixture_passes() {
    let ext_dir = fixture_ext("view_demo");
    let spec = ext_dir.join("tests/basic_spec.lua");
    let output = run_test_extension(&ext_dir, &spec);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "view_demo spec must pass; status: {:?}\nstdout: {stdout}\nstderr: {stderr}",
        output.status.code()
    );
    assert!(
        stdout.contains("PASS:"),
        "expected a PASS line; got stdout: {stdout}\nstderr: {stderr}"
    );
}

/// The `stub_demo` fixture (`t.stub_command` faking `kubectl`, then
/// asserting the wrapped command's stubbed output is actually painted)
/// must pass — proving process stubbing works end to end without any
/// real `kubectl` on this machine's `PATH`.
#[test]
fn stub_demo_fixture_passes() {
    let ext_dir = fixture_ext("stub_demo");
    let spec = ext_dir.join("tests/basic_spec.lua");
    let output = run_test_extension(&ext_dir, &spec);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "stub_demo spec must pass; status: {:?}\nstdout: {stdout}\nstderr: {stderr}",
        output.status.code()
    );
    assert!(
        stdout.contains("PASS:"),
        "expected a PASS line; got stdout: {stdout}\nstderr: {stderr}"
    );
}

/// A failing assertion must exit non-zero and print the rendered screen
/// dump — the acceptance criterion "A failing assertion prints the
/// screen dump" from the issue this harness implements.
#[test]
fn failing_assertion_exits_nonzero_and_prints_screen_dump() {
    let ext_dir = fixture_ext("view_demo");
    let dir = std::env::temp_dir().join(format!(
        "vc-test-extension-harness-fail-spec-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    let spec = dir.join("fail_spec.lua");
    std::fs::write(
        &spec,
        "assert(t.contains(\"THIS_TEXT_IS_NEVER_PAINTED_1917\"), \"expected marker\")\n",
    )
    .expect("write failing spec");

    let output = run_test_extension(&ext_dir, &spec);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "a failing assertion must exit non-zero; status: {:?}\nstdout: {stdout}\nstderr: {stderr}",
        output.status.code()
    );
    assert!(
        stderr.contains("expected marker"),
        "expected the assertion message in stderr; got: {stderr}"
    );
    assert!(
        stderr.contains("screen dump"),
        "expected a screen dump on a failing assertion; got stderr: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// An extension directory with no `manifest.toml` fails cleanly (exits
/// non-zero with a message naming the missing file) rather than hanging
/// or panicking — the harness's basic input-validation contract.
#[test]
fn missing_manifest_fails_cleanly() {
    let dir = std::env::temp_dir().join(format!(
        "vc-test-extension-harness-no-manifest-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    let spec = dir.join("spec.lua");
    std::fs::write(&spec, "assert(true)\n").expect("write spec");

    let output = run_test_extension(&dir, &spec);
    assert!(
        !output.status.success(),
        "an extension dir with no manifest.toml must exit non-zero"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("manifest"),
        "expected the error to name the missing manifest; got: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

# ms-example — the reachability slice, not a Gate-A contract

This directory is the worked example / seam proof for **#657** ("Put vimcode on
the oracle loop"). It is deliberately **not** a real milestone: nothing here was
authored from a Gate-A contract by the `test-author` agent, and no manifest maps
its tests to work-order issues.

## What `seam_657.rs` proves

That an *external* integration-test crate — `tests/acceptance.rs`, which links
only against `[lib] vimcode_core` — can drive **both** vimcode backends and both
of their black-box harnesses:

| Backend | Seam | Test |
|---|---|---|
| TUI | `vimcode_core::tui_main::testing::TuiShellApp` + quadraui `driver_with_shell` | `tui_backend_paints_a_full_frame_from_an_integration_test`, `tui_backend_paints_seeded_buffer_text` |
| GTK | `vimcode_core::gtk::testing::harness` (the #646 `GtkDriver` harness) | `gtk_backend_paints_from_an_integration_test`, `gtk_backend_reports_a_painted_editor_pane_rect` |

Before #657, none of that compiled. `render`, `tui_main` and `gtk` lived inside
the `vimcode` **binary**, so `tests/*.rs` could see nothing but `core`, `icons`
and `quadraui_pin` — which is why every black-box test in this repo had to be
in-crate. `coord/acceptance.py` hardcodes `ACCEPTANCE_DIRNAME =
"tests/acceptance"`, so an in-crate `#[cfg(test)] mod acceptance` was never an
option; the suite has to be a real integration-test target.

## Layout of a real slice

A real milestone directory looks like:

```text
tests/acceptance/ms-NN/
  contract.md            # Gate-A contract the slice was authored from
  manifest.yml           # libtest id -> work-order issue number
  mocks/                 # driver mock fixtures (vimcode's glob: *.screen)
  <topic>_<issue>.rs     # the sealed slice, include!d by tests/acceptance.rs
```

Slices are **sealed**: the worker fixing the issue may run them
(`coord acceptance run --issue N`) but may not read or edit them. The tamper
gate in `coord/merge_queue.py` enforces that on this path.

## Running

```sh
cargo test --test acceptance --features test-support
```

The configured driver adds libtest JSON output:

```sh
RUSTC_BOOTSTRAP=1 cargo test --test acceptance --features test-support \
  -- -Z unstable-options --format json
```

`RUSTC_BOOTSTRAP=1` is what makes `--format json` work on a stable rustc.

## Disposal

`ms-example` may be deleted once a real `ms-NN` slice exists. Keeping it costs
four fast tests and documents the seam for whoever writes that first slice.

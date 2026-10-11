//! Proves `paths::guard_real_config_write`'s backstop fires from a
//! `tests/*.rs` integration-test binary — the structural gap a `--lib` unit
//! test alone can't cover, since `cfg(test)` is false for the library when
//! it is linked into an integration test crate as an ordinary dependency.
//!
//! This test deliberately does **not** go through `common::engine_with()`
//! (which calls `session::suppress_disk_saves()`): the whole point is to
//! simulate a future test that forgot to isolate its home directory, and
//! confirm the write is refused instead of silently landing in the
//! developer's real `~/.config/vimcode`.

use vimcode_core::Engine;

#[test]
#[should_panic(expected = "refusing to write to the real vimcode config dir")]
fn set_ext_setting_without_isolation_panics_instead_of_writing() {
    let mut e = Engine::new();
    e.set_ext_setting("demo-ext", "some_key", "some_value");
}

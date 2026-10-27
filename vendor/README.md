# Vendored dependency patches

`wry/` is the unmodified crates.io wry 0.53.5 source except for a backport of the upstream initialization-focus fix: https://github.com/tauri-apps/wry/pull/1799 (commit c2a28c6, merged b95a7f8). Initial `ICoreWebView2Controller::MoveFocus` failure is nonfatal; genuine environment/controller/renderer failures still propagate. Its original MIT/Apache-2.0 license files are preserved. This is third-party code and is not relicensed under the project's GPL. Remove this patch after Dioxus adopts a Wry release carrying the fix and native regression tests pass.

# ADR 0001 — Windows vertical slice and release limits

Use a Rust workspace, stable Dioxus WebView renderer and direct native core calls on background workers. This implements a testable Windows path while keeping the framework decision conditional on RHEL and Android feasibility. No React/TypeScript or server data path is introduced. Native dependencies such as WebView2, SQLite and zstd are acknowledged and must be inventoried.

Google Drive and Dropbox desktop public-client flows use a maintained OAuth2 library with PKCE, system browser, state and loopback validation; Android requires its own native integration. This source build exposes registration details under advanced setup because no official release registration exists. It is not the novice production connection flow required by the full release.

One-copy fixed-size 1 MiB encrypted chunks precede immutable encrypted manifests. Full read-back and bounded restore are prioritized ahead of spanning pools, replica repair, deletion or distributed maintenance. No per-provider fixed quota is advertised. Fresh-device recovery requires an exported snapshot locator kit; key-only/lost-kit discovery is deliberately not promised.

The first real Windows launch exposed an upstream Wry0.53.5 initialization focus error, WebView2 HRESULT0x80070057. A minimal backport of [upstream Wry1799](https://github.com/tauri-apps/wry/pull/1799) in `vendor/wry` makes an initial focus failure nonfatal while retaining failures for real renderer/controller creation. Third-party original license files remain. The patch must be removed when the framework's renderer dependency includes the fix and a native startup regression check passes.

Human review remains required for crypto, hostile restore behavior, native secrets and release security; subagent review does not replace an independent audit. Missing credentials/devices/runners/signing materials are blockers recorded in STATUS.md, not simulated evidence.

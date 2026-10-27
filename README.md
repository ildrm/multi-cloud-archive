# Multi-cloud archive (early development)

CloudWeave is an experimental local-first encrypted archive client written in Rust. The Windows Dioxus application and CLI can create a vault, archive files/folders as compressed encrypted chunks, fully read back and verify uploaded data, export snapshot recovery kits, and restore into a new directory using a passphrase or independent recovery key. Original sources are retained. The working name is provisional and has not been checked for public use.

Google Drive and Dropbox adapters use real HTTPS APIs and browser-based desktop PKCE authorization, with multiple identities and OS credential storage. They require your own provider registration in this source build and have **not passed live account tests**. A local/removable destination is provided for engine testing and interoperability. This is not yet the full multi-provider, Android or production release described in the implementation brief. See [STATUS.md](STATUS.md), [PROVIDER_SUPPORT.md](PROVIDER_SUPPORT.md) and [SECURITY.md](SECURITY.md) before relying on it.

## Build and test

Install the exact Rust version in `rust-toolchain.toml` with rustup, including rustfmt and clippy, then run:

```text
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Run the desktop app with `cargo run -p cloudweave-desktop --locked`, or open `target/debug/cloudweave-desktop.exe` after a build. CLI help is available with `cargo run -p cloudweave-cli -- --help`. The GUI uses Windows WebView2; the C/C++ build tools are needed for native dependencies such as SQLite and zstd. No project server, Docker or database service is required.

On this development machine the installed `stable` toolchain identifies itself as Rust 1.97.1; commands use `cargo +stable ...` because the version-named rustup toolchain is not installed. Dev/test debug symbols and incremental artifacts are disabled to reduce build disk use. This does not establish compatibility on other operating systems. [Windows packaging](scripts/build-windows.ps1) produces portable binaries and SHA-256 hashes; MSI, signing and automatic updates are not implemented.

The GUI supports English/Persian, RTL, system/light/dark themes, native file dialogs and a persistent job history. Native accessibility and mobile layouts still need device/human validation. Interrupted uploads are marked for manual retry on restart; per-object session resume, cross-account placement, two-copy archives, selective restore, retention, scheduled jobs and source removal are not implemented. Filename metadata is encrypted; provider identity, sizes, counts and job phases remain visible locally.

The repository's existing [GPL-2.0 license](LICENSE) is retained. A change to a permissive license requires authorization from the rights holders; no third-party code has been relicensed.

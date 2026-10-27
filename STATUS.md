# Implementation status

Updated 2026-10-27. The Windows vertical slice is implemented; no full delivery milestone or production support claim is complete. Live cloud and other-platform acceptance remain blocked/unverified.

| Requirement | Status and evidence |
| --- | --- |
| Multiple accounts and reconnect safety | Domain isolation tests plus real desktop Google/Dropbox OAuth, provider-confirmed subjects, UUID account catalog, native credential references and per-account refresh/reconnect locks in `crates/app`. Multiple live accounts have not been tested. |
| Quota / provider-loss placement | Partial advisory per-object planner; tests cover unknown capacity, reserves, in-flight bytes, size limits, drain state and different providers. No durable reservations, quota freshness, resource/identity verification, rate or budget policy, aggregate capacity estimate or live data. |
| Vault, streaming, encryption, format, commit, restore | Implemented experimental one-copy engine in `crates/engine`: 1 MiB chunks, zstd, keyed BLAKE3, XChaCha20-Poly1305, bounded Argon2id envelopes, immutable native locators, full read-back verification, exported kits and new-directory restore. Local synthetic tests pass; no cloud durability claim. |
| Google Drive and Dropbox | Real browser PKCE and HTTPS identity/quota/upload/download source in `crates/app`; provider app registrations and authorized accounts are unavailable for live tests. Not marked supported. |
| OneDrive and other providers | Not implemented; see `docs/connectors.json`. |
| Desktop GUI / CLI | Dioxus Rust UI and CLI implemented. English/Persian RTL, themes, native dialogs, background operations and durable job phases. Human screen-reader/usability testing unperformed. |
| Android | Not implemented. No Android SDK/NDK target/device evidence; SAF, native authorization, Keystore and lifecycle work are explicit blockers. Desktop code does not satisfy Android. |
| Windows, Debian, RHEL, macOS release | Windows GUI/CLI built and portable unsigned package created in `dist/cloudweave-windows-x64`. Windows window rendered with title/handle/responding checks and actual WebView screenshots. Other systems, installers, signing and updates remain unverified/unimplemented. |
| Security, cryptography, destructive operations | Separate agent engineering review found issues in UI queue delivery, imported local paths, callback deadlines, Retry-After and refresh/reconnect locking; remediated in source. This is not an independent human audit. Deletion/GC remain unimplemented. |
| Acceptance tests A01–A26 | All full release scenarios incomplete; partial evidence is mapped in `docs/ACCEPTANCE.md`. |

## Verification on this machine

- `cargo +stable test --workspace --locked --offline`: 17 tests passed on Windows x86_64 build 10.0.19045.6466, Rust 1.97.1. Uses synthetic local data only, no live uploads. Includes fresh-kit/recovery-key bytes, committed v1 fixture, corruption, wrong secret, KDF bounds, changed sources, unsafe/colliding paths, local immutability, imported-kit path isolation, job interruption, OAuth correlation and UI queue-deadlock regression.
- Workspace Clippy with `-D warnings` passes for application crates. Vendored Wry emits one pre-existing lifetime-style compiler warning. Formatting check passes.
- Built CLI `self-test` passed: 630,000 synthetic original bytes, 1,399 stored payload bytes, recovery-key restore byte-identical. This is not a representative compression benchmark or cloud test.
- Real packaged Dioxus WebView rendered and English dark overview / Persian settings screenshots reviewed. axe-core 4.12.1 with WCAG2 A/AA tags reports 0 violations in both views (19/20 checks passed); decorative icon contrast remains one incomplete manual-review category. No human screen-reader/mobile/usability certification is claimed.
- Portable binaries, SHA-256 checksums, a 675-package Cargo license inventory and SPDX dependency SBOM are in `dist/cloudweave-windows-x64` / `artifacts`. Metadata inventory is not a vulnerability or license-compliance audit.
- The D: development volume exhausted space during the first debug build. Generated artifacts were relocated to `C:/Users/Mato/AppData/Local/Temp/cloudweave-build-c1b51d61`; final build/test/clippy use `--target-dir` there. Source and portable binaries stay in this repository. Use an adequately sized build volume; scripts accept `-TargetDir`.
- Version-pinned rustup toolchain is not installed; the locally installed stable compiler reports the same compiler version. Cross-platform builds, installers and signed releases have not been verified. No full milestone or production release is marked complete.

## Next executable work

1. Register provider desktop/Android clients and use authorized synthetic test accounts for two-account isolation, real archive/read-back, token expiry, quota exhaustion, duplicate/paginated discovery and fresh-device scoped recovery. Supply registrations in the UI; do not commit them.
2. Implement Android SAF stream and native authorization/Keystore/lifecycle bridges, then prove the required slice on real Android and actual RHEL, Debian and macOS runners.
3. Add OneDrive, full provider contract/capability harness, safe cancellation, persisted resumable sessions and fault injection at every durable transition. Perform independent human cryptography/release review.
4. Integrate the advisory planner with real account/resource/failure mappings and persistent atomic reservations before offering cross-account pools or two-copy protection. Add recovery discovery for lost kits, historical fixtures, selective restore and filesystem metadata support.
5. Run native accessibility/usability/performance tests, dependency advisory/license scans and SBOM generation; establish private disclosure contact, installers and signed release/update metadata. Keep source deletion disabled until a separately verified workflow exists.

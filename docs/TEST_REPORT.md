# Windows development test report — 2026-10-27

Environment: Windows x86_64 10.0.19045.6466, installed stable Rust 1.97.1/MSVC. Commands use `+stable`; exact version-named rustup installation is absent. Build cache was moved to a dedicated C: temporary directory after D: filled. Portable unsigned artifacts remain under `dist/cloudweave-windows-x64`.

| Check | Observed result |
| --- | --- |
| Full workspace tests | Passed: 17 tests, 0 failed. Synthetic data; no provider calls. |
| Workspace Clippy `--all-targets -- -D warnings` | Passed for application crates; vendored Wry has one upstream lifetime-style compiler warning. |
| Rust formatting | Passed. |
| Windows GUI + CLI build | Passed; real executable binaries produced. |
| CLI synthetic archive/read-back/recovery restore | Passed, exact bytes, 630,000 original / 1,399 stored payload bytes. |
| GUI startup | Initial Wry focus failure reproduced, upstream fix backported; packaged GUI then mounted, displayed a Windows title/handle and responded. |
| English Overview and Persian Settings | Actual renderer screenshots reviewed; locale and theme interaction exercised. |
| Automated accessibility | axe4.12.1 WCAG2 A/AA tags: 0 violations in both sampled views; one incomplete icon-contrast category needs manual review. |
| Native GUI file-picker archive/restore | Not fully automated end-to-end: native helper unavailable. Engine/CLI and UI source/controls tested separately. |
| Google/Dropbox live identity, upload, refresh, recovery | Blocked: no project registrations or authorized test accounts supplied. API source is not a passed integration test. |
| OneDrive and Android | Not implemented. |
| Debian/RHEL/macOS, Windows11 | Not tested. |
| Install/update/uninstall/signing/security audit | Not complete. Portable development package only. |

Behavioral coverage includes hostile and case/canonical-colliding paths, KDF caps, wrong secrets without output, local immutability/oversize reads, source mutation refusing commit, ciphertext corruption, independent exported-kit recovery, fixed v1 format fixture, interruption journaling, exact loopback correlation, imported-root isolation and saturated UI message delivery. It does not establish every A01–A26 requirement; see ACCEPTANCE.md.

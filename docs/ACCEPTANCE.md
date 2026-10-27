# Release acceptance evidence

No full acceptance ID is marked Passed. The following partial evidence is useful but does not substitute for the requested real-provider, fresh-device and platform scenarios. Source: user implementation brief, A01–A26. Test names are in `crates/*/tests` or module unit tests.

| ID | Current state |
| --- | --- |
| A01 | Partial: three same-provider model identities isolated. Real multi-Google sessions/quotas blocked on registrations/accounts. |
| A02 | Partial: duplicate identity rejection and identity-checked reconnect model; real repeated OAuth untested. |
| A03 | Not tested: live independent token revoke/refresh during upload. |
| A04 | Partial: exported kit + recovery secret restores after original vault path hidden, no catalog/keyring; local store only. Cloud fresh install blocked. |
| A05 | Blocked: no other required OS runner/device tests. |
| A06 | Not implemented: Android SAF sources. |
| A07 | Partial: SQLite restart marks unfinished job interrupted; no exhaustive kill-at-every-transition suite or automatic resume. |
| A08 | Not implemented: archive engine writes one copy only. |
| A09 | Partial: domain planner rejects two Google accounts for provider-loss policy; pipeline offers only one-copy and no false stronger badge. |
| A10 | Partial: exact callback checks, locks, bounded read retries and Retry-After source; no live 429/rotation/fault contract test. |
| A11 | Partial: model quota aliases/reserves/unknowns; no live admission/reservation layer. |
| A12 | Partial: bounded chunks and direct per-object upload with no whole-file spool; peak memory/low-disk tests not measured. |
| A13 | Partial: source mutation injected during upload refuses snapshot commit; no destructive workflow exists. |
| A14 | Partial: corrupt ciphertext and wrong secret detected in local recovery; no replica fallback or full truncation matrix. |
| A15 | Partial: bounded KDF rejection and wrong passphrase no-output tests; no comprehensive fuzzing. |
| A16 | Partial: Persian filenames round-trip; traversal/reserved/case/canonical conflict guards tested. No all-platform long-path fixture suite. |
| A17 | Partial: hostile paths validated before output, bounded parser/decompressor and immutable output; malicious authenticated manifest/decompressor fuzz suite incomplete. |
| A18 | Not implemented: Android lifecycle/process recovery. |
| A19 | Partial design: random immutable snapshot branches, no overwrite/GC; concurrent two-device contracts not run. |
| A20 | Not implemented: migration/drain/remove live workflow. |
| A21 | Partial: raw/zstd chunk metadata and a fixed encrypted v1 recovery fixture pass byte comparison; no older released formats exist yet. |
| A22 | Partial: actual Windows English/Persian screenshots reviewed, automated axe WCAG2 A/AA tags show 0 violations in overview and Persian settings with icon contrast needing manual review; queue regression passes. Native screen-reader/usability/mobile suite not run. |
| A23 | Partial: unsigned portable Windows build script; installer/update/uninstall matrix unavailable. |
| A24 | Partial: secret Debug redaction test and no token logging; comprehensive cache/crash/support-bundle scan not performed. |
| A25 | Blocked: desktop/Android client registrations and Android implementation unavailable. |
| A26 | Partial: native IDs, bounded Google folder pagination, no deletion; incomplete listing/duplicate provider contracts untested. |

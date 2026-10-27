# Recover an experimental snapshot

Keep an exported snapshot recovery kit and either its passphrase or independent recovery key. The key should be kept separately from cloud/local object storage. The kit is snapshot-specific; export each snapshot you need. Keep more than one independent copy of the kit. A key alone does not identify provider-native object IDs. The current app cannot discover a snapshot whose kit has been lost.

1. Install/build the application on a verified Windows machine. Other platforms are not yet validated.
2. For a cloud archive, connect the same Google or Dropbox identity using a registration that can see the app-created files. An unrelated client/API project may not have scoped access. Cross-client and Android scoped recovery remains untested and is a release blocker. You do not need the original catalog, keyring or local account UUID.
3. Open Recovery, import the exported kit, choose a new output parent, and enter the passphrase or select the independent recovery-key option. For a local archive explicitly select the existing object-storage folder as well. The imported kit's absolute path is only a hint and is never opened automatically; you can select its new location after moving the encrypted objects. Windows UNC roots and reparse-point roots are rejected by this local restore adapter.
4. Restore. A new `CloudWeave-{UUID}` child appears only after the files verify and staging completes. Existing files are not overwritten. On error a `.cloudweave-partial-*` directory may remain with partial plaintext; do not interpret it as success.
5. Compare required files before relying on the restored copy. Sources are never deleted by the app.

The synthetic integration test `fresh_install_recovery_dedup_corruption_and_wrong_secrets` hides the original vault path and restores using only an exported kit, recovery secret and object store. It checks exact Persian-path and binary bytes and tamper/wrong-secret failures. This is local-engine evidence only, not a live cloud or cross-platform recovery test. One-copy archives cannot survive loss of their storage account/provider. The planner's two-provider policy is not connected to the archive pipeline yet.

CLI equivalents: `cloudweave-cli init --vault <dir> --key-file <new-secret-file>`; `archive --vault <dir> --source <file-or-folder> --account <account-id>` (or `--local <dir>` for local testing); `restore --kit <exported.json> --destination <parent> --recovery-key` (add `--local-root <objects-dir>` for a local archive). Secrets are prompted rather than accepted as command-line arguments.

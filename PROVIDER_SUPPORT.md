# Provider support

Google Drive and Dropbox have real desktop OAuth/HTTPS adapters in `crates/app`, but **neither has been verified against an authorized live account**. They are experimental integrations, not release-supported providers. OneDrive, Box, pCloud, MEGA and Filen are not implemented. Local/removable storage is the only adapter with passing round-trip/contract evidence and does not satisfy the consumer-cloud requirement. The machine-readable registry is `docs/connectors.json`. No promotional storage amount is treated as actual capacity.

Do not interpret the provider names used in synthetic tests as evidence of API behavior. Real tests require explicitly authorized test accounts, registrations and suitable platform environments.

## Google desktop registration

Enable Drive API in a Google API project, configure the consent screen and authorized test users as required, and create a **Desktop app** OAuth client. In Cloud Accounts enter its client ID and, if required by that desktop registration, its desktop client value (`client_secret` in the installed JSON). This public-client value is not a confidential server secret. Never put a web application's confidential secret into a distributed build. The CLI can read the installed-client JSON with `connect-google --registration <file>`. Authorization opens in the system browser, uses a random loopback port, PKCE, state, `drive.file`, offline access and account selection. The adapter confirms Drive `permissionId`, queries live quota, and places encrypted objects under an app-created folder. See [Google desktop OAuth](https://developers.google.com/identity/protocols/oauth2/native-app) and [Drive uploads](https://developers.google.com/workspace/drive/api/guides/manage-uploads).

This source build has no official project registration. Production consent approval, Android authorization, two-live-account isolation, cross-client scoped visibility, fresh-device restore, pagination/duplicate-name contracts and expired-session recovery remain unverified. Preserve exported locator kits. Changing API project/client identity may require an additional scoped recovery flow that has not been built.

## Dropbox desktop registration

Create a scoped **App folder** application in the Dropbox app console. Enable `account_info.read`, `files.content.read`, `files.content.write`, and register exactly `http://127.0.0.1:53682/oauth/callback`. Enter the public app key in Cloud Accounts. The app uses PKCE without a confidential client secret, requests offline access, confirms `account_id` and queries quota. Team allocation is shown as unknown to avoid counting shared capacity as individual capacity. See [Dropbox OAuth](https://docs.dropboxapi.com/dropbox-api/docs/oauth) and the [HTTP API reference](https://www.dropbox.com/developers/documentation/http/documentation).

Release app approval, free-account eligibility/limits and all native live-account contracts still need testing. The adapter performs immutable single-object uploads and downloads bounded encrypted objects; it does not yet persist upload-session checkpoints, automatically resume jobs or implement deletion. One-copy archive completion requires full read-back verification, not merely HTTP success.

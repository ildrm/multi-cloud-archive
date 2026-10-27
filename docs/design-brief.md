# CloudWeave Windows implementation brief

Read C:/Users/Mato/.agents/skills/frontend-design/implementation.md and follow its applicable instructions. Build a real Dioxus 0.7.3 desktop Rust UI, not a standalone page. You own only apps/desktop/** and DESIGN.md. Do not edit root Cargo or backend crates. Coordinate compilation changes with parent. No model switch.

Purpose: local-first encrypted archiving for nontechnical Windows users. Visual direction: quiet editorial storage utility; off-white canvas, dark navy sidebar, deep teal primary action, light fine borders, ample spacing, Segoe UI/system typography, no remote fonts or images. Memorable feature: clear compact archive pipeline with four named stages and an honest verification badge. 4/8px spacing; accessible contrast, focus-visible controls; responsive to 360px with logical properties. English and Persian text map from day one, language toggle and dir RTL, isolated paths via bdi. Dark/system theme, reduced-motion rules. No decorative metrics or fake accounts.

Screens: Overview (real zero state, welcome + create vault + open vault), Files/snapshots, Cloud accounts (multiple Google and Dropbox; registration details inputs clearly called advanced source-build setup; no tokens), Transfers (real progress and errors), Recovery (import kit, passphrase OR recovery key, destination picker, restore), Settings (language, theme). Show release limitations succinctly in About. A single-file native file picker and folder picker are needed, multiple source selection ideal. Actions run in tokio::task::spawn_blocking; never block UI with crypto/network/file dialogs. Render all strings as text. Do not enable external WebView navigation or arbitrary evaluation. No false completed states. Source deletion disabled/unimplemented with plain explanation. No fake progress, previews or mocked connectors.

Parent is implementing cloudweave-app and cloudweave-engine with this API. Depend on their paths and ask parent if adjustment needed:

```
// cloudweave_app
pub struct Service { /* Clone + Send */ }
impl Service {
 pub fn open(data_root: PathBuf) -> Result<Self>;
 pub fn accounts(&self) -> Result<Vec<AccountSummary>>;
 pub fn connect_google(&self, client_id: String, desktop_client_value: Option<String>) -> Result<AccountSummary>;
 pub fn connect_dropbox(&self, app_key: String) -> Result<AccountSummary>;
 pub fn refresh_account(&self, account_id: &str) -> Result<AccountSummary>;
 pub fn archive(&self, request: ArchiveRequest, progress: impl Fn(cloudweave_engine::Progress)) -> Result<ArchiveOutcome>;
 pub fn restore(&self, request: RestoreRequest, progress: impl Fn(cloudweave_engine::Progress)) -> Result<RestoreOutcome>;
 pub fn jobs(&self) -> Result<Vec<JobSummary>>;
}
pub struct AccountSummary { pub id: String, pub provider: String, pub subject: String, pub label: String, pub free_bytes: Option<u64> }
pub enum Destination { Local(PathBuf), Account(String) }
pub struct ArchiveRequest { pub sources: Vec<PathBuf>, pub vault_dir: PathBuf, pub passphrase: String, pub destination: Destination }
pub struct RestoreRequest { pub kit_path: PathBuf, pub secret: String, pub use_recovery_key: bool, pub destination: PathBuf }
pub struct ArchiveOutcome { pub snapshot_id: String, pub files: usize, pub original_bytes: u64, pub stored_bytes: u64, pub recovery_kit: PathBuf }
pub struct RestoreOutcome { pub files: usize, pub bytes: u64, pub directory: PathBuf }
pub struct JobSummary { pub id: String, pub kind: String, pub phase: String }
// structs above Clone, Debug; Debug redacted for secrets
// cloudweave_engine
pub fn create_vault(dir: &Path, passphrase: &str) -> Result<String>; // independent 64-hex recovery secret, show and export via user save dialog; never log it
pub fn list_snapshots(vault_dir: &Path) -> Result<Vec<SnapshotSummary>>;
pub struct SnapshotSummary { pub snapshot_id: String, pub files: usize, pub original_bytes: u64, pub stored_bytes: u64, pub recovery_kit: PathBuf }
pub struct Progress { pub phase: String, pub files_done: usize, pub original_bytes: u64, pub stored_bytes: u64 }
```

Use native rfd dialogs. Window config title CloudWeave, around 1200x800 minimum 760x540; prevent unknown top-level navigation if Dioxus API supports. data root via directories::ProjectDirs. Workspace toolchain is cargo +stable (Rust1.97.1 installed). Your Cargo.toml dioxus = { version="=0.7.3", default-features=false, features=["desktop","launch","lib"] }; tokio 1.53.2, rfd stable pinned after cargo info; directories 6.0.0. Cargo dependency network must use require_escalated because sandbox Schannel fails. Fetch/build can be done by parent; don't launch a second cargo build while parent building.

Produce source and design notes, communicate API questions promptly, then verify compilation once parent dependencies ready. Avoid external images, the app needs none. Include screenshot-ready UI with honest empty states. Evaluation from a different provider is unavailable in this session; state that limitation rather than fabricate external review.

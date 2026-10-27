use crate::{
    Error, Result,
    auth::{self, Tokens},
    cloud::{self, CloudStore},
};
use cloudweave_engine::{LocalStore, ObjectStore, Origin, Progress};
use fs2::FileExt;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    path::PathBuf,
    sync::{Arc, Mutex},
};
use uuid::Uuid;
use zeroize::Zeroizing;

type AccountLock = Arc<Mutex<()>>;
type AccountLocks = Arc<Mutex<BTreeMap<String, AccountLock>>>;
#[derive(Clone)]
pub struct Service {
    pub(crate) root: PathBuf,
    locks: AccountLocks,
    _instance: Arc<File>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountSummary {
    pub id: String,
    pub provider: String,
    pub subject: String,
    pub label: String,
    pub free_bytes: Option<u64>,
    pub observed_unix: u64,
}
#[derive(Clone, Debug)]
pub enum Destination {
    Local(PathBuf),
    Account(String),
}
#[derive(Clone)]
pub struct ArchiveRequest {
    pub sources: Vec<PathBuf>,
    pub vault_dir: PathBuf,
    pub passphrase: String,
    pub destination: Destination,
}
#[derive(Clone)]
pub struct RestoreRequest {
    pub kit_path: PathBuf,
    pub secret: String,
    pub use_recovery_key: bool,
    pub destination: PathBuf,
    pub local_root: Option<PathBuf>,
}
impl std::fmt::Debug for ArchiveRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ArchiveRequest { secrets and private paths redacted }")
    }
}
impl std::fmt::Debug for RestoreRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RestoreRequest { secrets and private paths redacted }")
    }
}
#[derive(Clone, Debug)]
pub struct ArchiveOutcome {
    pub snapshot_id: String,
    pub files: usize,
    pub original_bytes: u64,
    pub stored_bytes: u64,
    pub recovery_kit: PathBuf,
}
#[derive(Clone, Debug)]
pub struct RestoreOutcome {
    pub files: usize,
    pub bytes: u64,
    pub directory: PathBuf,
}
#[derive(Clone, Debug, Serialize)]
pub struct JobSummary {
    pub id: String,
    pub kind: String,
    pub phase: String,
}

impl Service {
    pub fn open(data_root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&data_root)?;
        let instance = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(data_root.join("instance.lock"))?;
        instance
            .try_lock_exclusive()
            .map_err(|_| Error::AlreadyRunning)?;
        let service = Self {
            root: data_root,
            locks: Arc::new(Mutex::new(BTreeMap::new())),
            _instance: Arc::new(instance),
        };
        let db = service.database()?;
        let version: u32 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 1 {
            return Err(Error::Invalid);
        }
        db.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS accounts(id TEXT PRIMARY KEY, provider TEXT NOT NULL, subject TEXT NOT NULL, label TEXT NOT NULL, free_bytes TEXT, observed INTEGER NOT NULL, UNIQUE(provider,subject));
            CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY, kind TEXT NOT NULL, phase TEXT NOT NULL);
            PRAGMA user_version=1;")?;
        db.execute("UPDATE jobs SET phase='Interrupted — retry required' WHERE phase NOT IN ('Completed','Failed','Interrupted — retry required')", [])?;
        Ok(service)
    }
    fn database(&self) -> Result<Connection> {
        let db = Connection::open(self.root.join("catalog.sqlite3"))?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA synchronous=FULL;")?;
        Ok(db)
    }
    pub fn accounts(&self) -> Result<Vec<AccountSummary>> {
        let db = self.database()?;
        let mut stmt = db.prepare("SELECT id,provider,subject,label,free_bytes,observed FROM accounts ORDER BY provider,label,id")?;
        let accounts = stmt
            .query_map([], |row| {
                Ok(AccountSummary {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    subject: row.get(2)?,
                    label: row.get(3)?,
                    free_bytes: row
                        .get::<_, Option<String>>(4)?
                        .and_then(|v| v.parse().ok()),
                    observed_unix: row.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(accounts)
    }
    pub fn connect_google(
        &self,
        client_id: String,
        desktop_client_value: Option<String>,
    ) -> Result<AccountSummary> {
        self.connect("google", client_id, desktop_client_value)
    }
    pub fn connect_dropbox(&self, app_key: String) -> Result<AccountSummary> {
        self.connect("dropbox", app_key, None)
    }
    fn connect(
        &self,
        provider: &str,
        client_id: String,
        desktop_client_value: Option<String>,
    ) -> Result<AccountSummary> {
        let http = cloud::http()?;
        let tokens = auth::connect(provider, client_id, desktop_client_value, &http)?;
        let identity = cloud::identity(provider, &tokens.access, &http)?;
        let existing = self
            .accounts()?
            .into_iter()
            .find(|a| a.provider == provider && a.subject == identity.subject);
        let id = existing
            .map(|a| a.id)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let account_lock = self.account_lock(&id)?;
        let _guard = account_lock.lock().map_err(|_| Error::Invalid)?;
        // The provider-confirmed subject selects the existing account. Never use email.
        self.store_tokens(&id, &tokens)?;
        let account = AccountSummary {
            id,
            provider: provider.into(),
            subject: identity.subject,
            label: identity.label,
            free_bytes: identity.free_bytes,
            observed_unix: auth::now(),
        };
        self.database()?.execute("INSERT INTO accounts(id,provider,subject,label,free_bytes,observed) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET label=excluded.label,free_bytes=excluded.free_bytes,observed=excluded.observed", params![account.id, account.provider, account.subject, account.label, account.free_bytes.map(|v|v.to_string()), account.observed_unix])?;
        Ok(account)
    }
    fn entry(&self, id: &str) -> Result<keyring::Entry> {
        keyring::Entry::new("CloudWeave", id).map_err(|_| Error::Credential)
    }
    fn store_tokens(&self, id: &str, tokens: &Tokens) -> Result<()> {
        let encoded = Zeroizing::new(serde_json::to_string(tokens).map_err(|_| Error::Invalid)?);
        self.entry(id)?
            .set_password(&encoded)
            .map_err(|_| Error::Credential)
    }
    fn account_lock(&self, id: &str) -> Result<AccountLock> {
        Ok(self
            .locks
            .lock()
            .map_err(|_| Error::Invalid)?
            .entry(id.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone())
    }
    pub(crate) fn with_token<T>(
        &self,
        account: &AccountSummary,
        action: impl FnOnce(&str) -> Result<T>,
    ) -> Result<T> {
        let account_lock = self.account_lock(&account.id)?;
        let _guard = account_lock.lock().map_err(|_| Error::Invalid)?;
        let encoded = Zeroizing::new(
            self.entry(&account.id)?
                .get_password()
                .map_err(|_| Error::Credential)?,
        );
        let mut tokens: Tokens = serde_json::from_str(&encoded).map_err(|_| Error::Credential)?;
        let needs_refresh = tokens.expires_at <= auth::now().saturating_add(60);
        auth::refresh(&account.provider, &mut tokens, &cloud::http()?)?;
        // Persist a rotated refresh token before making any further request.
        if needs_refresh {
            self.store_tokens(&account.id, &tokens)?;
        }
        action(&tokens.access)
    }
    pub fn refresh_account(&self, account_id: &str) -> Result<AccountSummary> {
        let mut account = self
            .accounts()?
            .into_iter()
            .find(|a| a.id == account_id)
            .ok_or(Error::AccountMissing)?;
        let identity = self.with_token(&account, |token| {
            cloud::identity(&account.provider, token, &cloud::http()?)
        })?;
        if identity.subject != account.subject {
            return Err(Error::IdentityMismatch);
        }
        account.label = identity.label;
        account.free_bytes = identity.free_bytes;
        account.observed_unix = auth::now();
        self.database()?.execute(
            "UPDATE accounts SET label=?2,free_bytes=?3,observed=?4 WHERE id=?1",
            params![
                account.id,
                account.label,
                account.free_bytes.map(|v| v.to_string()),
                account.observed_unix
            ],
        )?;
        Ok(account)
    }
    fn store_for_account(&self, id: &str) -> Result<Box<dyn ObjectStore>> {
        let account = self.refresh_account(id)?;
        Ok(Box::new(CloudStore::new(self.clone(), account)?))
    }
    pub fn jobs(&self) -> Result<Vec<JobSummary>> {
        let db = self.database()?;
        let mut stmt =
            db.prepare("SELECT id,kind,phase FROM jobs ORDER BY rowid DESC LIMIT 100")?;
        Ok(stmt
            .query_map([], |row| {
                Ok(JobSummary {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    phase: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }
    fn start_job(&self, kind: &str) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        self.database()?.execute(
            "INSERT INTO jobs(id,kind,phase) VALUES(?1,?2,'Preparing')",
            params![id, kind],
        )?;
        Ok(id)
    }
    fn phase(&self, id: &str, phase: &str) -> Result<()> {
        self.database()?
            .execute("UPDATE jobs SET phase=?2 WHERE id=?1", params![id, phase])?;
        Ok(())
    }
    pub fn archive(
        &self,
        request: ArchiveRequest,
        progress: impl Fn(Progress),
    ) -> Result<ArchiveOutcome> {
        let passphrase = Zeroizing::new(request.passphrase);
        let job = self.start_job("Archive")?;
        let last = RefCell::new(String::new());
        let checkpoint_error = RefCell::new(None);
        let result = (|| {
            let store: Box<dyn ObjectStore> = match request.destination {
                Destination::Local(root) => Box::new(LocalStore::new(root)?),
                Destination::Account(id) => self.store_for_account(&id)?,
            };
            let result = cloudweave_engine::archive_to(
                &request.vault_dir,
                &passphrase,
                &request.sources,
                store.as_ref(),
                |mut state| {
                    if state.phase == "Completed" {
                        state.phase = "Finalizing".into();
                    }
                    if *last.borrow() != state.phase {
                        if let Err(error) = self.phase(&job, &state.phase) {
                            *checkpoint_error.borrow_mut() = Some(error);
                        }
                        *last.borrow_mut() = state.phase.clone();
                    }
                    progress(state);
                },
            )?;
            if let Some(error) = checkpoint_error.into_inner() {
                return Err(error);
            }
            self.phase(&job, "Completed")?;
            progress(Progress {
                phase: "Completed".into(),
                files_done: result.kit.files,
                original_bytes: result.kit.original_bytes,
                stored_bytes: result.kit.stored_bytes,
            });
            Ok(ArchiveOutcome {
                snapshot_id: result.kit.snapshot_id.to_string(),
                files: result.kit.files,
                original_bytes: result.kit.original_bytes,
                stored_bytes: result.kit.stored_bytes,
                recovery_kit: result.recovery_kit,
            })
        })();
        if result.is_err() {
            let _ = self.phase(&job, "Failed");
        }
        result
    }
    pub fn restore(
        &self,
        request: RestoreRequest,
        progress: impl Fn(Progress),
    ) -> Result<RestoreOutcome> {
        let secret = Zeroizing::new(request.secret);
        let job = self.start_job("Restore")?;
        let result = (|| {
            let mut kit = cloudweave_engine::read_kit(&request.kit_path)?;
            let store: Box<dyn ObjectStore> = match &kit.origin {
                Origin::Local { .. } => {
                    let store =
                        LocalStore::open(request.local_root.ok_or(Error::LocalRootRequired)?)?;
                    kit.origin = store.origin();
                    Box::new(store)
                }
                Origin::Cloud { provider, subject } => {
                    let account = self
                        .accounts()?
                        .into_iter()
                        .find(|a| &a.provider == provider && &a.subject == subject)
                        .ok_or(Error::AccountMissing)?;
                    self.store_for_account(&account.id)?
                }
            };
            self.phase(&job, "Restoring")?;
            let restored = cloudweave_engine::restore_from(
                &kit,
                &secret,
                request.use_recovery_key,
                &request.destination,
                store.as_ref(),
                |mut state| {
                    if state.phase == "Completed" {
                        state.phase = "Finalizing".into();
                    }
                    progress(state);
                },
            )?;
            self.phase(&job, "Completed")?;
            progress(Progress {
                phase: "Completed".into(),
                files_done: restored.files,
                original_bytes: restored.bytes,
                stored_bytes: 0,
            });
            Ok(RestoreOutcome {
                files: restored.files,
                bytes: restored.bytes,
                directory: restored.directory,
            })
        })();
        if result.is_err() {
            let _ = self.phase(&job, "Failed");
        }
        result
    }
}

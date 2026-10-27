use clap::{Parser, Subcommand};
use cloudweave_app::{ArchiveRequest, Destination, RestoreRequest, Service};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};
use zeroize::Zeroizing;

#[derive(Parser)]
#[command(
    name = "cloudweave",
    version,
    about = "Experimental encrypted archive client; source deletion is disabled"
)]
struct Cli {
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Create a vault and write its independent recovery key to a NEW file.
    Init {
        #[arg(long)]
        vault: PathBuf,
        #[arg(long)]
        key_file: PathBuf,
    },
    /// List connected identities (does not refresh quotas).
    Accounts,
    /// Connect Google using an installed/desktop OAuth registration JSON file.
    ConnectGoogle {
        #[arg(long)]
        registration: PathBuf,
    },
    /// Connect Dropbox using an app-folder public-client application key.
    ConnectDropbox {
        #[arg(long)]
        app_key: String,
    },
    /// Archive files/folders with one verified copy. Sources are always retained.
    Archive {
        #[arg(long)]
        vault: PathBuf,
        #[arg(long,required=true,num_args=1..)]
        source: Vec<PathBuf>,
        #[arg(long, conflicts_with = "account", required_unless_present = "account")]
        local: Option<PathBuf>,
        #[arg(long, conflicts_with = "local", required_unless_present = "local")]
        account: Option<String>,
    },
    /// Recover from an exported locator kit, into a new directory without overwriting.
    Restore {
        #[arg(long)]
        kit: PathBuf,
        #[arg(long)]
        destination: PathBuf,
        #[arg(long)]
        recovery_key: bool,
        #[arg(long)]
        local_root: Option<PathBuf>,
    },
    Jobs,
    /// Run an isolated synthetic local archive/restore check (not a cloud test).
    SelfTest {
        #[arg(long)]
        directory: PathBuf,
    },
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let root = cli.data_dir.unwrap_or_else(|| {
        directories::ProjectDirs::from("org", "CloudWeave", "CloudWeave")
            .expect("user data directory")
            .data_local_dir()
            .to_owned()
    });
    // Initialization and isolated synthetic tests don't require an account catalog.
    match cli.command {
        Command::Init { vault, key_file } => {
            let passphrase = Zeroizing::new(rpassword::prompt_password(
                "New passphrase (12+ characters): ",
            )?);
            let confirm = Zeroizing::new(rpassword::prompt_password("Repeat passphrase: ")?);
            if passphrase != confirm {
                return Err("Passphrases do not match".into());
            }
            // Reserve a NEW export file before creating the vault, preventing a
            // successful vault creation with an accidentally overwritten key file.
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(key_file)?;
            let key = Zeroizing::new(cloudweave_engine::create_vault(&vault, &passphrase)?);
            output.write_all(key.as_bytes())?;
            output.write_all(b"\n")?;
            output.sync_all()?;
            println!(
                "Vault created. Keep the independent recovery-key file separate from archive storage."
            );
        }
        Command::SelfTest { directory } => self_test(directory)?,
        command => {
            let service = Service::open(root)?;
            match command {
                Command::Accounts => {
                    println!("{}", serde_json::to_string_pretty(&service.accounts()?)?)
                }
                Command::Jobs => println!("{}", serde_json::to_string_pretty(&service.jobs()?)?),
                Command::ConnectGoogle { registration } => {
                    let raw = Zeroizing::new(cloudweave_engine::read_stream_bounded(
                        fs::File::open(registration)?,
                        16384,
                    )?);
                    let config: serde_json::Value = serde_json::from_slice(&raw)?;
                    let installed = &config["installed"];
                    let id = installed["client_id"]
                        .as_str()
                        .ok_or("Use a Google desktop/installed client JSON")?
                        .to_owned();
                    let value = installed["client_secret"].as_str().map(str::to_owned);
                    let account = service.connect_google(id, value)?;
                    println!("Connected {} · {}", account.provider, account.label);
                }
                Command::ConnectDropbox { app_key } => {
                    let account = service.connect_dropbox(app_key)?;
                    println!("Connected {} · {}", account.provider, account.label);
                }
                Command::Archive {
                    vault,
                    source,
                    local,
                    account,
                } => {
                    let passphrase = rpassword::prompt_password("Vault passphrase: ")?;
                    let destination = match (local, account) {
                        (Some(path), None) => Destination::Local(path),
                        (None, Some(id)) => Destination::Account(id),
                        _ => return Err("Choose one destination".into()),
                    };
                    let result = service.archive(
                        ArchiveRequest {
                            sources: source,
                            vault_dir: vault,
                            passphrase,
                            destination,
                        },
                        |_| {},
                    )?;
                    println!(
                        "Committed and fully read-back verified: {} files, {} original bytes.\nExport this recovery kit: {}",
                        result.files,
                        result.original_bytes,
                        result.recovery_kit.display()
                    );
                }
                Command::Restore {
                    kit,
                    destination,
                    recovery_key,
                    local_root,
                } => {
                    let secret = rpassword::prompt_password(if recovery_key {
                        "Independent recovery key: "
                    } else {
                        "Vault passphrase: "
                    })?;
                    let result = service.restore(
                        RestoreRequest {
                            kit_path: kit,
                            secret,
                            use_recovery_key: recovery_key,
                            destination,
                            local_root,
                        },
                        |_| {},
                    )?;
                    println!(
                        "Restored and verified {} files ({} bytes) to {}",
                        result.files,
                        result.bytes,
                        result.directory.display()
                    );
                }
                _ => unreachable!(),
            }
        }
    }
    Ok(())
}
fn self_test(directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir(&directory)?;
    let source = directory.join("synthetic.txt");
    let expected = "CloudWeave synthetic round trip. یادداشت آزمایشی\n".repeat(10000);
    fs::write(&source, expected.as_bytes())?;
    let vault = directory.join("vault");
    let key = cloudweave_engine::create_vault(&vault, "Synthetic self-test passphrase")?;
    let store = cloudweave_engine::LocalStore::new(directory.join("objects"))?;
    let snapshot = cloudweave_engine::archive_to(
        &vault,
        "Synthetic self-test passphrase",
        &[source],
        &store,
        |_| {},
    )?;
    let kit = cloudweave_engine::read_kit(&snapshot.recovery_kit)?;
    let restored = cloudweave_engine::restore_from(&kit, &key, true, &directory, &store, |_| {})?;
    if fs::read(restored.directory.join("synthetic.txt"))? != expected.as_bytes() {
        return Err("Round-trip bytes differ".into());
    }
    println!(
        "PASS: synthetic local archive, full read-back verification, recovery-key restore and byte comparison.\n{} original bytes; {} stored bytes.\nThis does not verify a cloud provider. Artifacts: {}",
        kit.original_bytes,
        kit.stored_bytes,
        directory.display()
    );
    Ok(())
}

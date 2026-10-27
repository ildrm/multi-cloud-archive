#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use cloudweave_app::{
    AccountSummary, ArchiveOutcome, ArchiveRequest, Destination, JobSummary, RestoreOutcome,
    RestoreRequest, Service,
};
use cloudweave_engine::{Progress, SnapshotSummary};
use dioxus::core::spawn_forever;
use dioxus::desktop::{Config, WindowBuilder};
use dioxus::prelude::*;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use zeroize::Zeroize;

fn main() {
    // Dioxus permits its first internal document before this callback;
    // this hook rejects non-HTTP external schemes inside the WebView.
    // HTTP(S) links are handled by Dioxus in the system browser.
    let config = Config::new()
        .with_custom_index(include_str!("index.html").to_owned())
        .with_navigation_handler(|_| false)
        .with_window(
            WindowBuilder::new()
                .with_title("CloudWeave")
                .with_inner_size(dioxus::desktop::LogicalSize::new(1200.0, 820.0))
                .with_min_inner_size(dioxus::desktop::LogicalSize::new(760.0, 540.0)),
        );
    // Opt-in local inspection is available only in Windows debug builds, for
    // synthetic-data diagnostics. Release builds never read this switch.
    #[cfg(all(debug_assertions, target_os = "windows"))]
    let config = match std::env::var("CLOUDWEAVE_INSPECT_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port > 1024)
    {
        Some(port) => config.with_windows_browser_args(format!(
            "--remote-debugging-address=127.0.0.1 --remote-debugging-port={port}"
        )),
        None => config,
    };
    dioxus::LaunchBuilder::desktop()
        .with_cfg(config)
        .launch(App);
}

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Overview,
    Files,
    Accounts,
    Transfers,
    Recovery,
    Settings,
}
#[derive(Clone, Copy, PartialEq)]
enum Lang {
    En,
    Fa,
}
#[derive(Clone, Copy)]
enum Pick {
    Vault,
    Store,
    Kit,
    Restore,
    RestoreStore,
}

enum Event {
    Ready(Result<(Service, Vec<AccountSummary>, Vec<JobSummary>), String>),
    Picked(Pick, Option<PathBuf>, Vec<SnapshotSummary>),
    Sources(Vec<PathBuf>),
    Created(PathBuf, String),
    Accounts(Vec<AccountSummary>),
    Progress(Progress),
    Archived(ArchiveOutcome, Vec<SnapshotSummary>, Vec<JobSummary>),
    Restored(RestoreOutcome, Vec<JobSummary>),
    SavedSecret,
    SavedKit,
    Journal(Vec<JobSummary>),
    Failed(String),
    Cancelled,
}

struct UiState {
    service: Option<Service>,
    tx: mpsc::SyncSender<Event>,
    page: Page,
    lang: Lang,
    theme: String,
    busy: bool,
    notice: Option<String>,
    error: bool,
    accounts: Vec<AccountSummary>,
    jobs: Vec<JobSummary>,
    snapshots: Vec<SnapshotSummary>,
    vault: Option<PathBuf>,
    sources: Vec<PathBuf>,
    destination: String,
    store: Option<PathBuf>,
    kit: Option<PathBuf>,
    restore_dir: Option<PathBuf>,
    local_restore_root: Option<PathBuf>,
    vault_passphrase: String,
    passphrase_confirm: String,
    restore_secret: String,
    use_recovery_key: bool,
    recovery_key: String,
    recovery_saved: bool,
    google_client: String,
    google_value: String,
    dropbox_key: String,
    progress: Option<Progress>,
    clock_unix: u64,
}

impl UiState {
    fn new(tx: mpsc::SyncSender<Event>) -> Self {
        Self {
            service: None,
            tx,
            page: Page::Overview,
            lang: Lang::En,
            theme: "system".into(),
            busy: true,
            notice: None,
            error: false,
            accounts: vec![],
            jobs: vec![],
            snapshots: vec![],
            vault: None,
            sources: vec![],
            destination: String::new(),
            store: None,
            kit: None,
            restore_dir: None,
            local_restore_root: None,
            vault_passphrase: String::new(),
            passphrase_confirm: String::new(),
            restore_secret: String::new(),
            use_recovery_key: false,
            recovery_key: String::new(),
            recovery_saved: true,
            google_client: String::new(),
            google_value: String::new(),
            dropbox_key: String::new(),
            progress: None,
            clock_unix: unix_now(),
        }
    }
}
impl Drop for UiState {
    fn drop(&mut self) {
        self.vault_passphrase.zeroize();
        self.passphrase_confirm.zeroize();
        self.restore_secret.zeroize();
        self.recovery_key.zeroize();
        self.google_value.zeroize();
    }
}

fn run_task(
    mut state: Signal<UiState>,
    work: impl FnOnce(mpsc::SyncSender<Event>) -> Event + Send + 'static,
) {
    let tx = state.read().tx.clone();
    let service = state.read().service.clone();
    {
        let mut s = state.write();
        s.busy = true;
        s.notice = None;
        s.error = false;
    }
    // Operations belong to the application lifetime, not the page that started
    // them: navigating to Transfers must not cancel the worker launch future.
    spawn_forever(execute_worker(tx, service, work));
}

async fn execute_worker(
    tx: mpsc::SyncSender<Event>,
    service: Option<Service>,
    work: impl FnOnce(mpsc::SyncSender<Event>) -> Event + Send + 'static,
) {
    let _ = tokio::task::spawn_blocking(move || {
        let progress_tx = tx;
            let event = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(progress_tx.clone())))
                .unwrap_or_else(|_| Event::Failed("The operation stopped unexpectedly. Your source files were kept. Restart and retry the operation.".into()));
            if let Some(service) = service
                && let Ok(jobs) = service.jobs()
            {
                let _ = progress_tx.try_send(Event::Journal(jobs));
            }
            // Terminal delivery may wait for space, but only on this blocking
            // worker. The Dioxus runtime must remain free to drain the queue.
            let _ = progress_tx.send(event);
    })
    .await;
}

fn fail(mut state: Signal<UiState>, message: &'static str) {
    let mut s = state.write();
    s.notice = Some(tr(s.lang, message).into());
    s.error = true;
}

fn pick(state: Signal<UiState>, which: Pick) {
    run_task(state, move |_| {
        let dialog = rfd::FileDialog::new();
        let path = match which {
            Pick::Kit => dialog.add_filter("Recovery kit", &["json"]).pick_file(),
            _ => dialog.pick_folder(),
        };
        let snapshots = if matches!(which, Pick::Vault) {
            if let Some(vault) = &path {
                if let Err(error) = cloudweave_engine::read_header(vault) {
                    return Event::Failed(error.to_string());
                }
                match cloudweave_engine::list_snapshots(vault) {
                    Ok(snapshots) => snapshots,
                    Err(error) => return Event::Failed(error.to_string()),
                }
            } else {
                vec![]
            }
        } else {
            vec![]
        };
        Event::Picked(which, path, snapshots)
    });
}

fn copy_kit(state: Signal<UiState>, source: PathBuf) {
    run_task(state, move |_| {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("cloudweave-recovery-kit.json")
            .add_filter("Recovery kit", &["json"])
            .save_file()
        else {
            return Event::Cancelled;
        };
        let result = (|| -> std::io::Result<()> {
            let mut input = std::fs::File::open(source)?;
            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?;
            std::io::copy(&mut input, &mut output)?;
            output.sync_all()
        })();
        match result { Ok(()) => Event::SavedKit, Err(_) => Event::Failed("Could not export the kit. Choose a new filename in a writable folder; existing files are never overwritten.".into()) }
    });
}

#[component]
fn App() -> Element {
    let (tx, rx) = use_hook(|| {
        let (tx, rx) = mpsc::sync_channel(64);
        (tx, Arc::new(Mutex::new(rx)))
    });
    let mut state = use_signal(|| UiState::new(tx.clone()));
    let locale = use_memo(move || state.read().lang);
    use_effect(move || {
        // Fixed application-authored scripts only; no user data is interpolated.
        let script = match locale() {
            Lang::En => {
                "document.documentElement.lang = 'en'; document.documentElement.dir = 'ltr';"
            }
            Lang::Fa => {
                "document.documentElement.lang = 'fa'; document.documentElement.dir = 'rtl';"
            }
        };
        let _ = document::eval(script);
    });
    use_future(move || {
        let tx = tx.clone();
        async move {
            let _ = tokio::task::spawn_blocking(move || {
                let event = std::panic::catch_unwind(|| {
                    let root = directories::ProjectDirs::from("org", "CloudWeave", "CloudWeave")
                        .ok_or_else(|| {
                            "Could not find an application data directory.".to_string()
                        })?;
                    let service = Service::open(root.data_local_dir().to_path_buf())
                        .map_err(|e| e.to_string())?;
                    let accounts = service.accounts().map_err(|e| e.to_string())?;
                    let jobs = service.jobs().map_err(|e| e.to_string())?;
                    Ok((service, accounts, jobs))
                })
                .unwrap_or_else(|_| Err("Application setup failed.".into()));
                let _ = tx.send(Event::Ready(event));
            })
            .await;
        }
    });
    use_future(move || {
        let rx = rx.clone();
        async move {
            let mut clock_tick = std::time::Instant::now();
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                if clock_tick.elapsed() >= std::time::Duration::from_secs(60) {
                    state.write().clock_unix = unix_now();
                    clock_tick = std::time::Instant::now();
                }
                let events: Vec<_> = match rx.lock() {
                    Ok(rx) => rx.try_iter().collect(),
                    Err(_) => vec![],
                };
                for event in events {
                    let mut s = state.write();
                    if !matches!(&event, Event::Progress(_) | Event::Journal(_)) {
                        s.busy = false;
                    }
                    match event {
                        Event::Ready(Ok((service, accounts, jobs))) => { s.service = Some(service); s.accounts = accounts; s.jobs = jobs; }
                        Event::Ready(Err(error)) | Event::Failed(error) => { s.notice = Some(error); s.error = true; s.vault_passphrase.zeroize(); s.passphrase_confirm.zeroize(); s.restore_secret.zeroize(); s.google_value.zeroize(); }
                        Event::Picked(which, Some(path), snapshots) => match which {
                            Pick::Vault => { s.vault = Some(path); s.snapshots = snapshots; s.recovery_key.zeroize(); s.recovery_saved = true; }
                            Pick::Store => s.store = Some(path), Pick::Kit => { s.kit = Some(path); s.local_restore_root = None; s.restore_secret.zeroize(); }, Pick::Restore => s.restore_dir = Some(path), Pick::RestoreStore => s.local_restore_root = Some(path),
                        },
                        Event::Sources(paths) => if !paths.is_empty() { s.sources = paths; },
                        Event::Created(path, key) => { s.vault = Some(path); s.recovery_key = key; s.recovery_saved = false; s.vault_passphrase.zeroize(); s.passphrase_confirm.zeroize(); s.notice = Some(tr(s.lang, "Vault created. Export your recovery key before archiving.").into()); }
                        Event::Accounts(accounts) => { s.accounts = accounts; s.google_value.zeroize(); s.notice = Some(tr(s.lang, "Connection verified. Each account keeps its own credentials.").into()); }
                        Event::Progress(mut progress) => { if progress.phase == "Completed" { progress.phase = "Finalizing".into(); } s.progress = Some(progress); },
                        Event::Archived(outcome, snapshots, jobs) => { s.kit = Some(outcome.recovery_kit); s.snapshots = snapshots; s.jobs = jobs; s.vault_passphrase.zeroize(); s.progress = Some(Progress { phase: "Completed".into(), files_done: outcome.files, original_bytes: outcome.original_bytes, stored_bytes: outcome.stored_bytes }); s.notice = Some(format!("{} · {} {} · {}", tr(s.lang, "Snapshot committed and read-back verified"), outcome.files, tr(s.lang, "files"), human_bytes(outcome.original_bytes))); }
                        Event::Restored(outcome, jobs) => { s.jobs = jobs; s.restore_secret.zeroize(); s.progress = Some(Progress { phase: "Completed".into(), files_done: outcome.files, original_bytes: outcome.bytes, stored_bytes: 0 }); s.notice = Some(format!("{} · {} {} · {}", tr(s.lang, "Restore verified"), outcome.files, tr(s.lang, "files"), outcome.directory.display())); }
                        Event::SavedSecret => { s.recovery_saved = true; s.recovery_key.zeroize(); s.notice = Some(tr(s.lang, "Recovery key exported. Keep it apart from your archive storage.").into()); }
                        Event::SavedKit => s.notice = Some(tr(s.lang, "Recovery kit exported. Keep your passphrase or recovery key separately.").into()),
                        Event::Journal(jobs) => s.jobs = jobs,
                        Event::Picked(_, None, _) | Event::Cancelled => {},
                    }
                }
            }
        }
    });
    let s = state.read();
    let lang = s.lang;
    let theme = s.theme.clone();
    let page = s.page;
    let busy = s.busy;
    let title = match page {
        Page::Overview => "Overview",
        Page::Files => "Snapshots",
        Page::Accounts => "Cloud accounts",
        Page::Transfers => "Transfers",
        Page::Recovery => "Recovery",
        Page::Settings => "Settings",
    };
    let vault_label = s
        .vault
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned());
    drop(s);
    rsx! {
        style { {include_str!("style.css")} }
        div { class: "shell", "data-theme": theme, dir: if lang == Lang::Fa { "rtl" } else { "ltr" }, lang: if lang == Lang::Fa { "fa" } else { "en" }, onmounted: move |_| { if std::env::var_os("CLOUDWEAVE_SMOKE_TEST").is_some() { eprintln!("CloudWeave UI mounted"); } },
            aside { class: "sidebar",
                div { class: "brand", span { class: "brand-mark", "aria-hidden": "true", "W" } div { strong { "CloudWeave" } small { {tr(lang, "Your files. Your accounts.")} } } }
                nav { "aria-label": tr(lang, "Main navigation"),
                    for (target, icon, label) in [(Page::Overview,"◈","Overview"), (Page::Files,"▤","Snapshots"), (Page::Accounts,"◎","Cloud accounts"), (Page::Transfers,"⇄","Transfers"), (Page::Recovery,"◇","Recovery"), (Page::Settings,"⚙","Settings")] {
                        button { class: if page == target { "nav-item active" } else { "nav-item" }, "aria-label": tr(lang, label), "aria-current": if page == target { "page" } else { "false" }, onclick: move |_| state.write().page = target,
                            span { class: "nav-icon", "aria-hidden": "true", "{icon}" } span { {tr(lang, label)} }
                        }
                    }
                }
                div { class: "sidebar-foot", span { class: "privacy-dot", "aria-hidden": "true" } p { strong { {tr(lang, "Local first")} } small { {tr(lang, "No project account. No telemetry.")} } } }
            }
            div { class: "workspace",
                header { class: "topbar", div { class: "breadcrumb", span { "CloudWeave" } span { " / " } strong { {tr(lang, title)} } }
                    div { class: "topbar-right",
                        span { class: "build-tag", {tr(lang, "Development build")} }
                        button { class: "language-button", "aria-label": tr(lang, "Change language"), onclick: move |_| { state.write().lang = if lang == Lang::En { Lang::Fa } else { Lang::En }; }, if lang == Lang::En { "فارسی" } else { "English" } }
                    }
                }
                main { id: "main-content",
                    if let Some(notice) = state.read().notice.clone() {
                        div { class: if state.read().error { "notice error" } else { "notice success" }, role: if state.read().error { "alert" } else { "status" },
                            p { "{notice}" } button { class: "dismiss", "aria-label": tr(lang, "Dismiss"), onclick: move |_| state.write().notice = None, "×" }
                        }
                    }
                    if busy { div { class: "busy-line", role: "status", span { class: "spinner", "aria-hidden": "true" } {tr(lang, "Working… You can continue browsing.")} } }
                    match page {
                        Page::Overview => rsx! { Overview { state } }, Page::Files => rsx! { Snapshots { state } },
                        Page::Accounts => rsx! { CloudAccounts { state } }, Page::Transfers => rsx! { Transfers { state } },
                        Page::Recovery => rsx! { Recovery { state } }, Page::Settings => rsx! { Settings { state } },
                    }
                }
                footer { class: "footer", span { {tr(lang, "Sources are always kept on your device.")} } span { if let Some(label) = vault_label { bdi { "{label}" } } else { {tr(lang, "No vault selected")} } } }
            }
        }
    }
}

#[component]
fn Overview(mut state: Signal<UiState>) -> Element {
    let s = state.read();
    let lang = s.lang;
    let busy = s.busy;
    let ready = s.service.is_some();
    let has_vault = s.vault.is_some();
    let source_count = s.sources.len();
    let destination = s.destination.clone();
    let can_archive = !busy
        && ready
        && has_vault
        && source_count > 0
        && !s.vault_passphrase.is_empty()
        && s.recovery_saved
        && (!destination.is_empty())
        && (destination != "local" || s.store.is_some());
    let sources = s.sources.clone();
    let accounts = s.accounts.clone();
    let recovery_key = s.recovery_key.clone();
    drop(s);
    rsx! {
        div { class: "page-heading", div { p { class: "eyebrow", {tr(lang, "PRIVATE BY DESIGN")} } h1 { {tr(lang, "Make room for what’s next.")} } p { class: "intro", {tr(lang, "Compress and encrypt your files, then store a recoverable snapshot in your own storage.")} } }
            button { class: "button secondary", disabled: busy || !state.read().recovery_saved, onclick: move |_| pick(state, Pick::Vault), {tr(lang, "Open vault")} }
        }
        section { class: "pipeline", "aria-label": tr(lang, "Archive stages"),
            for (number, name, detail) in [("01","Prepare","Read stable source files"),("02","Protect","Compress, encrypt, upload"),("03","Verify","Read back every object"),("04","Commit","Publish recovery metadata")] {
                div { class: "pipeline-step", span { class: "step-number", "{number}" } div { strong { {tr(lang, name)} } small { {tr(lang, detail)} } } }
            }
        }
        if !recovery_key.is_empty() {
            section { class: "recovery-callout", "aria-labelledby": "key-title", h2 { id: "key-title", {tr(lang, "Save your independent recovery key")} } p { {tr(lang, "This key can unlock your vault if you lose its passphrase. Keep it away from your archive storage. It is shown only until exported.")} }
                textarea { class: "secret-code", readonly: true, rows: "2", dir: "ltr", "aria-label": tr(lang, "Recovery key"), value: "{recovery_key}" }
                button { class: "button primary", disabled: busy, onclick: move |_| {
                    let mut key = state.read().recovery_key.clone();
                    run_task(state, move |_| {
                        let Some(path) = rfd::FileDialog::new().set_file_name("cloudweave-recovery.key").save_file() else { key.zeroize(); return Event::Cancelled; };
                        let result = (|| -> std::io::Result<()> { let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?; output.write_all(key.as_bytes())?; output.sync_all() })(); key.zeroize();
                        match result { Ok(()) => Event::SavedSecret, Err(_) => Event::Failed("Could not export the key. Choose a new filename in a writable folder.".into()) }
                    });
                }, {tr(lang, "Export recovery key")} }
            }
        }
        div { class: "work-grid",
            section { class: "panel", h2 { span { class: "section-index", "1" } {tr(lang, "Your vault")} }
                p { class: "muted", {tr(lang, "Vault settings stay on this device. Export a recovery kit after every snapshot.")} }
                PathField { label: tr(lang, "Vault folder"), path: state.read().vault.clone() }
                if !has_vault {
                    label { r#for: "new-passphrase", {tr(lang, "Passphrase")} }
                    input { id: "new-passphrase", r#type: "password", autocomplete: "new-password", value: "{state.read().vault_passphrase}", oninput: move |e| state.write().vault_passphrase = e.value() }
                    label { r#for: "confirm-passphrase", {tr(lang, "Confirm passphrase")} }
                    input { id: "confirm-passphrase", r#type: "password", autocomplete: "new-password", value: "{state.read().passphrase_confirm}", oninput: move |e| state.write().passphrase_confirm = e.value() }
                    button { class: "button primary", disabled: busy, onclick: move |_| {
                        if state.read().vault_passphrase.len() < 12 { fail(state, "Use a passphrase of at least 12 characters."); return; }
                        if state.read().vault_passphrase != state.read().passphrase_confirm { fail(state, "Passphrases do not match."); return; }
                        let mut passphrase = state.read().vault_passphrase.clone();
                        run_task(state, move |_| { let Some(path) = rfd::FileDialog::new().pick_folder() else { passphrase.zeroize(); return Event::Cancelled; }; let result = cloudweave_engine::create_vault(&path, &passphrase); passphrase.zeroize(); match result { Ok(key) => Event::Created(path, key), Err(e) => Event::Failed(e.to_string()) } });
                    }, {tr(lang, "Create encrypted vault")} }
                } else {
                    label { r#for: "vault-passphrase", {tr(lang, "Vault passphrase")} }
                    input { id: "vault-passphrase", r#type: "password", autocomplete: "current-password", value: "{state.read().vault_passphrase}", oninput: move |e| state.write().vault_passphrase = e.value() }
                    p { class: "hint", {tr(lang, "Used only for this operation; cleared when it finishes.")} }
                    button { class: "text-button", disabled: busy || !state.read().recovery_saved, onclick: move |_| { let mut s = state.write(); s.vault = None; s.snapshots.clear(); s.vault_passphrase.zeroize(); s.passphrase_confirm.zeroize(); }, {tr(lang, "Create another vault")} }
                }
            }
            section { class: "panel", h2 { span { class: "section-index", "2" } {tr(lang, "Files to archive")} }
                p { class: "muted", {tr(lang, "Choose files or one folder. Originals are kept; links are rejected for safety.")} }
                div { class: "button-row", button { class: "button secondary", disabled: busy, onclick: move |_| run_task(state, |_| Event::Sources(rfd::FileDialog::new().pick_files().unwrap_or_default())), {tr(lang, "Choose files")} }
                    button { class: "button secondary", disabled: busy, onclick: move |_| run_task(state, |_| Event::Sources(rfd::FileDialog::new().pick_folder().into_iter().collect())), {tr(lang, "Choose folder")} }
                }
                if source_count == 0 { div { class: "source-empty", span { "＋" } p { {tr(lang, "Your selection will appear here.")} } } }
                else { ul { class: "source-list", for path in sources { li { bdi { "{path.display()}" } } } } }
            }
            section { class: "panel full-width", h2 { span { class: "section-index", "3" } {tr(lang, "Where to store it")} }
                div { class: "destination-grid", div {
                    label { r#for: "destination", {tr(lang, "Destination")} }
                    select { id: "destination", value: "{destination}", onchange: move |e| state.write().destination = e.value(),
                        option { value: "", {tr(lang, "Choose a connected account")} }
                        for account in accounts { option { value: "{account.id}", "{account.provider} · {account.label}" } }
                        option { value: "local", {tr(lang, "Local test destination")} }
                    }
                    if destination == "local" { p { class: "hint", {tr(lang, "For development round trips only. This does not provide cloud storage or protection against device loss.")} }
                        PathField { label: tr(lang, "Object folder"), path: state.read().store.clone() }
                        button { class: "button secondary", disabled: busy, onclick: move |_| pick(state, Pick::Store), {tr(lang, "Choose object folder")} }
                    }
                    if state.read().accounts.is_empty() { button { class: "text-button", onclick: move |_| state.write().page = Page::Accounts, {tr(lang, "Connect a cloud account →")} } }
                }
                div { class: "policy-summary", span { class: "badge neutral", {tr(lang, "One copy")} } h3 { {tr(lang, "Every byte verified")} } p { {tr(lang, "A snapshot completes only after uploaded data is read back, decrypted and checked. A single destination does not survive loss of that account.")} } }
                }
                div { class: "action-bar", p { {tr(lang, "Storage savings depend on your files. Compressed media may not get smaller.")} }
                    button { class: "button primary", disabled: !can_archive, onclick: move |_| {
                        let s = state.read(); let Some(service) = s.service.clone() else { return; }; let Some(vault_dir) = s.vault.clone() else { return; };
                        let destination = if s.destination == "local" { let Some(path) = s.store.clone() else { return; }; Destination::Local(path) } else { Destination::Account(s.destination.clone()) };
                        let request = ArchiveRequest { sources: s.sources.clone(), vault_dir: vault_dir.clone(), passphrase: s.vault_passphrase.clone(), destination }; drop(s);
                        state.write().page = Page::Transfers;
                        state.write().progress = None;
                        run_task(state, move |tx| match service.archive(request, |p| { let _ = tx.try_send(Event::Progress(p)); }) {
                            Ok(outcome) => Event::Archived(outcome, cloudweave_engine::list_snapshots(&vault_dir).unwrap_or_default(), service.jobs().unwrap_or_default()), Err(e) => Event::Failed(e.to_string())
                        });
                    }, {tr(lang, "Archive and verify")} span { " →" } }
                }
            }
        }
    }
}

#[component]
fn PathField(label: &'static str, path: Option<PathBuf>) -> Element {
    rsx! { div { class: "path-field", span { "{label}" } bdi { {path.map(|p| p.display().to_string()).unwrap_or_else(|| "—".into())} } } }
}

#[component]
fn Snapshots(mut state: Signal<UiState>) -> Element {
    let s = state.read();
    let lang = s.lang;
    let busy = s.busy;
    let snapshots = s.snapshots.clone();
    drop(s);
    rsx! {
        div { class: "page-heading", div { p { class: "eyebrow", {tr(lang, "YOUR ARCHIVE")} } h1 { {tr(lang, "Snapshots")} } p { class: "intro", {tr(lang, "Immutable snapshots from your selected vault. Export their kits for another device.")} } } button { class: "button secondary", disabled: busy || !state.read().recovery_saved, onclick: move |_| pick(state, Pick::Vault), {tr(lang, "Open vault")} } }
        if snapshots.is_empty() { EmptyState { title: tr(lang, "Your first snapshot starts here."), body: tr(lang, "Open a vault to see its committed snapshots, or create one from Overview."), action: tr(lang, "Go to Overview"), onclick: move |_| state.write().page = Page::Overview } }
        else { section { class: "panel snapshot-table", div { class: "table-head", span { {tr(lang, "Snapshot")} } span { {tr(lang, "Original size")} } span { {tr(lang, "Actions")} } }
            for snapshot in snapshots { div { class: "snapshot-row", div { strong { bdi { "{snapshot.snapshot_id}" } } small { "{snapshot.files} " {tr(lang, "files")} } } span { bdi { {human_bytes(snapshot.original_bytes)} } }
                div { class: "button-row", button { class: "button secondary compact", disabled: busy, onclick: { let path = snapshot.recovery_kit.clone(); move |_| { let mut s = state.write(); s.kit = Some(path.clone()); s.local_restore_root = None; s.restore_secret.zeroize(); s.page = Page::Recovery; } }, {tr(lang, "Restore")} }
                    button { class: "text-button", disabled: busy, onclick: { let path = snapshot.recovery_kit.clone(); move |_| copy_kit(state, path.clone()) }, {tr(lang, "Export kit")} }
                }
            } }
        } }
    }
}

#[component]
fn CloudAccounts(mut state: Signal<UiState>) -> Element {
    let s = state.read();
    let lang = s.lang;
    let busy = s.busy;
    let accounts = s.accounts.clone();
    let clock_unix = s.clock_unix;
    drop(s);
    rsx! {
        div { class: "page-heading", div { p { class: "eyebrow", {tr(lang, "YOUR STORAGE")} } h1 { {tr(lang, "Cloud accounts")} } p { class: "intro", {tr(lang, "Connect another account without replacing an existing one. Credentials stay in the operating system’s secure store.")} } } }
        if accounts.is_empty() { div { class: "empty-inline", span { class: "empty-symbol", "◎" } div { h2 { {tr(lang, "No accounts connected yet")} } p { {tr(lang, "Add a provider registration below, then authorize in your system browser.")} } } } }
        else { section { class: "account-list", for account in accounts { article { class: "account-row", div { class: "provider-symbol", "{account.provider.chars().next().unwrap_or('C')}" } div { class: "account-identity", h2 { "{account.label}" } p { "{account.provider} · " bdi { "{account.subject}" } } }
            div { class: "quota", strong { bdi { {account.free_bytes.map(human_bytes).unwrap_or_else(|| tr(lang, "Unknown").into())} } } small { {tr(lang, "free at last check")} } small { {quota_age(lang, account.observed_unix, clock_unix)} } }
            button { class: "button secondary compact", disabled: busy, onclick: { let id = account.id.clone(); move |_| { let Some(service) = state.read().service.clone() else { return; }; let id = id.clone(); run_task(state, move |_| match service.refresh_account(&id).and_then(|_| service.accounts()) { Ok(accounts) => Event::Accounts(accounts), Err(e) => Event::Failed(e.to_string()) }); } }, {tr(lang, "Refresh")} }
        } } } }
        section { class: "panel registration", h2 { {tr(lang, "Add another account")} } p { class: "muted", {tr(lang, "This source build needs your own OAuth application registration. Production Connect registrations are not configured yet.")} }
            details { open: true, summary { "Google Drive" }, p { class: "hint", {tr(lang, "Use a Desktop OAuth client with Drive API enabled. The browser asks which account to authorize; app-created files use drive.file access.")} }
                label { r#for: "google-client", {tr(lang, "Desktop client ID")} } input { id: "google-client", value: "{state.read().google_client}", autocomplete: "off", spellcheck: "false", dir: "ltr", oninput: move |e| state.write().google_client = e.value() }
                label { r#for: "google-client-value", {tr(lang, "Desktop client value (if required by registration)")} } input { id: "google-client-value", r#type: "password", value: "{state.read().google_value}", autocomplete: "off", oninput: move |e| state.write().google_value = e.value() }
                button { class: "button primary", disabled: busy || state.read().google_client.trim().is_empty(), onclick: move |_| { let s = state.read(); let Some(service) = s.service.clone() else { return; }; let client = s.google_client.trim().to_owned(); let value = if s.google_value.is_empty() { None } else { Some(s.google_value.clone()) }; drop(s); run_task(state, move |_| match service.connect_google(client, value).and_then(|_| service.accounts()) { Ok(accounts) => Event::Accounts(accounts), Err(e) => Event::Failed(e.to_string()) }); }, {tr(lang, "Connect Google account")} }
            }
            details { summary { "Dropbox" }, p { class: "hint", {tr(lang, "Create a scoped App folder registration and enable account information, file metadata, read and write permissions. Add the loopback redirect described in PROVIDER_SUPPORT.md.")} }
                label { r#for: "dropbox-key", {tr(lang, "Dropbox app key")} } input { id: "dropbox-key", value: "{state.read().dropbox_key}", autocomplete: "off", spellcheck: "false", dir: "ltr", oninput: move |e| state.write().dropbox_key = e.value() }
                button { class: "button primary", disabled: busy || state.read().dropbox_key.trim().is_empty(), onclick: move |_| { let s = state.read(); let Some(service) = s.service.clone() else { return; }; let key = s.dropbox_key.trim().to_owned(); drop(s); run_task(state, move |_| match service.connect_dropbox(key).and_then(|_| service.accounts()) { Ok(accounts) => Event::Accounts(accounts), Err(e) => Event::Failed(e.to_string()) }); }, {tr(lang, "Connect Dropbox account")} }
            }
        }
    }
}

#[component]
fn Transfers(state: Signal<UiState>) -> Element {
    let s = state.read();
    let lang = s.lang;
    let busy = s.busy;
    let jobs = s.jobs.clone();
    let progress = s.progress.as_ref().map(|p| {
        (
            p.phase.clone(),
            p.files_done,
            p.original_bytes,
            p.stored_bytes,
        )
    });
    drop(s);
    rsx! {
        div { class: "page-heading", div { p { class: "eyebrow", {tr(lang, "ACTIVITY")} } h1 { {tr(lang, "Transfers")} } p { class: "intro", {tr(lang, "Completion means the snapshot and its recovery metadata have been verified.")} } } }
        if let Some((phase, files, original, stored)) = progress { section { class: "panel transfer-card", div { class: "transfer-title", h2 { {tr(lang, "Current operation")} } span { class: if phase == "Completed" { "badge" } else { "badge neutral" }, {phase_label(lang, &phase)} } }
            div { class: "transfer-metrics", div { small { {tr(lang, "Files processed")} } strong { bdi { "{files}" } } } div { small { {tr(lang, "Original bytes processed")} } strong { bdi { {human_bytes(original)} } } } div { small { {tr(lang, "Stored bytes processed")} } strong { bdi { {human_bytes(stored)} } } } }
            p { class: "hint", if busy { {tr(lang, "The operation is running. Closing the application can interrupt it; a partial snapshot is never reported as complete.")} } else { {tr(lang, "See the operation result above and the durable journal below.")} } }
        } }
        if jobs.is_empty() { EmptyState { title: tr(lang, "A clear view of every transfer."), body: tr(lang, "Archive and restore jobs will appear here. Nothing is transferred until you start an operation."), action: tr(lang, "Go to Overview"), onclick: move |_| { let mut state = state; state.write().page = Page::Overview; } } }
        else { section { class: "panel", h2 { {tr(lang, "Job journal")} } div { class: "job-list", for job in jobs { div { class: "job-row", div { strong { {phase_label(lang, &job.kind)} } small { bdi { "{job.id}" } } } span { class: "badge neutral", {phase_label(lang, &job.phase)} } } } } } }
    }
}

#[component]
fn Recovery(mut state: Signal<UiState>) -> Element {
    let s = state.read();
    let lang = s.lang;
    let busy = s.busy;
    let can_restore = !busy
        && s.service.is_some()
        && s.kit.is_some()
        && s.restore_dir.is_some()
        && !s.restore_secret.is_empty();
    drop(s);
    rsx! {
        div { class: "page-heading", div { p { class: "eyebrow", {tr(lang, "A WAY BACK")} } h1 { {tr(lang, "Recover your files.")} } p { class: "intro", {tr(lang, "Import a snapshot’s kit and unlock it with your passphrase or independent recovery key. Reconnect its cloud account first.")} } } }
        section { class: "panel recovery-panel", h2 { {tr(lang, "Restore a snapshot")} }
            PathField { label: tr(lang, "Recovery kit"), path: state.read().kit.clone() }
            button { class: "button secondary", disabled: busy, onclick: move |_| pick(state, Pick::Kit), {tr(lang, "Import recovery kit")} }
            PathField { label: tr(lang, "Local archive storage (for local archives)"), path: state.read().local_restore_root.clone() }
            button { class: "button secondary", disabled: busy, onclick: move |_| pick(state, Pick::RestoreStore), {tr(lang, "Choose local archive storage")} }
            p { class: "hint", {tr(lang, "For a local archive, select the folder containing its encrypted objects. Paths inside an imported kit are never trusted automatically. Cloud archives use their reconnected account.")} }
            label { class: "checkbox-label", input { r#type: "checkbox", checked: state.read().use_recovery_key, onchange: move |e| { state.write().use_recovery_key = e.checked(); state.write().restore_secret.zeroize(); } } {tr(lang, "Use my independent recovery key")} }
            label { r#for: "restore-secret", if state.read().use_recovery_key { {tr(lang, "Recovery key")} } else { {tr(lang, "Vault passphrase")} } }
            input { id: "restore-secret", r#type: "password", autocomplete: "off", value: "{state.read().restore_secret}", oninput: move |e| state.write().restore_secret = e.value() }
            PathField { label: tr(lang, "Restore destination"), path: state.read().restore_dir.clone() }
            button { class: "button secondary", disabled: busy, onclick: move |_| pick(state, Pick::Restore), {tr(lang, "Choose destination folder")} }
            div { class: "info-strip", {tr(lang, "Restored files go into a new folder. Existing files are never silently overwritten. Every restored chunk is checked.")} }
            button { class: "button primary", disabled: !can_restore, onclick: move |_| { let s = state.read(); let Some(service) = s.service.clone() else { return; }; let (Some(kit_path), Some(destination)) = (s.kit.clone(), s.restore_dir.clone()) else { return; }; let request = RestoreRequest { kit_path, destination, local_root: s.local_restore_root.clone(), secret: s.restore_secret.clone(), use_recovery_key: s.use_recovery_key }; drop(s); state.write().page = Page::Transfers; state.write().progress = None; run_task(state, move |tx| match service.restore(request, |p| { let _ = tx.try_send(Event::Progress(p)); }) { Ok(outcome) => Event::Restored(outcome, service.jobs().unwrap_or_default()), Err(e) => Event::Failed(e.to_string()) }); }, {tr(lang, "Restore and verify")} }
        }
        section { class: "recovery-note", h2 { {tr(lang, "Keep two things, separately.")} } p { {tr(lang, "The recovery kit locates a snapshot and contains encrypted key envelopes. Your passphrase or recovery key unlocks it. Cloud authorization is still required on a new device.")} } }
    }
}

#[component]
fn Settings(mut state: Signal<UiState>) -> Element {
    let lang = state.read().lang;
    rsx! {
        div { class: "page-heading", div { p { class: "eyebrow", {tr(lang, "MAKE IT YOURS")} } h1 { {tr(lang, "Settings")} } p { class: "intro", {tr(lang, "Display preferences apply to this session.")} } } }
        section { class: "panel settings-panel", h2 { {tr(lang, "Appearance")} }
            label { r#for: "theme", {tr(lang, "Theme")} } select { id: "theme", value: "{state.read().theme}", onchange: move |e| state.write().theme = e.value(), option { value: "system", {tr(lang, "System")} } option { value: "light", {tr(lang, "Light")} } option { value: "dark", {tr(lang, "Dark")} } }
            label { r#for: "language", {tr(lang, "Language")} } select { id: "language", value: if lang == Lang::Fa { "fa" } else { "en" }, onchange: move |e| state.write().lang = if e.value() == "fa" { Lang::Fa } else { Lang::En }, option { value: "en", "English" } option { value: "fa", "فارسی" } }
        }
        section { class: "panel settings-panel", h2 { {tr(lang, "About this build")} } p { {tr(lang, "An early Windows development build. Live cloud accounts, Android, other operating systems, accessibility and release security still need independent validation. See STATUS.md for evidence and gaps.")} }
            dl { class: "about-list", dt { {tr(lang, "Privacy")} } dd { {tr(lang, "No telemetry or project account")} } dt { {tr(lang, "Source removal")} } dd { {tr(lang, "Unavailable; your originals are kept")} } dt { {tr(lang, "Protection policy")} } dd { {tr(lang, "One copy; provider-loss replication is not enabled")} } dt { {tr(lang, "License")} } dd { "GPL-2.0" } }
        }
    }
}

#[component]
fn EmptyState(
    title: &'static str,
    body: &'static str,
    action: &'static str,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    rsx! { section { class: "empty-state", div { class: "empty-symbol", "◇" } h2 { "{title}" } p { "{body}" } button { class: "button secondary", onclick, "{action}" } } }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn quota_age(lang: Lang, observed: u64, now: u64) -> String {
    if observed == 0 {
        return if lang == Lang::Fa {
            "زمان بررسی نامشخص"
        } else {
            "Observation time unknown"
        }
        .into();
    }
    if observed > now.saturating_add(60) {
        return if lang == Lang::Fa {
            "زمان بررسی با ساعت دستگاه متفاوت است"
        } else {
            "Observation time differs from device clock"
        }
        .into();
    }
    let seconds = now.saturating_sub(observed);
    match (lang, seconds) {
        (Lang::En, 0..60) => "Checked just now".into(),
        (Lang::Fa, 0..60) => "همین حالا بررسی شد".into(),
        (Lang::En, 60..3600) => format!("Checked {} min ago", seconds / 60),
        (Lang::Fa, 60..3600) => format!("{} دقیقه پیش بررسی شد", seconds / 60),
        (Lang::En, 3600..86400) => format!("Checked {} h ago", seconds / 3600),
        (Lang::Fa, 3600..86400) => format!("{} ساعت پیش بررسی شد", seconds / 3600),
        (Lang::En, _) => format!("Checked {} days ago", seconds / 86400),
        (Lang::Fa, _) => format!("{} روز پیش بررسی شد", seconds / 86400),
    }
}

fn phase_label(lang: Lang, phase: &str) -> String {
    if lang == Lang::En {
        return phase.into();
    }
    match phase.to_lowercase().as_str() {
        "archive" => "بایگانی",
        "restore" => "بازیابی",
        "preparing" | "planned" => "آماده‌سازی",
        "encoding" | "reading" => "رمزگذاری",
        "uploading" => "بارگذاری",
        "verifying" => "بررسی",
        "finalizing" | "committing" => "ثبت نهایی",
        "completed" => "تکمیل‌شده",
        "restoring" => "بازیابی",
        "failed" => "ناموفق",
        "paused" | "interrupted" => "متوقف‌شده",
        "cancelled" => "لغوشده",
        "needsauthorization" => "نیاز به مجوز",
        _ => phase,
    }
    .into()
}

fn tr(lang: Lang, english: &'static str) -> &'static str {
    if lang == Lang::En {
        return english;
    }
    match english {
        "Overview" => "نمای کلی",
        "Snapshots" => "نسخه‌های بایگانی",
        "Cloud accounts" => "حساب‌های ابری",
        "Transfers" => "انتقال‌ها",
        "Recovery" => "بازیابی",
        "Settings" => "تنظیمات",
        "Your files. Your accounts." => "فایل‌های شما، حساب‌های شما.",
        "Main navigation" => "ناوبری اصلی",
        "Local first" => "اولویت با دستگاه شما",
        "No project account. No telemetry." => "بدون حساب پروژه و داده‌کاوی.",
        "Development build" => "نسخهٔ در حال توسعه",
        "Change language" => "تغییر زبان",
        "Dismiss" => "بستن",
        "Working… You can continue browsing." => "در حال انجام… می‌توانید به مرور ادامه دهید.",
        "Sources are always kept on your device." => {
            "فایل‌های اصلی همواره روی دستگاه شما حفظ می‌شوند."
        }
        "No vault selected" => "خزانه‌ای انتخاب نشده",
        "PRIVATE BY DESIGN" => "طراحی‌شده برای حریم خصوصی",
        "Make room for what’s next." => "برای چیزهای تازه جا باز کنید.",
        "Compress and encrypt your files, then store a recoverable snapshot in your own storage." => {
            "فایل‌ها را فشرده و رمزگذاری کنید و نسخه‌ای قابل بازیابی در فضای خود نگه دارید."
        }
        "Open vault" => "باز کردن خزانه",
        "Archive stages" => "مراحل بایگانی",
        "Prepare" => "آماده‌سازی",
        "Read stable source files" => "خواندن فایل‌های پایدار",
        "Protect" => "محافظت",
        "Compress, encrypt, upload" => "فشرده‌سازی، رمزگذاری، بارگذاری",
        "Verify" => "بررسی",
        "Read back every object" => "بازخوانی همهٔ داده‌ها",
        "Commit" => "ثبت نهایی",
        "Publish recovery metadata" => "ثبت فرادادهٔ بازیابی",
        "Your vault" => "خزانهٔ شما",
        "Vault settings stay on this device. Export a recovery kit after every snapshot." => {
            "تنظیمات خزانه روی این دستگاه می‌ماند. پس از هر نسخه، بستهٔ بازیابی را صادر کنید."
        }
        "Vault folder" => "پوشهٔ خزانه",
        "Passphrase" => "عبارت عبور",
        "Confirm passphrase" => "تکرار عبارت عبور",
        "Create encrypted vault" => "ساخت خزانهٔ رمزگذاری‌شده",
        "Vault passphrase" => "عبارت عبور خزانه",
        "Used only for this operation; cleared when it finishes." => {
            "فقط برای این عملیات استفاده و پس از پایان پاک می‌شود."
        }
        "Files to archive" => "فایل‌های بایگانی",
        "Choose files or one folder. Originals are kept; links are rejected for safety." => {
            "فایل‌ها یا یک پوشه را انتخاب کنید. اصل فایل‌ها حفظ و پیوندها برای ایمنی رد می‌شوند."
        }
        "Choose files" => "انتخاب فایل‌ها",
        "Choose folder" => "انتخاب پوشه",
        "Your selection will appear here." => "موارد انتخاب‌شده اینجا نمایش داده می‌شوند.",
        "Where to store it" => "محل نگه‌داری",
        "Destination" => "مقصد",
        "Choose a connected account" => "یک حساب متصل را انتخاب کنید",
        "Local test destination" => "مقصد محلی آزمایشی",
        "For development round trips only. This does not provide cloud storage or protection against device loss." => {
            "فقط برای آزمون توسعه است؛ فضای ابری یا حفاظت در برابر از دست رفتن دستگاه فراهم نمی‌کند."
        }
        "Object folder" => "پوشهٔ داده‌ها",
        "Choose object folder" => "انتخاب پوشهٔ داده‌ها",
        "Connect a cloud account →" => "اتصال حساب ابری ←",
        "One copy" => "یک نسخه",
        "Every byte verified" => "بررسی تک‌تک بایت‌ها",
        "A snapshot completes only after uploaded data is read back, decrypted and checked. A single destination does not survive loss of that account." => {
            "ثبت نهایی پس از بازخوانی، رمزگشایی و بررسی داده‌ها انجام می‌شود. یک مقصد در برابر از دست رفتن همان حساب مقاوم نیست."
        }
        "Storage savings depend on your files. Compressed media may not get smaller." => {
            "صرفه‌جویی به فایل‌ها بستگی دارد؛ رسانه‌های فشرده ممکن است کوچک‌تر نشوند."
        }
        "Archive and verify" => "بایگانی و بررسی",
        "Save your independent recovery key" => "کلید بازیابی مستقل را ذخیره کنید",
        "This key can unlock your vault if you lose its passphrase. Keep it away from your archive storage. It is shown only until exported." => {
            "اگر عبارت عبور را گم کنید، این کلید خزانه را باز می‌کند. آن را جدا از محل بایگانی نگه دارید. پس از صدور نمایش داده نمی‌شود."
        }
        "Recovery key" => "کلید بازیابی",
        "Export recovery key" => "صدور کلید بازیابی",
        "Use a passphrase of at least 12 characters." => {
            "عبارت عبور باید دست‌کم ۱۲ نویسه داشته باشد."
        }
        "Passphrases do not match." => "عبارت‌های عبور یکسان نیستند.",
        "Create another vault" => "ساخت خزانهٔ دیگر",
        "YOUR ARCHIVE" => "بایگانی شما",
        "Immutable snapshots from your selected vault. Export their kits for another device." => {
            "نسخه‌های تغییرناپذیر خزانهٔ انتخاب‌شده. بسته‌های آن‌ها را برای دستگاه دیگر صادر کنید."
        }
        "Your first snapshot starts here." => "اولین نسخهٔ بایگانی از اینجا آغاز می‌شود.",
        "Open a vault to see its committed snapshots, or create one from Overview." => {
            "برای دیدن نسخه‌های ثبت‌شده خزانه را باز کنید یا از نمای کلی خزانه بسازید."
        }
        "Go to Overview" => "رفتن به نمای کلی",
        "Snapshot" => "نسخهٔ بایگانی",
        "Original size" => "اندازهٔ اصلی",
        "Actions" => "عملیات",
        "files" => "فایل",
        "Restore" => "بازیابی",
        "Export kit" => "صدور بسته",
        "YOUR STORAGE" => "فضای شما",
        "Connect another account without replacing an existing one. Credentials stay in the operating system’s secure store." => {
            "حساب دیگری را بدون جایگزینی حساب قبلی متصل کنید. اعتبارنامه‌ها در مخزن امن سیستم‌عامل می‌مانند."
        }
        "No accounts connected yet" => "هنوز حسابی متصل نشده",
        "Add a provider registration below, then authorize in your system browser." => {
            "ثبت برنامهٔ ارائه‌دهنده را در زیر وارد و در مرورگر سیستم مجوز دهید."
        }
        "Unknown" => "نامشخص",
        "free at last check" => "فضای آزاد در آخرین بررسی",
        "Refresh" => "به‌روزرسانی",
        "Add another account" => "افزودن حساب دیگر",
        "This source build needs your own OAuth application registration. Production Connect registrations are not configured yet." => {
            "این نسخه به ثبت برنامهٔ OAuth خودتان نیاز دارد. ثبت اتصال عمومی هنوز آماده نیست."
        }
        "Use a Desktop OAuth client with Drive API enabled. The browser asks which account to authorize; app-created files use drive.file access." => {
            "از کلاینت OAuth دسکتاپ با Drive API فعال استفاده کنید. حساب را در مرورگر انتخاب می‌کنید؛ فایل‌های برنامه از دسترسی drive.file استفاده می‌کنند."
        }
        "Desktop client ID" => "شناسهٔ کلاینت دسکتاپ",
        "Desktop client value (if required by registration)" => {
            "مقدار کلاینت دسکتاپ (اگر ثبت برنامه نیاز دارد)"
        }
        "Connect Google account" => "اتصال حساب گوگل",
        "Create a scoped App folder registration and enable account information, file metadata, read and write permissions. Add the loopback redirect described in PROVIDER_SUPPORT.md." => {
            "برنامه‌ای با دسترسی App folder و مجوز اطلاعات حساب، فراداده، خواندن و نوشتن ثبت کنید. آدرس بازگشت محلی را طبق PROVIDER_SUPPORT.md اضافه کنید."
        }
        "Dropbox app key" => "کلید برنامهٔ دراپ‌باکس",
        "Connect Dropbox account" => "اتصال حساب دراپ‌باکس",
        "ACTIVITY" => "فعالیت",
        "Completion means the snapshot and its recovery metadata have been verified." => {
            "تکمیل یعنی نسخه و فرادادهٔ بازیابی آن بررسی شده‌اند."
        }
        "Current operation" => "عملیات جاری",
        "Files processed" => "فایل‌های پردازش‌شده",
        "Original bytes processed" => "حجم اصلی پردازش‌شده",
        "Stored bytes processed" => "حجم ذخیره‌شدهٔ پردازش‌شده",
        "The operation is running. Closing the application can interrupt it; a partial snapshot is never reported as complete." => {
            "عملیات در حال انجام است. بستن برنامه می‌تواند آن را قطع کند؛ نسخهٔ ناقص هرگز کامل گزارش نمی‌شود."
        }
        "See the operation result above and the durable journal below." => {
            "نتیجهٔ عملیات را در بالا و گزارش پایدار را در پایین ببینید."
        }
        "A clear view of every transfer." => "نمای روشن همهٔ انتقال‌ها.",
        "Archive and restore jobs will appear here. Nothing is transferred until you start an operation." => {
            "عملیات بایگانی و بازیابی اینجا نمایش داده می‌شوند. انتقال فقط با شروع عملیات انجام می‌شود."
        }
        "Job journal" => "گزارش عملیات",
        "A WAY BACK" => "راه بازگشت",
        "Recover your files." => "فایل‌هایتان را بازیابی کنید.",
        "Import a snapshot’s kit and unlock it with your passphrase or independent recovery key. Reconnect its cloud account first." => {
            "بستهٔ نسخه را وارد و با عبارت عبور یا کلید مستقل باز کنید. ابتدا حساب ابری مربوط را دوباره متصل کنید."
        }
        "Restore a snapshot" => "بازیابی یک نسخه",
        "Recovery kit" => "بستهٔ بازیابی",
        "Import recovery kit" => "ورود بستهٔ بازیابی",
        "Local archive storage (for local archives)" => "فضای بایگانی محلی (برای بایگانی‌های محلی)",
        "Choose local archive storage" => "انتخاب فضای بایگانی محلی",
        "For a local archive, select the folder containing its encrypted objects. Paths inside an imported kit are never trusted automatically. Cloud archives use their reconnected account." => {
            "برای بایگانی محلی، پوشهٔ داده‌های رمزگذاری‌شده را انتخاب کنید. مسیرهای داخل بستهٔ واردشده خودکار پذیرفته نمی‌شوند. بایگانی ابری از حساب دوباره‌متصل‌شده استفاده می‌کند."
        }
        "Use my independent recovery key" => "استفاده از کلید بازیابی مستقل",
        "Restore destination" => "مقصد بازیابی",
        "Choose destination folder" => "انتخاب پوشهٔ مقصد",
        "Restored files go into a new folder. Existing files are never silently overwritten. Every restored chunk is checked." => {
            "فایل‌ها در پوشه‌ای تازه بازیابی می‌شوند. فایل‌های موجود بی‌اجازه جایگزین نمی‌شوند و هر بخش بررسی می‌شود."
        }
        "Restore and verify" => "بازیابی و بررسی",
        "Keep two things, separately." => "دو چیز را جداگانه نگه دارید.",
        "The recovery kit locates a snapshot and contains encrypted key envelopes. Your passphrase or recovery key unlocks it. Cloud authorization is still required on a new device." => {
            "بستهٔ بازیابی نسخه را پیدا می‌کند و کلیدهای رمزگذاری‌شده را دارد. عبارت عبور یا کلید بازیابی آن را باز می‌کند. روی دستگاه تازه همچنان مجوز حساب ابری لازم است."
        }
        "MAKE IT YOURS" => "به سلیقهٔ شما",
        "Display preferences apply to this session." => "تنظیمات نمایش برای این نشست اعمال می‌شود.",
        "Appearance" => "ظاهر",
        "Theme" => "پوسته",
        "System" => "سیستم",
        "Light" => "روشن",
        "Dark" => "تیره",
        "Language" => "زبان",
        "About this build" => "دربارهٔ این نسخه",
        "An early Windows development build. Live cloud accounts, Android, other operating systems, accessibility and release security still need independent validation. See STATUS.md for evidence and gaps." => {
            "نسخهٔ اولیهٔ توسعه برای ویندوز. حساب‌های واقعی، اندروید، سایر سیستم‌ها، دسترس‌پذیری و امنیت انتشار به ارزیابی مستقل نیاز دارند. شواهد و کمبودها در STATUS.md آمده‌اند."
        }
        "Privacy" => "حریم خصوصی",
        "No telemetry or project account" => "بدون داده‌کاوی یا حساب پروژه",
        "Source removal" => "حذف فایل اصلی",
        "Unavailable; your originals are kept" => "در دسترس نیست؛ فایل‌های اصلی حفظ می‌شوند",
        "Protection policy" => "سیاست محافظت",
        "One copy; provider-loss replication is not enabled" => {
            "یک نسخه؛ تکثیر میان ارائه‌دهندگان فعال نیست"
        }
        "License" => "مجوز",
        "Vault created. Export your recovery key before archiving." => {
            "خزانه ساخته شد. پیش از بایگانی کلید بازیابی را صادر کنید."
        }
        "Connection verified. Each account keeps its own credentials." => {
            "اتصال بررسی شد. اعتبارنامهٔ هر حساب جداست."
        }
        "Snapshot committed and read-back verified" => "نسخه ثبت و با بازخوانی بررسی شد",
        "Restore verified" => "بازیابی بررسی شد",
        "Recovery key exported. Keep it apart from your archive storage." => {
            "کلید بازیابی صادر شد. آن را جدا از بایگانی نگه دارید."
        }
        "Recovery kit exported. Keep your passphrase or recovery key separately." => {
            "بستهٔ بازیابی صادر شد. عبارت عبور یا کلید را جداگانه نگه دارید."
        }
        _ => english,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn saturated_progress_queue_keeps_runtime_responsive_and_delivers_terminal_event() {
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(Event::Cancelled).unwrap();
        let (tick_tx, tick_rx) = mpsc::channel();
        // An independent watchdog drains the full queue even if a regression
        // blocks the single-thread runtime, so this test fails instead of hanging.
        let observer = std::thread::spawn(move || {
            let runtime_responsive = tick_rx.recv_timeout(Duration::from_secs(3)).is_ok();
            assert!(matches!(
                rx.recv_timeout(Duration::from_secs(3)).unwrap(),
                Event::Cancelled
            ));
            let terminal_delivered = matches!(
                rx.recv_timeout(Duration::from_secs(3)).unwrap(),
                Event::SavedKit
            );
            (runtime_responsive, terminal_delivered)
        });
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(async move {
                let worker = tokio::spawn(execute_worker(tx, None, |tx| {
                    assert!(tx.try_send(Event::Progress(Progress::default())).is_err());
                    Event::SavedKit
                }));
                tokio::time::sleep(Duration::from_millis(20)).await;
                let _ = tick_tx.send(());
                worker.await.unwrap();
            });
        let (runtime_responsive, terminal_delivered) = observer.join().unwrap();
        assert!(
            runtime_responsive,
            "terminal delivery blocked the UI runtime"
        );
        assert!(
            terminal_delivered,
            "terminal event was lost when the queue filled"
        );
    }
}

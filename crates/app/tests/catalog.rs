use cloudweave_app::*;

#[test]
fn restart_marks_incomplete_jobs_interrupted_and_preserves_success() {
    let temp = tempfile::tempdir().unwrap();
    let service = Service::open(temp.path().to_owned()).unwrap();
    assert!(Service::open(temp.path().to_owned()).is_err());
    let db = rusqlite::Connection::open(temp.path().join("catalog.sqlite3")).unwrap();
    db.execute(
        "INSERT INTO jobs(id,kind,phase) VALUES('unfinished','Archive','Uploading')",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO jobs(id,kind,phase) VALUES('finished','Restore','Completed')",
        [],
    )
    .unwrap();
    drop(db);
    drop(service);
    let restarted = Service::open(temp.path().to_owned()).unwrap();
    let jobs = restarted.jobs().unwrap();
    assert_eq!(
        jobs.iter().find(|j| j.id == "unfinished").unwrap().phase,
        "Interrupted — retry required"
    );
    assert_eq!(
        jobs.iter().find(|j| j.id == "finished").unwrap().phase,
        "Completed"
    );
}

#[test]
fn request_debug_never_contains_secret_or_source_paths() {
    let request = RestoreRequest {
        kit_path: "private-path".into(),
        secret: "password-should-never-leak".into(),
        use_recovery_key: false,
        destination: "private-destination".into(),
        local_root: None,
    };
    let debug = format!("{request:?}");
    assert!(!debug.contains("password-should"));
    assert!(!debug.contains("private-path"));
}

#[test]
fn imported_local_kit_cannot_create_or_open_its_storage_hint() {
    let temp = tempfile::tempdir().unwrap();
    let untrusted_root = temp.path().join("must-not-be-created");
    let kit = cloudweave_engine::RecoveryKit {
        version: 1,
        snapshot_id: uuid::Uuid::new_v4(),
        header: cloudweave_engine::VaultHeader {
            version: 1,
            vault_id: uuid::Uuid::new_v4(),
            salt: [0; 16],
            memory_kib: 8192,
            iterations: 1,
            parallelism: 1,
            passphrase_envelope: vec![0; 72],
            recovery_envelope: vec![0; 72],
        },
        header_locator: cloudweave_engine::ObjectLocator {
            id: "header".into(),
        },
        manifest: cloudweave_engine::ObjectLocator {
            id: "manifest".into(),
        },
        origin: cloudweave_engine::Origin::Local {
            root: untrusted_root.clone(),
        },
        files: 0,
        original_bytes: 0,
        stored_bytes: 0,
    };
    let kit_path = temp.path().join("imported.json");
    std::fs::write(&kit_path, serde_json::to_vec(&kit).unwrap()).unwrap();
    let service = Service::open(temp.path().join("app")).unwrap();
    let request = RestoreRequest {
        kit_path,
        secret: "wrong secret".into(),
        use_recovery_key: false,
        destination: temp.path().to_owned(),
        local_root: None,
    };
    assert!(matches!(
        service.restore(request, |_| {}),
        Err(Error::LocalRootRequired)
    ));
    assert!(!untrusted_root.exists());
    assert_eq!(service.jobs().unwrap()[0].phase, "Failed");
}

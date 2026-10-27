use cloudweave_engine::*;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

const PASS: &str = "synthetic test passphrase only";

#[test]
fn fresh_install_recovery_dedup_corruption_and_wrong_secrets() {
    let temp = tempfile::tempdir().unwrap();
    let vault = temp.path().join("vault");
    let recovery_key = create_vault(&vault, PASS).unwrap();
    let sources = temp.path().join("عکس‌ها");
    fs::create_dir(&sources).unwrap();
    let content = vec![42u8; 2 * 1024 * 1024 + 13];
    fs::write(sources.join("تصویر.bin"), &content).unwrap();
    fs::write(sources.join("same.bin"), &content).unwrap();
    fs::write(sources.join("empty.txt"), []).unwrap();
    let store = LocalStore::new(temp.path().join("objects")).unwrap();
    let result = archive_to(&vault, PASS, &[sources], &store, |_| {}).unwrap();
    assert_eq!(result.kit.files, 3);
    assert_eq!(result.kit.original_bytes, (content.len() * 2) as u64);
    // Two unique content chunks + immutable header and manifest; dedup before AEAD.
    assert_eq!(
        fs::read_dir(temp.path().join("objects")).unwrap().count(),
        4
    );
    let kit_copy = temp.path().join("exported-kit.json");
    fs::copy(&result.recovery_kit, &kit_copy).unwrap();
    drop(result);
    fs::rename(&vault, temp.path().join("old-device-unavailable")).unwrap();
    // Recovery requires neither vault directory nor any local catalog/keyring.
    let independent = read_kit(&kit_copy).unwrap();
    let out = temp.path().join("restore");
    fs::create_dir(&out).unwrap();
    assert!(restore_from(&independent, "wrong password", false, &out, &store, |_| {}).is_err());
    assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
    let restored = restore_from(&independent, &recovery_key, true, &out, &store, |_| {}).unwrap();
    assert_eq!(
        fs::read(restored.directory.join("عکس‌ها/تصویر.bin")).unwrap(),
        content
    );
    assert_eq!(
        fs::read(restored.directory.join("عکس‌ها/same.bin")).unwrap(),
        content
    );
    assert_eq!(
        fs::read(restored.directory.join("عکس‌ها/empty.txt")).unwrap(),
        Vec::<u8>::new()
    );
    assert_eq!(restored.bytes, (content.len() * 2) as u64);
    let chunk = fs::read_dir(temp.path().join("objects"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            let name = p.file_name().unwrap().to_str().unwrap();
            !name.ends_with("header") && !name.ends_with("manifest")
        })
        .unwrap();
    let mut bytes = fs::read(&chunk).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(chunk, bytes).unwrap();
    assert!(matches!(
        restore_from(&independent, &recovery_key, true, &out, &store, |_| {}),
        Err(Error::Integrity)
    ));
    assert_eq!(
        fs::read(restored.directory.join("عکس‌ها/تصویر.bin")).unwrap(),
        content
    );
}

#[test]
fn hostile_kdf_is_rejected_before_allocation() {
    let temp = tempfile::tempdir().unwrap();
    let vault = temp.path().join("vault");
    create_vault(&vault, PASS).unwrap();
    let path = vault.join("vault.json");
    let mut header: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    header["memory_kib"] = u32::MAX.into();
    fs::write(path, serde_json::to_vec(&header).unwrap()).unwrap();
    assert!(matches!(read_header(&vault), Err(Error::Format)));
}

struct MutatingStore {
    inner: LocalStore,
    source: PathBuf,
    changed: AtomicBool,
}
impl ObjectStore for MutatingStore {
    fn origin(&self) -> Origin {
        self.inner.origin()
    }
    fn put(&self, name: &str, data: &[u8]) -> Result<ObjectLocator> {
        let object = self.inner.put(name, data)?;
        if !name.ends_with("header") && !self.changed.swap(true, Ordering::SeqCst) {
            fs::write(&self.source, b"changed while uploading").unwrap();
        }
        Ok(object)
    }
    fn get(&self, locator: &ObjectLocator, max: usize) -> Result<Vec<u8>> {
        self.inner.get(locator, max)
    }
}
#[test]
fn changed_source_never_commits_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let vault = temp.path().join("vault");
    create_vault(&vault, PASS).unwrap();
    let source = temp.path().join("synthetic.bin");
    fs::write(&source, vec![1u8; 1024 * 1024 + 100]).unwrap();
    let store = MutatingStore {
        inner: LocalStore::new(temp.path().join("objects")).unwrap(),
        source: source.clone(),
        changed: AtomicBool::new(false),
    };
    assert!(matches!(
        archive_to(&vault, PASS, std::slice::from_ref(&source), &store, |_| {}),
        Err(Error::SourceChanged)
    ));
    assert!(list_snapshots(&vault).unwrap().is_empty());
    assert!(source.exists());
}

#[test]
fn local_contract_is_immutable_bounded_and_rejects_traversal() {
    let temp = tempfile::tempdir().unwrap();
    let store = LocalStore::new(temp.path().to_owned()).unwrap();
    let locator = store.put("opaque-object", b"123456789").unwrap();
    assert!(store.put("opaque-object", b"replacement").is_err());
    assert_eq!(store.get(&locator, 9).unwrap(), b"123456789");
    assert!(store.get(&locator, 8).is_err());
    assert!(
        store
            .get(
                &ObjectLocator {
                    id: "../outside".into()
                },
                100
            )
            .is_err()
    );
}

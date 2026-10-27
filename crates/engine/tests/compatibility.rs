use cloudweave_engine::*;
use std::{fs, path::PathBuf};

#[test]
fn restores_committed_v1_fixture_without_original_catalog_or_paths() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/v1");
    let store = LocalStore::open(fixture.join("objects").canonicalize().unwrap()).unwrap();
    let mut kit = read_kit(&fixture.join("snapshot.json")).unwrap();
    kit.origin = store.origin();
    let destination = tempfile::tempdir().unwrap();
    let result = restore_from(
        &kit,
        "Synthetic self-test passphrase",
        false,
        destination.path(),
        &store,
        |_| {},
    )
    .unwrap();
    let expected = "CloudWeave synthetic round trip. یادداشت آزمایشی\n".repeat(10000);
    assert_eq!(
        fs::read(result.directory.join("synthetic.txt")).unwrap(),
        expected.as_bytes()
    );
    assert_eq!(result.bytes, 630000);
}

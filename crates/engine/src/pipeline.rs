use crate::{
    Error, ObjectLocator, ObjectStore, Origin, Result, VaultHeader,
    crypto::{derive, open, seal},
    read_header,
    store::read_bounded,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;
use zeroize::Zeroizing;

pub const CHUNK_BYTES: usize = 1024 * 1024;
const MAX_MANIFEST: usize = 8 * 1024 * 1024;
const MAX_FILES: usize = 20000;
const MAX_CHUNKS: usize = 50000;

#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub phase: String,
    pub files_done: usize,
    pub original_bytes: u64,
    pub stored_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryKit {
    pub version: u32,
    pub snapshot_id: Uuid,
    pub header: VaultHeader,
    pub header_locator: ObjectLocator,
    pub manifest: ObjectLocator,
    pub origin: Origin,
    pub files: usize,
    pub original_bytes: u64,
    pub stored_bytes: u64,
}

#[derive(Clone, Debug)]
pub struct SnapshotSummary {
    pub snapshot_id: String,
    pub files: usize,
    pub original_bytes: u64,
    pub stored_bytes: u64,
    pub recovery_kit: PathBuf,
}
pub struct ArchiveResult {
    pub kit: RecoveryKit,
    pub recovery_kit: PathBuf,
}
pub struct RestoreResult {
    pub files: usize,
    pub bytes: u64,
    pub directory: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    vault_id: Uuid,
    snapshot_id: Uuid,
    files: Vec<FileRecord>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileRecord {
    path: String,
    bytes: u64,
    chunks: Vec<Chunk>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Chunk {
    id: String,
    raw_bytes: usize,
    codec: Codec,
    locator: ObjectLocator,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Codec {
    Raw,
    Zstd,
}

pub fn read_kit(path: &Path) -> Result<RecoveryKit> {
    let kit: RecoveryKit =
        serde_json::from_slice(&read_bounded(path, 32768)?).map_err(|_| Error::Format)?;
    if kit.version != 1 {
        return Err(Error::Format);
    }
    Ok(kit)
}
pub fn list_snapshots(vault_dir: &Path) -> Result<Vec<SnapshotSummary>> {
    let dir = vault_dir.join("snapshots");
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut snapshots = Vec::new();
    for entry in fs::read_dir(dir)?.take(MAX_FILES + 1) {
        let entry = entry?;
        if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|ext| ext == "json")
        {
            let kit = read_kit(&entry.path())?;
            snapshots.push(SnapshotSummary {
                snapshot_id: kit.snapshot_id.to_string(),
                files: kit.files,
                original_bytes: kit.original_bytes,
                stored_bytes: kit.stored_bytes,
                recovery_kit: entry.path(),
            });
        }
    }
    if snapshots.len() > MAX_FILES {
        return Err(Error::Format);
    }
    snapshots.sort_by(|a, b| a.snapshot_id.cmp(&b.snapshot_id));
    Ok(snapshots)
}

pub fn archive_to(
    vault_dir: &Path,
    passphrase: &str,
    sources: &[PathBuf],
    store: &dyn ObjectStore,
    progress: impl Fn(Progress),
) -> Result<ArchiveResult> {
    let header = read_header(vault_dir)?;
    let master = header.unlock(passphrase, false)?;
    let content_key = derive(&master, "cloudweave v1 content encryption");
    let metadata_key = derive(&master, "cloudweave v1 metadata encryption");
    let identity_key = derive(&master, "cloudweave v1 content identity");
    let snapshot_id = Uuid::new_v4();
    let mut state = Progress {
        phase: "Preparing".into(),
        ..Default::default()
    };
    progress(state.clone());
    let mut entries = Vec::new();
    let mut budget = CollectionBudget {
        remaining_nodes: MAX_FILES,
        logical_bytes: 0,
    };
    for source in sources {
        if budget.remaining_nodes == 0 {
            return Err(Error::Format);
        }
        budget.remaining_nodes -= 1;
        let name = source
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or(Error::UnsafePath)?;
        collect(source, name.into(), 0, &mut entries, &mut budget)?;
    }
    if entries.is_empty() {
        return Err(Error::Format);
    }
    validate_paths(entries.iter().map(|(_, path)| path.as_str()))?;
    // Upload bootstrap before chunks; keep its actual native locator.
    let header_bytes = serde_json::to_vec(&header).map_err(|_| Error::Format)?;
    let header_locator = store.put(
        &format!("{}-{snapshot_id}-header", header.vault_id),
        &header_bytes,
    )?;
    if store.get(&header_locator, 16384)? != header_bytes {
        return Err(Error::Integrity);
    }
    state.stored_bytes = header_bytes.len() as u64;
    let mut manifest = Manifest {
        version: 1,
        vault_id: header.vault_id,
        snapshot_id,
        files: vec![],
    };
    let mut dedup: BTreeMap<String, Chunk> = BTreeMap::new();
    let mut chunk_count = 0;
    for (source, logical_path) in entries {
        let mut file = File::open(&source)?;
        let before = file.metadata()?;
        if !before.is_file() {
            return Err(Error::UnsafePath);
        }
        let mut chunks = Vec::new();
        let mut hasher = blake3::Hasher::new_keyed(&identity_key);
        let mut bytes_read = 0u64;
        loop {
            let mut buffer = Zeroizing::new(vec![0u8; CHUNK_BYTES]);
            let mut filled = 0;
            while filled < CHUNK_BYTES {
                let n = file.read(&mut buffer[filled..])?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            if filled == 0 {
                break;
            }
            buffer.truncate(filled);
            hasher.update(&buffer);
            bytes_read = bytes_read.checked_add(filled as u64).ok_or(Error::Format)?;
            if bytes_read > before.len() {
                return Err(Error::SourceChanged);
            }
            let id = blake3::keyed_hash(&identity_key, &buffer)
                .to_hex()
                .to_string();
            let chunk = if let Some(chunk) = dedup.get(&id) {
                chunk.clone()
            } else {
                state.phase = "Encoding".into();
                progress(state.clone());
                let compressed = Zeroizing::new(zstd::bulk::compress(&buffer, 1)?);
                let (codec, encoded) = if compressed.len() < buffer.len() {
                    (Codec::Zstd, compressed.as_slice())
                } else {
                    (Codec::Raw, buffer.as_slice())
                };
                let aad = chunk_aad(header.vault_id, &id, filled, codec);
                let ciphertext = seal(&content_key, aad.as_bytes(), encoded)?;
                state.phase = "Uploading".into();
                progress(state.clone());
                let locator = store.put(
                    &format!("{}-{snapshot_id}-{id}", header.vault_id),
                    &ciphertext,
                )?;
                let chunk = Chunk {
                    id: id.clone(),
                    raw_bytes: filled,
                    codec,
                    locator,
                };
                state.phase = "Verifying".into();
                progress(state.clone());
                let verified = decode_chunk(store, &header, &content_key, &identity_key, &chunk)?;
                if verified.as_slice() != buffer.as_slice() {
                    return Err(Error::Integrity);
                }
                state.stored_bytes = state
                    .stored_bytes
                    .checked_add(ciphertext.len() as u64)
                    .ok_or(Error::Format)?;
                dedup.insert(id, chunk.clone());
                chunk
            };
            chunk_count += 1;
            if chunk_count > MAX_CHUNKS {
                return Err(Error::Format);
            }
            chunks.push(chunk);
            state.original_bytes = state
                .original_bytes
                .checked_add(filled as u64)
                .ok_or(Error::Format)?;
            progress(state.clone());
        }
        let after = file.metadata()?;
        if before.len() != bytes_read
            || before.len() != after.len()
            || before.modified()? != after.modified()?
        {
            return Err(Error::SourceChanged);
        }
        // A second bounded pass catches equal-size changes beyond timestamp checks.
        let archived_hash = hasher.finalize();
        let mut check = File::open(&source)?;
        let mut verify_hasher = blake3::Hasher::new_keyed(&identity_key);
        let mut buffer = Zeroizing::new(vec![0u8; CHUNK_BYTES]);
        let mut checked_bytes = 0u64;
        loop {
            let n = check.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            checked_bytes = checked_bytes
                .checked_add(n as u64)
                .ok_or(Error::SourceChanged)?;
            if checked_bytes > bytes_read {
                return Err(Error::SourceChanged);
            }
            verify_hasher.update(&buffer[..n]);
        }
        if verify_hasher.finalize() != archived_hash {
            return Err(Error::SourceChanged);
        }
        manifest.files.push(FileRecord {
            path: logical_path,
            bytes: bytes_read,
            chunks,
        });
        state.files_done += 1;
    }
    state.phase = "Finalizing".into();
    progress(state.clone());
    let mut bounded = BoundedMetadata(Zeroizing::new(Vec::new()));
    serde_json::to_writer(&mut bounded, &manifest).map_err(|_| Error::Format)?;
    let metadata = bounded.0;
    let aad = manifest_aad(header.vault_id, snapshot_id);
    let ciphertext = seal(&metadata_key, aad.as_bytes(), &metadata)?;
    let manifest_locator = store.put(
        &format!("{}-{snapshot_id}-manifest", header.vault_id),
        &ciphertext,
    )?;
    let readback = store.get(&manifest_locator, MAX_MANIFEST + 40)?;
    if open(&metadata_key, aad.as_bytes(), &readback)?.as_slice() != metadata.as_slice() {
        return Err(Error::Integrity);
    }
    state.stored_bytes = state
        .stored_bytes
        .checked_add(ciphertext.len() as u64)
        .ok_or(Error::Format)?;
    let kit = RecoveryKit {
        version: 1,
        snapshot_id,
        header,
        header_locator,
        manifest: manifest_locator,
        origin: store.origin(),
        files: manifest.files.len(),
        original_bytes: state.original_bytes,
        stored_bytes: state.stored_bytes,
    };
    let snapshot_dir = vault_dir.join("snapshots");
    fs::create_dir_all(&snapshot_dir)?;
    let recovery_kit = snapshot_dir.join(format!("{snapshot_id}.json"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&recovery_kit)?;
    file.write_all(&serde_json::to_vec_pretty(&kit).map_err(|_| Error::Format)?)?;
    file.sync_all()?;
    state.phase = "Completed".into();
    progress(state);
    Ok(ArchiveResult { kit, recovery_kit })
}

pub fn restore_from(
    kit: &RecoveryKit,
    secret: &str,
    use_recovery: bool,
    destination: &Path,
    store: &dyn ObjectStore,
    progress: impl Fn(Progress),
) -> Result<RestoreResult> {
    if kit.version != 1 || store.origin() != kit.origin {
        return Err(Error::Format);
    }
    let master = kit.header.unlock(secret, use_recovery)?;
    let metadata_key = derive(&master, "cloudweave v1 metadata encryption");
    let content_key = derive(&master, "cloudweave v1 content encryption");
    let identity_key = derive(&master, "cloudweave v1 content identity");
    let ciphertext = store.get(&kit.manifest, MAX_MANIFEST + 40)?;
    let aad = manifest_aad(kit.header.vault_id, kit.snapshot_id);
    let plaintext = open(&metadata_key, aad.as_bytes(), &ciphertext)?;
    let manifest: Manifest = serde_json::from_slice(&plaintext).map_err(|_| Error::Format)?;
    if manifest.version != 1
        || manifest.vault_id != kit.header.vault_id
        || manifest.snapshot_id != kit.snapshot_id
        || manifest.files.is_empty()
        || manifest.files.len() > MAX_FILES
    {
        return Err(Error::Format);
    }
    validate_paths(manifest.files.iter().map(|f| f.path.as_str()))?;
    let mut chunks = 0usize;
    for file in &manifest.files {
        let mut expected = 0u64;
        for chunk in &file.chunks {
            validate_chunk(chunk)?;
            expected = expected
                .checked_add(chunk.raw_bytes as u64)
                .ok_or(Error::Format)?;
            chunks += 1;
            if chunks > MAX_CHUNKS {
                return Err(Error::Format);
            }
        }
        if expected != file.bytes {
            return Err(Error::Format);
        }
    }
    // Only after authenticating and validating every path do we create output.
    let destination = destination.canonicalize()?;
    let restore_id = Uuid::new_v4();
    let staging = destination.join(format!(".cloudweave-partial-{restore_id}"));
    fs::create_dir(&staging)?;
    let mut state = Progress {
        phase: "Restoring".into(),
        ..Default::default()
    };
    progress(state.clone());
    for record in &manifest.files {
        let path = staging.join(&record.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        for chunk in &record.chunks {
            let data = decode_chunk(store, &kit.header, &content_key, &identity_key, chunk)?;
            file.write_all(&data)?;
            state.original_bytes = state
                .original_bytes
                .checked_add(data.len() as u64)
                .ok_or(Error::Format)?;
            progress(state.clone());
        }
        file.sync_all()?;
        state.files_done += 1;
    }
    let directory = destination.join(format!("CloudWeave-{restore_id}"));
    fs::rename(&staging, &directory)?;
    state.phase = "Completed".into();
    progress(state.clone());
    Ok(RestoreResult {
        files: state.files_done,
        bytes: state.original_bytes,
        directory,
    })
}

struct CollectionBudget {
    remaining_nodes: usize,
    logical_bytes: usize,
}
struct BoundedMetadata(Zeroizing<Vec<u8>>);
impl Write for BoundedMetadata {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_MANIFEST {
            return Err(std::io::Error::other("metadata size limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn collect(
    path: &Path,
    logical: String,
    depth: usize,
    entries: &mut Vec<(PathBuf, String)>,
    budget: &mut CollectionBudget,
) -> Result<()> {
    if depth > 64 || entries.len() >= MAX_FILES || logical.len() > 4096 {
        return Err(Error::Format);
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(Error::UnsafePath);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(Error::UnsafePath);
        }
    }
    if metadata.is_file() {
        budget.logical_bytes = budget
            .logical_bytes
            .checked_add(logical.len())
            .ok_or(Error::Format)?;
        if budget.logical_bytes > MAX_MANIFEST / 2 {
            return Err(Error::Format);
        }
        entries.push((path.to_owned(), logical));
    } else if metadata.is_dir() {
        let mut children = fs::read_dir(path)?
            .take(budget.remaining_nodes + 1)
            .collect::<std::io::Result<Vec<_>>>()?;
        if children.len() > budget.remaining_nodes {
            return Err(Error::Format);
        }
        budget.remaining_nodes -= children.len();
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            let name = child
                .file_name()
                .into_string()
                .map_err(|_| Error::UnsafePath)?;
            collect(
                &child.path(),
                format!("{logical}/{name}"),
                depth + 1,
                entries,
                budget,
            )?;
        }
    } else {
        return Err(Error::UnsafePath);
    }
    Ok(())
}
fn chunk_aad(vault: Uuid, id: &str, bytes: usize, codec: Codec) -> String {
    format!(
        "cloudweave:1:{vault}:chunk:{id}:{bytes}:{}",
        match codec {
            Codec::Raw => "raw",
            Codec::Zstd => "zstd",
        }
    )
}
fn manifest_aad(vault: Uuid, snapshot: Uuid) -> String {
    format!("cloudweave:1:{vault}:manifest:{snapshot}")
}
fn validate_chunk(chunk: &Chunk) -> Result<()> {
    if chunk.raw_bytes == 0
        || chunk.raw_bytes > CHUNK_BYTES
        || chunk.id.len() != 64
        || !chunk.id.bytes().all(|b| b.is_ascii_hexdigit())
        || chunk.locator.id.len() > 4096
    {
        return Err(Error::Format);
    }
    Ok(())
}
fn decode_chunk(
    store: &dyn ObjectStore,
    header: &VaultHeader,
    content_key: &[u8; 32],
    identity_key: &[u8; 32],
    chunk: &Chunk,
) -> Result<Zeroizing<Vec<u8>>> {
    validate_chunk(chunk)?;
    let aad = chunk_aad(header.vault_id, &chunk.id, chunk.raw_bytes, chunk.codec);
    let ciphertext = store.get(&chunk.locator, CHUNK_BYTES + 40)?;
    let encoded = open(content_key, aad.as_bytes(), &ciphertext)?;
    let raw = match chunk.codec {
        Codec::Raw => encoded,
        Codec::Zstd => {
            let mut decoder = zstd::stream::read::Decoder::new(encoded.as_slice())
                .map_err(|_| Error::Integrity)?;
            // Bound zstd's internal window as well as output. An authenticated writer
            // can still supply hostile compressed data.
            decoder.window_log_max(23).map_err(|_| Error::Integrity)?;
            let mut bytes = Zeroizing::new(Vec::new());
            decoder
                .take(chunk.raw_bytes as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| Error::Integrity)?;
            bytes
        }
    };
    if raw.len() != chunk.raw_bytes
        || blake3::keyed_hash(identity_key, &raw).to_hex().as_str() != chunk.id
    {
        return Err(Error::Integrity);
    }
    Ok(raw)
}

fn validate_paths<'a>(paths: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    let mut prefixes = BTreeMap::new();
    for path in paths {
        if path.len() > 4096 {
            return Err(Error::UnsafePath);
        }
        for part in path.split('/') {
            let upper = part.to_uppercase();
            let base = upper.split('.').next().unwrap_or("").trim_end_matches(' ');
            if part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with([' ', '.'])
                || part
                    .chars()
                    .any(|c| c.is_control() || "\\:*?\"<>|".contains(c))
                || ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&base)
                || (base.starts_with("COM") || base.starts_with("LPT"))
                    && (base.chars().count() == 4
                        && matches!(base.chars().last(), Some('1'..='9' | '¹' | '²' | '³')))
            {
                return Err(Error::UnsafePath);
            }
        }
        let key: String = path.nfc().flat_map(char::to_lowercase).collect();
        if !seen.insert(key) {
            return Err(Error::UnsafePath);
        }
        let mut prefix = String::new();
        for part in path.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            let key: String = prefix.nfc().flat_map(char::to_lowercase).collect();
            if let Some(previous) = prefixes.insert(key, prefix.clone())
                && previous != prefix
            {
                return Err(Error::UnsafePath);
            }
        }
    }
    // Reject file/directory collisions before any output is created.
    for path in &seen {
        let mut parent = path.as_str();
        while let Some((head, _)) = parent.rsplit_once('/') {
            if seen.contains(head) {
                return Err(Error::UnsafePath);
            }
            parent = head;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_hostile_and_colliding_paths_before_writing() {
        for path in [
            "../escape",
            "/absolute",
            "C:/drive",
            "a\\escape",
            "x/../z",
            "NUL.txt",
            "file.",
            "x/COM1",
            "COM¹.txt",
            "",
        ] {
            assert!(validate_paths([path].into_iter()).is_err(), "{path}");
        }
        assert!(validate_paths(["Folder/a", "folder/A"].into_iter()).is_err());
        assert!(validate_paths(["a", "a/file"].into_iter()).is_err());
        assert!(validate_paths(["Folder/a", "folder/b"].into_iter()).is_err());
        assert!(validate_paths(["عکس/یادداشت.txt", "notes/a.txt"].into_iter()).is_ok());
    }
}

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ObjectLocator {
    pub id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Origin {
    Local { root: PathBuf },
    Cloud { provider: String, subject: String },
}

/// Objects are immutable. Put returns the provider-native ID, never an inferred path.
/// Implementations bound reads *while streaming*, including missing Content-Length.
pub trait ObjectStore: Send + Sync {
    fn origin(&self) -> Origin;
    fn put(&self, name: &str, data: &[u8]) -> Result<ObjectLocator>;
    fn get(&self, locator: &ObjectLocator, max_bytes: usize) -> Result<Vec<u8>>;
}

pub struct LocalStore {
    root: PathBuf,
}
impl LocalStore {
    /// Local/removable storage is a development and interoperability destination.
    pub fn new(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        Ok(Self {
            root: root.canonicalize()?,
        })
    }
    /// Restore opens an existing trusted local root without creating kit-supplied paths.
    pub fn open(root: PathBuf) -> Result<Self> {
        if !root.is_absolute() {
            return Err(Error::UnsafePath);
        }
        #[cfg(windows)]
        {
            use std::path::{Component, Prefix};
            match root.components().next() {
                Some(Component::Prefix(prefix))
                    if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) => {}
                _ => return Err(Error::UnsafePath),
            }
            use std::os::windows::fs::MetadataExt;
            if fs::symlink_metadata(&root)?.file_attributes() & 0x400 != 0 {
                return Err(Error::UnsafePath);
            }
        }
        if !fs::symlink_metadata(&root)?.is_dir() {
            return Err(Error::UnsafePath);
        }
        Ok(Self {
            root: root.canonicalize()?,
        })
    }
    fn object_path(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty()
            || id.len() > 240
            || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(Error::Format);
        }
        Ok(self.root.join(id))
    }
}
impl ObjectStore for LocalStore {
    fn origin(&self) -> Origin {
        Origin::Local {
            root: self.root.clone(),
        }
    }
    fn put(&self, name: &str, data: &[u8]) -> Result<ObjectLocator> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.object_path(name)?)?;
        file.write_all(data)?;
        file.sync_all()?;
        Ok(ObjectLocator { id: name.into() })
    }
    fn get(&self, locator: &ObjectLocator, max_bytes: usize) -> Result<Vec<u8>> {
        let path = self.object_path(&locator.id)?;
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(Error::UnsafePath);
        }
        read_bounded(&path, max_bytes)
    }
}
pub(crate) fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    read_stream_bounded(File::open(path)?, limit)
}
pub fn read_stream_bounded(reader: impl Read, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(Error::Format);
    }
    Ok(bytes)
}

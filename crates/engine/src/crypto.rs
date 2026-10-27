use crate::{Error, Result, store::read_bounded};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VaultHeader {
    pub version: u32,
    pub vault_id: Uuid,
    pub salt: [u8; 16],
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
    pub passphrase_envelope: Vec<u8>,
    pub recovery_envelope: Vec<u8>,
}

impl VaultHeader {
    fn validate(&self) -> Result<()> {
        if self.version != 1
            || !(8192..=65536).contains(&self.memory_kib)
            || !(1..=6).contains(&self.iterations)
            || !(1..=4).contains(&self.parallelism)
            || self.passphrase_envelope.len() != 72
            || self.recovery_envelope.len() != 72
        {
            return Err(Error::Format);
        }
        Ok(())
    }
    fn aad(&self, role: &str) -> String {
        format!(
            "cloudweave:1:{}:{role}:{}:{}:{}:{}",
            self.vault_id,
            hex::encode(self.salt),
            self.memory_kib,
            self.iterations,
            self.parallelism
        )
    }
    fn passphrase_key(&self, passphrase: &str) -> Result<Zeroizing<[u8; 32]>> {
        self.validate()?;
        let params = Params::new(self.memory_kib, self.iterations, self.parallelism, Some(32))
            .map_err(|_| Error::Format)?;
        let mut key = Zeroizing::new([0u8; 32]);
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(passphrase.as_bytes(), &self.salt, &mut *key)
            .map_err(|_| Error::Format)?;
        Ok(key)
    }
    pub(crate) fn unlock(&self, secret: &str, recovery: bool) -> Result<Zeroizing<[u8; 32]>> {
        self.validate()?;
        let (key, envelope, role) = if recovery {
            let mut bytes = Zeroizing::new([0u8; 32]);
            hex::decode_to_slice(secret.trim(), &mut *bytes).map_err(|_| Error::Integrity)?;
            (
                Zeroizing::new(blake3::derive_key(
                    "cloudweave v1 recovery wrapping key",
                    &*bytes,
                )),
                &self.recovery_envelope,
                "recovery",
            )
        } else {
            (
                self.passphrase_key(secret)?,
                &self.passphrase_envelope,
                "passphrase",
            )
        };
        let bytes = open(&key, self.aad(role).as_bytes(), envelope)?;
        let master: [u8; 32] = bytes.as_slice().try_into().map_err(|_| Error::Integrity)?;
        Ok(Zeroizing::new(master))
    }
}

pub fn read_header(dir: &Path) -> Result<VaultHeader> {
    let bytes = read_bounded(&dir.join("vault.json"), 16384)?;
    let header: VaultHeader = serde_json::from_slice(&bytes).map_err(|_| Error::Format)?;
    header.validate()?;
    Ok(header)
}

/// Creates only new vault headers. Recovery secret must be exported independently.
pub fn create_vault(dir: &Path, passphrase: &str) -> Result<String> {
    if passphrase.chars().count() < 12 {
        return Err(Error::WeakPassphrase);
    }
    fs::create_dir_all(dir)?;
    let master = random_key()?;
    let recovery = random_key()?;
    let mut header = VaultHeader {
        version: 1,
        vault_id: Uuid::new_v4(),
        salt: [0; 16],
        memory_kib: 65536,
        iterations: 3,
        parallelism: 1,
        passphrase_envelope: vec![0; 72],
        recovery_envelope: vec![0; 72],
    };
    OsRng
        .try_fill_bytes(&mut header.salt)
        .map_err(|_| Error::Integrity)?;
    header.passphrase_envelope = seal(
        &*header.passphrase_key(passphrase)?,
        header.aad("passphrase").as_bytes(),
        &*master,
    )?;
    let recovery_key = Zeroizing::new(blake3::derive_key(
        "cloudweave v1 recovery wrapping key",
        &*recovery,
    ));
    header.recovery_envelope = seal(&recovery_key, header.aad("recovery").as_bytes(), &*master)?;
    let encoded = serde_json::to_vec_pretty(&header).map_err(|_| Error::Format)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join("vault.json"))?;
    file.write_all(&encoded)?;
    file.sync_all()?;
    Ok(hex::encode(recovery.as_slice()))
}

fn random_key() -> Result<Zeroizing<[u8; 32]>> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    OsRng
        .try_fill_bytes(&mut *bytes)
        .map_err(|_| Error::Integrity)?;
    Ok(bytes)
}

pub(crate) fn derive(master: &[u8; 32], context: &'static str) -> Zeroizing<[u8; 32]> {
    Zeroizing::new(blake3::derive_key(context, master))
}
pub(crate) fn seal(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
    let mut nonce = [0u8; 24];
    OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| Error::Integrity)?;
    let ciphertext = XChaCha20Poly1305::new(key.into())
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| Error::Integrity)?;
    let mut output = nonce.to_vec();
    output.extend(ciphertext);
    Ok(output)
}
pub(crate) fn open(key: &[u8; 32], aad: &[u8], ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if ciphertext.len() < 40 {
        return Err(Error::Integrity);
    }
    XChaCha20Poly1305::new(key.into())
        .decrypt(
            XNonce::from_slice(&ciphertext[..24]),
            Payload {
                msg: &ciphertext[24..],
                aad,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| Error::Integrity)
}

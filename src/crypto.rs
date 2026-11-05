use std::env;
use std::process::Command;

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
};
use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};

pub const KEY_LEN: usize = 32;
pub type Key = [u8; KEY_LEN];

const VERSION_PREFIX: &str = "enc:v1:";
const AAD: &[u8] = b"quantumdata:sqlite:v1";
const KEYCHAIN_SERVICE: &str = "com.quantumdata.credentials";
const KEYCHAIN_ACCOUNT: &str = "master-key";
const MASTER_KEY_ENV: &str = "QUANTUMDATA_MASTER_KEY";

pub struct Cipher {
    cipher: Aes256Gcm,
}

impl Cipher {
    pub fn new(key: &Key) -> Self {
        Self {
            cipher: Aes256Gcm::new_from_slice(key).expect("AES-256 keys are always 32 bytes"),
        }
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: AAD,
                },
            )
            .map_err(|_| anyhow!("Failed to encrypt the value"))?;

        let mut blob = nonce.to_vec();
        blob.extend(ciphertext);
        Ok(format!("{VERSION_PREFIX}{}", STANDARD.encode(blob)))
    }

    pub fn decrypt(&self, stored: &str) -> Result<String> {
        let encoded = stored
            .strip_prefix(VERSION_PREFIX)
            .ok_or_else(|| anyhow!("Value is not encrypted"))?;
        let blob = STANDARD
            .decode(encoded)
            .context("Failed to decode an encrypted value")?;

        if blob.len() <= 12 {
            bail!("Encrypted value is truncated");
        }

        let (nonce, ciphertext) = blob.split_at(12);
        let plaintext = self
            .cipher
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: AAD,
                },
            )
            .map_err(|_| anyhow!("Failed to decrypt the value; the master key may have changed"))?;

        String::from_utf8(plaintext).context("Decrypted value is not valid UTF-8")
    }
}

pub fn is_encrypted(value: &str) -> bool {
    value.starts_with(VERSION_PREFIX)
}

pub fn load_master_key() -> Result<Key> {
    if let Some(key) = key_from_env()? {
        return Ok(key);
    }

    #[cfg(target_os = "macos")]
    {
        load_keychain_key()
    }

    #[cfg(not(target_os = "macos"))]
    {
        bail!("No encryption key available. Set {MASTER_KEY_ENV} to a base64-encoded 32-byte key.")
    }
}

fn key_from_env() -> Result<Option<Key>> {
    match env::var(MASTER_KEY_ENV) {
        Ok(value) if !value.trim().is_empty() => decode_key(value.trim()).map(Some),
        Ok(_) => Ok(None),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error).context("Failed to read QUANTUMDATA_MASTER_KEY"),
    }
}

fn decode_key(encoded: &str) -> Result<Key> {
    let bytes = STANDARD
        .decode(encoded.trim())
        .with_context(|| format!("{MASTER_KEY_ENV} must be base64-encoded"))?;
    let length = bytes.len();

    bytes.try_into().map_err(|_| {
        anyhow!("{MASTER_KEY_ENV} must decode to exactly {KEY_LEN} bytes (got {length})")
    })
}

#[cfg(target_os = "macos")]
fn load_keychain_key() -> Result<Key> {
    if let Some(key) = read_keychain_key()? {
        return Ok(key);
    }

    let key = Aes256Gcm::generate_key(&mut OsRng);
    let encoded = STANDARD.encode(key);
    let status = Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            KEYCHAIN_ACCOUNT,
            "-w",
            &encoded,
        ])
        .status()
        .context("Failed to run the macOS security tool")?;

    if !status.success() {
        bail!(
            "Failed to store the encryption key in the macOS Keychain. \
             Grant the terminal access or set {MASTER_KEY_ENV}."
        );
    }

    let key: Key = key
        .as_slice()
        .try_into()
        .expect("generated key is 32 bytes");
    Ok(key)
}

#[cfg(target_os = "macos")]
fn read_keychain_key() -> Result<Option<Key>> {
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            KEYCHAIN_ACCOUNT,
            "-w",
        ])
        .output()
        .context("Failed to run the macOS security tool")?;

    if !output.status.success() {
        return Ok(None);
    }

    let encoded =
        String::from_utf8(output.stdout).context("Keychain returned a non UTF-8 encryption key")?;
    decode_key(encoded.trim()).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_round_trip() {
        let cipher = Cipher::new(&[7u8; KEY_LEN]);
        let encrypted = cipher
            .encrypt("sk-test-123")
            .expect("failed to encrypt value");

        assert!(encrypted.starts_with(VERSION_PREFIX));
        assert!(!encrypted.contains("sk-test-123"));
        assert_eq!(
            cipher.decrypt(&encrypted).expect("failed to decrypt value"),
            "sk-test-123"
        );
    }

    #[test]
    fn ciphertexts_are_not_stable() {
        let cipher = Cipher::new(&[7u8; KEY_LEN]);
        let first = cipher.encrypt("same").expect("failed to encrypt");
        let second = cipher.encrypt("same").expect("failed to encrypt");

        assert_ne!(first, second);
    }

    #[test]
    fn wrong_key_is_rejected() {
        let encrypted = Cipher::new(&[1u8; KEY_LEN])
            .encrypt("secret")
            .expect("failed to encrypt");

        let error = Cipher::new(&[2u8; KEY_LEN])
            .decrypt(&encrypted)
            .expect_err("decryption should fail");

        assert!(error.to_string().contains("Failed to decrypt"));
    }

    #[test]
    fn tampering_is_rejected() {
        let cipher = Cipher::new(&[7u8; KEY_LEN]);
        let encrypted = cipher.encrypt("secret").expect("failed to encrypt");
        let mut blob = STANDARD
            .decode(encrypted.trim_start_matches(VERSION_PREFIX))
            .expect("failed to decode");
        let last = blob.len() - 1;
        blob[last] ^= 0b0000_0001;
        let tampered = format!("{VERSION_PREFIX}{}", STANDARD.encode(blob));

        let error = cipher
            .decrypt(&tampered)
            .expect_err("tampered value should fail");

        assert!(error.to_string().contains("Failed to decrypt"));
    }

    #[test]
    fn key_decoding_requires_32_bytes() {
        assert!(decode_key(&STANDARD.encode([9u8; KEY_LEN])).is_ok());
        assert!(decode_key(&STANDARD.encode([9u8; KEY_LEN - 1])).is_err());
        assert!(decode_key("not base64!").is_err());
    }
}

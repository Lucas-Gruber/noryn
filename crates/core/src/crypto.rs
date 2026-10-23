use anyhow::{anyhow, ensure, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD, Engine};
use chacha20poly1305::{aead::{Aead, Payload}, KeyInit, XChaCha20Poly1305, XNonce};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub version: u8,
    pub nonce: String,
    pub ciphertext: String,
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

pub fn derive_key(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    let mut key = Zeroizing::new([0; 32]);
    let params = Params::new(64 * 1024, 3, 1, Some(32)).map_err(|e| anyhow!(e.to_string()))?;
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, key.as_mut()).map_err(|e| anyhow!(e.to_string()))?;
    Ok(key)
}

pub fn decode_key(key: &str) -> Result<Zeroizing<[u8; 32]>> {
    let bytes = Zeroizing::new(STANDARD.decode(key.trim())?);
    ensure!(bytes.len() == 32, "Team-Schlüssel muss 32 zufällige Bytes (Base64) enthalten");
    let mut key = Zeroizing::new([0; 32]);
    key.copy_from_slice(&bytes);
    Ok(key)
}

pub fn seal(key: &[u8; 32], plaintext: &[u8], context: &[u8]) -> Result<Envelope> {
    let nonce = random_bytes::<24>();
    let cipher = XChaCha20Poly1305::new(key.into());
    let ciphertext = cipher.encrypt(XNonce::from_slice(&nonce), Payload { msg: plaintext, aad: context })
        .map_err(|_| anyhow!("Verschlüsselung fehlgeschlagen"))?;
    Ok(Envelope { version: 1, nonce: STANDARD.encode(nonce), ciphertext: STANDARD.encode(ciphertext) })
}

pub fn unseal(key: &[u8; 32], envelope: &Envelope, context: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    ensure!(envelope.version == 1, "Unbekanntes Verschlüsselungsformat");
    let nonce = STANDARD.decode(&envelope.nonce)?;
    ensure!(nonce.len() == 24, "Ungültige Nonce");
    let ciphertext = STANDARD.decode(&envelope.ciphertext)?;
    XChaCha20Poly1305::new(key.into()).decrypt(XNonce::from_slice(&nonce), Payload { msg: &ciphertext, aad: context })
        .map(Zeroizing::new).map_err(|_| anyhow!("Entschlüsselung fehlgeschlagen: falsches Passwort/Schlüssel oder manipulierte Daten"))
}

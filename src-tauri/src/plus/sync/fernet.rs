use aes::Aes128;
use base64::{engine::general_purpose::URL_SAFE, Engine};
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;
type Enc = cbc::Encryptor<Aes128>;
type Dec = cbc::Decryptor<Aes128>;

const VERSION: u8 = 0x80;
const HEADER_LEN: usize = 1 + 8 + 16;
const MAC_LEN: usize = 32;

#[derive(Debug, PartialEq, Eq)]
pub enum FernetError {
    InvalidKey,
    InvalidToken,
    Entropy,
}

impl fmt::Display for FernetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FernetError::InvalidKey => f.write_str("fernet key must be 32 url-safe base64 bytes"),
            FernetError::InvalidToken => f.write_str("invalid fernet token or wrong key"),
            FernetError::Entropy => f.write_str("system entropy source unavailable"),
        }
    }
}

impl std::error::Error for FernetError {}

pub struct FernetKey {
    signing: [u8; 16],
    encryption: [u8; 16],
}

impl FernetKey {
    pub fn parse(encoded: &str) -> Result<Self, FernetError> {
        let raw = URL_SAFE
            .decode(encoded.trim())
            .map_err(|_| FernetError::InvalidKey)?;
        if raw.len() != 32 {
            return Err(FernetError::InvalidKey);
        }
        let mut signing = [0u8; 16];
        let mut encryption = [0u8; 16];
        signing.copy_from_slice(&raw[..16]);
        encryption.copy_from_slice(&raw[16..]);
        Ok(Self {
            signing,
            encryption,
        })
    }

    fn mac(&self) -> HmacSha256 {
        HmacSha256::new_from_slice(&self.signing).expect("hmac accepts any key length")
    }
}

pub fn encrypt(key: &FernetKey, plaintext: &[u8]) -> Result<Vec<u8>, FernetError> {
    let mut iv = [0u8; 16];
    getrandom::getrandom(&mut iv).map_err(|_| FernetError::Entropy)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(encrypt_at(key, plaintext, now, iv))
}

pub fn encrypt_at(key: &FernetKey, plaintext: &[u8], timestamp: u64, iv: [u8; 16]) -> Vec<u8> {
    let ciphertext =
        Enc::new(&key.encryption.into(), &iv.into()).encrypt_padded_vec_mut::<Pkcs7>(plaintext);
    let mut token = Vec::with_capacity(HEADER_LEN + ciphertext.len() + MAC_LEN);
    token.push(VERSION);
    token.extend_from_slice(&timestamp.to_be_bytes());
    token.extend_from_slice(&iv);
    token.extend_from_slice(&ciphertext);
    let mut mac = key.mac();
    mac.update(&token);
    token.extend_from_slice(&mac.finalize().into_bytes());
    URL_SAFE.encode(token).into_bytes()
}

pub fn decrypt(key: &FernetKey, token: &[u8]) -> Result<Vec<u8>, FernetError> {
    let text = std::str::from_utf8(token).map_err(|_| FernetError::InvalidToken)?;
    let raw = URL_SAFE
        .decode(text.trim())
        .map_err(|_| FernetError::InvalidToken)?;
    if raw.len() < HEADER_LEN + 16 + MAC_LEN || raw[0] != VERSION {
        return Err(FernetError::InvalidToken);
    }
    let (signed, tag) = raw.split_at(raw.len() - MAC_LEN);
    let mut mac = key.mac();
    mac.update(signed);
    mac.verify_slice(tag)
        .map_err(|_| FernetError::InvalidToken)?;
    let iv: [u8; 16] = signed[9..HEADER_LEN].try_into().expect("slice is 16 bytes");
    let body = &signed[HEADER_LEN..];
    if body.len() % 16 != 0 {
        return Err(FernetError::InvalidToken);
    }
    Dec::new(&key.encryption.into(), &iv.into())
        .decrypt_padded_vec_mut::<Pkcs7>(body)
        .map_err(|_| FernetError::InvalidToken)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC_KEY: &str = "cw_0x689RpI-jtRR7oE8h_eQsKImvJapLeSbXpwF4e4=";
    const SPEC_TOKEN: &str = "gAAAAAAdwJ6wAAECAwQFBgcICQoLDA0ODy021cpGVWKZ_eEwCGM4BLLF_5CV9dOPmrhuVUPgJobwOz7JcbmrR64jVmpU4IwqDA==";

    #[test]
    fn decrypts_the_fernet_spec_vector() {
        let key = FernetKey::parse(SPEC_KEY).unwrap();
        assert_eq!(decrypt(&key, SPEC_TOKEN.as_bytes()).unwrap(), b"hello");
    }

    #[test]
    fn encrypt_at_reproduces_the_fernet_spec_vector() {
        let key = FernetKey::parse(SPEC_KEY).unwrap();
        let iv: [u8; 16] = std::array::from_fn(|i| i as u8);
        let token = encrypt_at(&key, b"hello", 499_162_800, iv);
        assert_eq!(token, SPEC_TOKEN.as_bytes());
    }

    #[test]
    fn round_trips_and_rejects_tampering_and_wrong_keys() {
        let key = FernetKey::parse(SPEC_KEY).unwrap();
        let token = encrypt(&key, b"synthetic payload").unwrap();
        assert_eq!(decrypt(&key, &token).unwrap(), b"synthetic payload");

        let mut flipped = token.clone();
        let last = flipped.len() - 6;
        flipped[last] = if flipped[last] == b'A' { b'B' } else { b'A' };
        assert_eq!(decrypt(&key, &flipped), Err(FernetError::InvalidToken));

        let other =
            FernetKey::parse(&crate::plus::sync::kdf::derive_key_with("x", b"salt", 1)).unwrap();
        assert_eq!(decrypt(&other, &token), Err(FernetError::InvalidToken));
        assert_eq!(
            FernetKey::parse("short").err(),
            Some(FernetError::InvalidKey)
        );
    }
}

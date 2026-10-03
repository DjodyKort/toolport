use base64::{engine::general_purpose::URL_SAFE, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;

pub const PBKDF2_ITERATIONS: u32 = 600_000;
pub const SALT_LEN: usize = 16;

type HmacSha256 = Hmac<Sha256>;

pub fn pbkdf2_sha256_32(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let base = HmacSha256::new_from_slice(password).expect("hmac accepts any key length");
    let mut mac = base.clone();
    mac.update(salt);
    mac.update(&1u32.to_be_bytes());
    let mut u: [u8; 32] = mac.finalize().into_bytes().into();
    let mut out = u;
    for _ in 1..iterations {
        let mut mac = base.clone();
        mac.update(&u);
        u = mac.finalize().into_bytes().into();
        for (o, b) in out.iter_mut().zip(u.iter()) {
            *o ^= b;
        }
    }
    out
}

pub fn derive_key(passphrase: &str, salt: &[u8]) -> String {
    derive_key_with(passphrase, salt, PBKDF2_ITERATIONS)
}

pub fn derive_key_with(passphrase: &str, salt: &[u8], iterations: u32) -> String {
    URL_SAFE.encode(pbkdf2_sha256_32(passphrase.as_bytes(), salt, iterations))
}

pub fn random_salt() -> Result<[u8; SALT_LEN], getrandom::Error> {
    let mut salt = [0u8; SALT_LEN];
    getrandom::getrandom(&mut salt)?;
    Ok(salt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plus::hashing::hex;

    #[test]
    fn pbkdf2_matches_rfc7914_vectors() {
        assert_eq!(
            hex(&pbkdf2_sha256_32(b"passwd", b"salt", 1)),
            "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc"
        );
        assert_eq!(
            hex(&pbkdf2_sha256_32(b"password", b"salt", 4096)),
            "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a"
        );
    }
}

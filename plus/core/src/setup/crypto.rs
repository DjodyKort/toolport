//! Passphrase encryption of the secret parts of a setup file: Argon2id derives the key,
//! XChaCha20-Poly1305 seals the data in chunks. The header (magic, version, cost, salt, nonce
//! prefix, chunk size) is authenticated by every chunk, each chunk's nonce carries its index and
//! whether it is the last one, so a cut, reordered or extended file does not open.
//!
//! The ciphers are behind the `cipher` feature; without it every call says so.

use std::io::{Read, Write};

pub const MIN_PASSPHRASE_CHARS: usize = 12;
pub const WRONG_PASSPHRASE: &str = "wrong passphrase or damaged file";

const MAGIC: &[u8; 4] = b"TPSE";
const VERSION: u8 = 1;
const KDF_ARGON2ID: u8 = 1;
const HEADER_LEN: usize = 4 + 1 + 1 + 4 + 4 + 4 + 16 + 19 + 4;
const CHUNK: usize = 64 * 1024;
const TAG: usize = 16;
const MAX_MEMORY_KIB: u32 = 1 << 20;
const MAX_ITERATIONS: u32 = 16;

/// Argon2id cost. The defaults take a few tenths of a second on a laptop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cost {
    pub memory_kib: u32,
    pub iterations: u32,
    pub lanes: u32,
}

impl Default for Cost {
    fn default() -> Self {
        Self {
            memory_kib: 64 * 1024,
            iterations: 3,
            lanes: 1,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    /// The passphrase does not open the file, or the file was changed.
    Wrong,
    /// Not a file this build can read (format, version or cost out of range).
    Unreadable(String),
    Io(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Wrong => f.write_str(WRONG_PASSPHRASE),
            OpenError::Unreadable(why) | OpenError::Io(why) => f.write_str(why),
        }
    }
}

pub fn available() -> bool {
    cfg!(feature = "cipher")
}

pub fn check_passphrase(passphrase: &str) -> Result<(), String> {
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(format!(
            "the passphrase needs at least {MIN_PASSPHRASE_CHARS} characters"
        ));
    }
    Ok(())
}

pub fn seal(passphrase: &str, plain: &mut dyn Read, out: &mut dyn Write) -> Result<(), String> {
    seal_with(passphrase, Cost::default(), plain, out)
}

pub fn seal_with(
    passphrase: &str,
    cost: Cost,
    plain: &mut dyn Read,
    out: &mut dyn Write,
) -> Result<(), String> {
    check_passphrase(passphrase)?;
    imp::seal(passphrase, cost, plain, out)
}

pub fn open(passphrase: &str, sealed: &mut dyn Read, out: &mut dyn Write) -> Result<(), OpenError> {
    imp::open(passphrase, sealed, out)
}

/// Seals a small value in memory.
pub fn seal_bytes(passphrase: &str, plain: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    seal(passphrase, &mut &plain[..], &mut out)?;
    Ok(out)
}

pub fn open_bytes(passphrase: &str, sealed: &[u8]) -> Result<Vec<u8>, OpenError> {
    let mut out = Vec::new();
    open(passphrase, &mut &sealed[..], &mut out)?;
    Ok(out)
}

#[cfg(not(feature = "cipher"))]
mod imp {
    use super::*;

    const NO_CIPHER: &str = "this build has no encryption";

    pub fn seal(_: &str, _: Cost, _: &mut dyn Read, _: &mut dyn Write) -> Result<(), String> {
        Err(NO_CIPHER.into())
    }

    pub fn open(_: &str, _: &mut dyn Read, _: &mut dyn Write) -> Result<(), OpenError> {
        Err(OpenError::Unreadable(NO_CIPHER.into()))
    }
}

#[cfg(feature = "cipher")]
mod imp {
    use super::*;
    use argon2::{Algorithm, Argon2, Params, Version};
    use chacha20poly1305::aead::{Aead, KeyInit, OsRng, Payload};
    use chacha20poly1305::{AeadCore, XChaCha20Poly1305, XNonce};

    fn derive(passphrase: &str, salt: &[u8], cost: Cost) -> Result<[u8; 32], String> {
        let params = Params::new(cost.memory_kib, cost.iterations, cost.lanes, Some(32))
            .map_err(|e| format!("encryption cost: {e}"))?;
        let mut key = [0u8; 32];
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(passphrase.as_bytes(), salt, &mut key)
            .map_err(|e| format!("key derivation: {e}"))?;
        Ok(key)
    }

    fn nonce(prefix: &[u8], index: u32, last: bool) -> XNonce {
        let mut bytes = [0u8; 24];
        bytes[..19].copy_from_slice(prefix);
        bytes[19..23].copy_from_slice(&index.to_be_bytes());
        bytes[23] = u8::from(last);
        *XNonce::from_slice(&bytes)
    }

    /// Fills `buf` and returns how many bytes arrived before the end of the input.
    fn fill(input: &mut dyn Read, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut filled = 0;
        while filled < buf.len() {
            match input.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(filled)
    }

    pub fn seal(
        passphrase: &str,
        cost: Cost,
        plain: &mut dyn Read,
        out: &mut dyn Write,
    ) -> Result<(), String> {
        let mut salt = [0u8; 16];
        let mut prefix = [0u8; 19];
        let random = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        prefix.copy_from_slice(&random[..19]);
        let second = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        salt.copy_from_slice(&second[..16]);

        let mut header = Vec::with_capacity(HEADER_LEN);
        header.extend_from_slice(MAGIC);
        header.push(VERSION);
        header.push(KDF_ARGON2ID);
        header.extend_from_slice(&cost.memory_kib.to_be_bytes());
        header.extend_from_slice(&cost.iterations.to_be_bytes());
        header.extend_from_slice(&cost.lanes.to_be_bytes());
        header.extend_from_slice(&salt);
        header.extend_from_slice(&prefix);
        header.extend_from_slice(&(CHUNK as u32).to_be_bytes());
        out.write_all(&header).map_err(|e| e.to_string())?;

        let key = derive(passphrase, &salt, cost)?;
        let cipher = XChaCha20Poly1305::new((&key).into());
        let mut buf = vec![0u8; CHUNK];
        let mut ahead = vec![0u8; CHUNK];
        let mut have = fill(plain, &mut buf).map_err(|e| e.to_string())?;
        let mut index = 0u32;
        loop {
            let next = if have == CHUNK {
                fill(plain, &mut ahead).map_err(|e| e.to_string())?
            } else {
                0
            };
            let last = next == 0;
            let sealed = cipher
                .encrypt(
                    &nonce(&prefix, index, last),
                    Payload {
                        msg: &buf[..have],
                        aad: &header,
                    },
                )
                .map_err(|_| "encryption failed".to_string())?;
            out.write_all(&sealed).map_err(|e| e.to_string())?;
            if last {
                return Ok(());
            }
            std::mem::swap(&mut buf, &mut ahead);
            have = next;
            index = index
                .checked_add(1)
                .ok_or("the data is too large to seal")?;
        }
    }

    pub fn open(
        passphrase: &str,
        sealed: &mut dyn Read,
        out: &mut dyn Write,
    ) -> Result<(), OpenError> {
        let mut header = [0u8; HEADER_LEN];
        if fill(sealed, &mut header).map_err(|e| OpenError::Io(e.to_string()))? != HEADER_LEN
            || &header[..4] != MAGIC
        {
            return Err(OpenError::Unreadable(
                "not an encrypted Toolport part".into(),
            ));
        }
        if header[4] != VERSION || header[5] != KDF_ARGON2ID {
            return Err(OpenError::Unreadable(
                "this encrypted part was made by a newer Toolport".into(),
            ));
        }
        let word = |at: usize| u32::from_be_bytes(header[at..at + 4].try_into().unwrap());
        let cost = Cost {
            memory_kib: word(6),
            iterations: word(10),
            lanes: word(14),
        };
        let chunk = word(HEADER_LEN - 4) as usize;
        if cost.memory_kib > MAX_MEMORY_KIB
            || cost.iterations == 0
            || cost.iterations > MAX_ITERATIONS
            || cost.lanes == 0
            || cost.lanes > 16
            || chunk == 0
            || chunk > 16 * CHUNK
        {
            return Err(OpenError::Unreadable(
                "the encryption cost is out of range".into(),
            ));
        }
        let key = derive(passphrase, &header[18..34], cost).map_err(OpenError::Unreadable)?;
        let cipher = XChaCha20Poly1305::new((&key).into());
        let prefix = &header[34..53];
        let size = chunk + TAG;
        let mut buf = vec![0u8; size];
        let mut ahead = vec![0u8; size];
        let mut have = fill(sealed, &mut buf).map_err(|e| OpenError::Io(e.to_string()))?;
        if have < TAG {
            return Err(OpenError::Wrong);
        }
        let mut index = 0u32;
        loop {
            let next = if have == size {
                fill(sealed, &mut ahead).map_err(|e| OpenError::Io(e.to_string()))?
            } else {
                0
            };
            let last = next == 0;
            let plain = cipher
                .decrypt(
                    &nonce(prefix, index, last),
                    Payload {
                        msg: &buf[..have],
                        aad: &header,
                    },
                )
                .map_err(|_| OpenError::Wrong)?;
            out.write_all(&plain)
                .map_err(|e| OpenError::Io(e.to_string()))?;
            if last {
                return Ok(());
            }
            std::mem::swap(&mut buf, &mut ahead);
            have = next;
            index += 1;
        }
    }
}

#[cfg(all(test, feature = "cipher"))]
mod tests {
    use super::*;

    const PASS: &str = "correct horse battery";
    const CHEAP: Cost = Cost {
        memory_kib: 64,
        iterations: 1,
        lanes: 1,
    };

    fn seal_cheap(plain: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        seal_with(PASS, CHEAP, &mut &plain[..], &mut out).unwrap();
        out
    }

    #[test]
    fn a_sealed_value_opens_with_the_same_passphrase() {
        let sealed = seal_cheap(b"static value");
        assert!(!sealed.windows(6).any(|w| w == b"static"));
        assert_eq!(open_bytes(PASS, &sealed).unwrap(), b"static value");
    }

    #[test]
    fn data_across_chunk_borders_and_empty_data_round_trip() {
        for len in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, 3 * CHUNK, 3 * CHUNK + 7] {
            let plain: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            assert_eq!(
                open_bytes(PASS, &seal_cheap(&plain)).unwrap(),
                plain,
                "{len}"
            );
        }
    }

    #[test]
    fn a_wrong_passphrase_says_only_that() {
        let sealed = seal_cheap(b"x");
        let error = open_bytes("another passphrase!", &sealed).unwrap_err();
        assert_eq!(error, OpenError::Wrong);
        assert_eq!(error.to_string(), WRONG_PASSPHRASE);
    }

    #[test]
    fn a_changed_header_byte_or_ciphertext_byte_does_not_open() {
        let sealed = seal_cheap(&vec![7u8; CHUNK + 10]);
        for at in [7, 20, HEADER_LEN + 3, sealed.len() - 1] {
            let mut copy = sealed.clone();
            copy[at] ^= 1;
            assert!(open_bytes(PASS, &copy).is_err(), "byte {at}");
        }
    }

    #[test]
    fn a_file_cut_at_a_chunk_border_does_not_open() {
        let sealed = seal_cheap(&vec![7u8; 3 * CHUNK]);
        let cut = HEADER_LEN + 2 * (CHUNK + TAG);
        assert_eq!(open_bytes(PASS, &sealed[..cut]), Err(OpenError::Wrong));
    }

    #[test]
    fn a_short_passphrase_is_refused_before_anything_is_written() {
        let mut out = Vec::new();
        let error = seal("short", &mut &b"x"[..], &mut out).unwrap_err();
        assert!(error.contains("at least 12"), "{error}");
        assert!(out.is_empty());
    }

    #[test]
    fn a_hostile_cost_is_refused_before_it_is_paid() {
        let mut sealed = seal_cheap(b"x");
        sealed[6..10].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(
            open_bytes(PASS, &sealed),
            Err(OpenError::Unreadable(_))
        ));
    }

    #[test]
    fn something_that_is_not_a_sealed_part_is_unreadable() {
        assert!(matches!(
            open_bytes(PASS, b"PK\x03\x04 a zip, not a part"),
            Err(OpenError::Unreadable(_))
        ));
    }
}

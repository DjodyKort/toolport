use sha2::{Digest, Sha256};

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn lock_hash(bytes: impl AsRef<[u8]>) -> String {
    finish_lock_hash(Sha256::new().chain_update(bytes))
}

pub(crate) fn finish_lock_hash(hasher: Sha256) -> String {
    format!("sha256:{}", &format!("{:x}", hasher.finalize())[..16])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests_have_the_lock_and_hex_shapes() {
        let full = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(sha256_hex("abc"), full);
        assert_eq!(sha256_hex(b"abc"), full);
        assert_eq!(lock_hash("abc"), "sha256:ba7816bf8f01cfea");
        assert_eq!(hex(&[0, 15, 255]), "000fff");
    }

    #[test]
    fn streamed_lock_hash_matches_one_shot() {
        let streamed = finish_lock_hash(Sha256::new().chain_update("a").chain_update("bc"));
        assert_eq!(streamed, lock_hash("abc"));
    }
}

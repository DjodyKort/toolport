use sha2::{Digest, Sha256};

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
    fn lock_hash_has_the_lock_shape() {
        assert_eq!(lock_hash("abc"), "sha256:ba7816bf8f01cfea");
    }

    #[test]
    fn streamed_lock_hash_matches_one_shot() {
        let streamed = finish_lock_hash(Sha256::new().chain_update("a").chain_update("bc"));
        assert_eq!(streamed, lock_hash("abc"));
    }
}

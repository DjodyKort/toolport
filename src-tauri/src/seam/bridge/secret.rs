use super::BridgeError;

const MAX_BYTES: usize = 64 * 1024;
const MIN_SCRUB_BYTES: usize = 4;
const MASK: &str = "[redacted]";

/// A value for the child's stdin. It has no `Debug`, `Display` or `Serialize` output of its own,
/// is wiped on drop and is masked out of anything the bridge hands back.
pub struct Secret(Vec<u8>);

impl Secret {
    pub fn new(value: impl Into<String>) -> Result<Self, BridgeError> {
        let bytes = value.into().into_bytes();
        if bytes.len() > MAX_BYTES {
            return Err(BridgeError::Usage(format!(
                "secret is longer than {MAX_BYTES} bytes"
            )));
        }
        Ok(Self(bytes))
    }

    pub(super) fn bytes(&self) -> &[u8] {
        &self.0
    }

    /// Masks the value (and its trimmed form) in text that came back from the child.
    pub(super) fn scrub(&self, text: &str) -> String {
        let Ok(value) = std::str::from_utf8(&self.0) else {
            return text.to_string();
        };
        let mut out = text.to_string();
        for needle in [value.trim_end_matches(['\r', '\n']), value] {
            if needle.len() >= MIN_SCRUB_BYTES && out.contains(needle) {
                out = out.replace(needle, MASK);
            }
        }
        out
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        for byte in self.0.iter_mut() {
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_the_value() {
        let secret = Secret::new("CANARY-value-123").unwrap();
        assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
    }

    #[test]
    fn scrub_masks_the_value_and_its_trimmed_form() {
        let secret = Secret::new("CANARY-value-123\n").unwrap();
        let scrubbed = secret.scrub("got CANARY-value-123 and CANARY-value-123\n");
        assert_eq!(scrubbed, "got [redacted] and [redacted]\n");
        assert_eq!(Secret::new("ab").unwrap().scrub("ab ab"), "ab ab");
    }

    #[test]
    fn oversized_values_are_refused() {
        assert!(Secret::new("x".repeat(MAX_BYTES + 1)).is_err());
        assert!(Secret::new("x".repeat(MAX_BYTES)).is_ok());
    }
}

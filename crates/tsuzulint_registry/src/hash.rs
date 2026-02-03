use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HashError {
    #[error("Hash mismatch: expected {expected}, actual {actual}")]
    Mismatch { expected: String, actual: String },

    #[error("Invalid hash format: {0}")]
    InvalidFormat(String),
}

pub struct HashVerifier;

impl HashVerifier {
    /// Compute SHA256 hash of bytes
    pub fn compute(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let result = hasher.finalize();
        hex::encode(result)
    }

    /// Verify hash matches expected value
    pub fn verify(bytes: &[u8], expected: &str) -> Result<(), HashError> {
        // Validate expected hash format (basic length check for SHA256 hex string)
        if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(HashError::InvalidFormat(expected.to_string()));
        }

        let actual = Self::compute(bytes);

        if actual.eq_ignore_ascii_case(expected) {
            Ok(())
        } else {
            Err(HashError::Mismatch {
                expected: expected.to_string(),
                actual,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute() {
        let data = b"hello world";
        let hash = HashVerifier::compute(data);
        // calculated with `echo -n "hello world" | shasum -a 256`
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_verify_success() {
        let data = b"hello world";
        let hash = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert!(HashVerifier::verify(data, hash).is_ok());
    }

    #[test]
    fn test_verify_case_insensitive() {
        let data = b"hello world";
        let hash = "B94D27B9934D3E08A52E52D7DA7DABFAC484EFE37A5380EE9088F7ACE2EFCDE9";
        assert!(HashVerifier::verify(data, hash).is_ok());
    }

    #[test]
    fn test_verify_mismatch() {
        let data = b"hello world";
        // Changed first char
        let hash = "a94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        match HashVerifier::verify(data, hash) {
            Err(HashError::Mismatch { expected, actual }) => {
                assert_eq!(expected, hash);
                assert_eq!(
                    actual,
                    "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
                );
            }
            _ => panic!("Expected Mismatch error"),
        }
    }

    #[test]
    fn test_verify_invalid_format() {
        let data = b"hello world";
        let hash = "invalid";
        match HashVerifier::verify(data, hash) {
            Err(HashError::InvalidFormat(h)) => assert_eq!(h, hash),
            _ => panic!("Expected InvalidFormat error"),
        }
    }

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_compute_returns_valid_hash(bytes in any::<Vec<u8>>()) {
            let hash = HashVerifier::compute(&bytes);

            // Should be 64 characters long
            prop_assert_eq!(hash.len(), 64);

            // Should be all hex digits
            prop_assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));

            // Should be lowercase (implementation detail choice, but good to enforce)
            prop_assert!(hash.chars().all(|c| !c.is_ascii_uppercase()));
        }

        #[test]
        fn test_round_trip_verify(bytes in any::<Vec<u8>>()) {
            let hash = HashVerifier::compute(&bytes);
            prop_assert!(HashVerifier::verify(&bytes, &hash).is_ok());
        }

        #[test]
        fn test_verify_case_insensitive_proptest(bytes in any::<Vec<u8>>()) {
            let hash = HashVerifier::compute(&bytes);
            let upper_hash = hash.to_uppercase();

            prop_assert!(HashVerifier::verify(&bytes, &upper_hash).is_ok());
        }
    }
}

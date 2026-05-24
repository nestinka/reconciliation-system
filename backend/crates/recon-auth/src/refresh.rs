use sha2::{Digest, Sha256};

/// Returns (plaintext_token, sha256_hex_hash). Only the hash is persisted.
pub fn generate() -> (String, String) {
    use rand::RngCore;
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    let plaintext = hex::encode(b);
    let h = hash(&plaintext);
    (plaintext, h)
}

pub fn hash(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generate_is_unique_and_hash_matches() {
        let (p1, h1) = generate();
        let (p2, _h2) = generate();
        assert_ne!(p1, p2);
        assert_eq!(p1.len(), 64);
        assert_eq!(h1, hash(&p1));
        assert_ne!(h1, p1);
    }
    #[test]
    fn hash_is_stable() {
        assert_eq!(hash("abc"), hash("abc"));
        assert_ne!(hash("abc"), hash("abd"));
    }
}

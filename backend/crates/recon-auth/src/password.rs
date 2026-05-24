use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use crate::error::AuthError;

pub fn hash_password(plain: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AuthError::Hash(e.to_string()))
}

pub fn verify_password(plain: &str, hash: &str) -> Result<bool, AuthError> {
    let parsed = PasswordHash::new(hash).map_err(|e| AuthError::Hash(e.to_string()))?;
    Ok(Argon2::default().verify_password(plain.as_bytes(), &parsed).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hash_then_verify_roundtrip() {
        let h = hash_password("s3cret-pw").unwrap();
        assert!(h.starts_with("$argon2"));
        assert!(verify_password("s3cret-pw", &h).unwrap());
    }
    #[test]
    fn wrong_password_rejected() {
        let h = hash_password("s3cret-pw").unwrap();
        assert!(!verify_password("nope", &h).unwrap());
    }
    #[test]
    fn malformed_hash_errors() {
        assert!(matches!(verify_password("x", "not-a-hash"), Err(AuthError::Hash(_))));
    }
}

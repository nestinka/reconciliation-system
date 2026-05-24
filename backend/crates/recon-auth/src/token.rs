use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation, Algorithm, errors::ErrorKind};
use serde::{Deserialize, Serialize};
use recon_domain::UserRole;
use crate::error::AuthError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccessClaims {
    pub sub: String,
    pub tid: String,
    pub role: UserRole,
    pub jti: String,
    pub iat: i64,
    pub exp: i64,
    pub typ: String,
}

pub fn encode_access(secret: &[u8], user_id: &str, tenant_id: &str, role: UserRole, ttl_secs: i64, now_unix: i64) -> Result<String, AuthError> {
    let claims = AccessClaims {
        sub: user_id.to_string(),
        tid: tenant_id.to_string(),
        role,
        jti: uuidish(),
        iat: now_unix,
        exp: now_unix + ttl_secs,
        typ: "access".to_string(),
    };
    encode(&Header::new(Algorithm::HS256), &claims, &EncodingKey::from_secret(secret))
        .map_err(|_| AuthError::TokenInvalid)
}

pub fn decode_access(secret: &[u8], token: &str, now_unix: i64) -> Result<AccessClaims, AuthError> {
    let mut v = Validation::new(Algorithm::HS256);
    v.validate_exp = false; // we validate exp manually against now_unix for deterministic tests
    let data = decode::<AccessClaims>(token, &DecodingKey::from_secret(secret), &v)
        .map_err(|e| match e.kind() { ErrorKind::ExpiredSignature => AuthError::TokenExpired, _ => AuthError::TokenInvalid })?;
    let c = data.claims;
    if c.typ != "access" { return Err(AuthError::TokenInvalid); }
    if now_unix >= c.exp { return Err(AuthError::TokenExpired); }
    Ok(c)
}

fn uuidish() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    const S: &[u8] = b"test-secret-please-rotate";
    #[test]
    fn roundtrip_valid() {
        let t = encode_access(S, "user-mia", "tenant-acme", UserRole::Operator, 900, 1000).unwrap();
        let c = decode_access(S, &t, 1100).unwrap();
        assert_eq!(c.sub, "user-mia");
        assert_eq!(c.tid, "tenant-acme");
        assert_eq!(c.role, UserRole::Operator);
        assert_eq!(c.typ, "access");
    }
    #[test]
    fn expired_rejected() {
        let t = encode_access(S, "u", "t", UserRole::Admin, 900, 1000).unwrap();
        assert_eq!(decode_access(S, &t, 5000), Err(AuthError::TokenExpired));
    }
    #[test]
    fn tampered_secret_rejected() {
        let t = encode_access(S, "u", "t", UserRole::Admin, 900, 1000).unwrap();
        assert_eq!(decode_access(b"other-secret", &t, 1100), Err(AuthError::TokenInvalid));
    }
}

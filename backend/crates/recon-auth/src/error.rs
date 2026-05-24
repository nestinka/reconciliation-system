#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    InvalidCredentials,
    TokenExpired,
    TokenInvalid,
    Forbidden,
    Locked,
    Hash(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::InvalidCredentials => write!(f, "invalid credentials"),
            AuthError::TokenExpired => write!(f, "token expired"),
            AuthError::TokenInvalid => write!(f, "token invalid"),
            AuthError::Forbidden => write!(f, "forbidden"),
            AuthError::Locked => write!(f, "account locked"),
            AuthError::Hash(m) => write!(f, "hash error: {m}"),
        }
    }
}
impl std::error::Error for AuthError {}

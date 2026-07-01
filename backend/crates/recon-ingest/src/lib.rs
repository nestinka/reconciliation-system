pub mod bai2;
pub mod camt053;
pub mod csv;
pub mod detect;
pub mod money;
pub mod mt940;
pub mod mt942;
pub mod mt94x_shared;
pub mod pdf;

use recon_domain::Direction;

/// A parsed transaction draft. No id / tenant / source yet — the API assigns
/// those when mapping to a `CanonicalTransaction`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTxn {
    pub external_ref: String,
    pub value_date: String,
    pub posted_at: Option<String>,
    pub amount_minor: i64,
    pub currency: Option<String>,
    pub direction: Direction,
    pub counterparty: Option<String>,
    pub description: String,
    pub counterparty_bic: Option<String>,
    pub counterparty_account: Option<String>,
}

/// One row-level parse failure. Collected so a whole file is rejected atomically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowError {
    pub row: usize,
    pub field: String,
    pub message: String,
}

impl RowError {
    pub fn new(row: usize, field: impl Into<String>, message: impl Into<String>) -> Self {
        Self { row, field: field.into(), message: message.into() }
    }
}

/// Parse raw file bytes into transaction drafts. On ANY row error, returns Err
/// with the full list (atomic: the caller stores nothing).
pub trait Parser {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<ParsedTxn>, Vec<RowError>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_error_constructs() {
        let e = RowError::new(3, "amount", "bad");
        assert_eq!(e.row, 3);
        assert_eq!(e.field, "amount");
        assert_eq!(e.message, "bad");
    }
}

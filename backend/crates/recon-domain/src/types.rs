use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Bank,
    Ledger,
    CrossSystem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Debit,
    Credit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchType {
    Matched,
    Partial,
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakType {
    Unmatched,
    Partial,
    Duplicate,
    Break,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakStatus {
    Open,
    Investigating,
    PendingApproval,
    Resolved,
    WrittenOff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgeingBucket {
    #[serde(rename = "0-1d")]
    ZeroToOne,
    #[serde(rename = "2-7d")]
    TwoToSeven,
    #[serde(rename = "8-30d")]
    EightToThirty,
    #[serde(rename = "30d+")]
    ThirtyPlus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    Operator,
    Approver,
    Admin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tenant {
    pub id: String,
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub name: String,
    pub email: String,
    pub disabled: bool,
    /// Role in the active-tenant context.
    pub role: UserRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Membership {
    pub tenant_id: String,
    pub tenant_name: String,
    pub role: UserRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub tenant_id: String,
    pub kind: SourceKind,
    pub name: String,
    pub currency: String,
    pub format_dialect: Option<String>,
    pub pdf_profile: Option<String>,
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalTransaction {
    pub id: String,
    pub tenant_id: String,
    pub source_id: String,
    pub external_ref: String,
    pub value_date: String, // "YYYY-MM-DD"
    pub posted_at: String,  // RFC3339
    pub amount_minor: i64,
    pub currency: String,
    pub direction: Direction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterparty: Option<String>,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterparty_bic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterparty_account: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStats {
    pub matched: i64,
    pub unmatched: i64,
    pub partial: i64,
    pub duplicate: i64,
    pub break_count: i64,
    pub match_rate_pct: f64,
    pub value_at_risk_minor: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconciliationRun {
    pub id: String,
    pub tenant_id: String,
    pub name: String,
    pub source_a_id: String,
    pub source_b_id: String,
    pub status: RunStatus,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    pub config_version: String,
    pub stats: RunStats,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchDecision {
    pub id: String,
    pub run_id: String,
    #[serde(rename = "type")]
    pub match_type: MatchType,
    pub txn_ids: Vec<String>,
    pub score: f64,
    pub config_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Break {
    pub id: String,
    pub tenant_id: String,
    pub run_id: String,
    pub case_id: String,
    #[serde(rename = "type")]
    pub break_type: BreakType,
    pub status: BreakStatus,
    pub ageing_days: i64,
    pub ageing_bucket: AgeingBucket,
    pub value_minor: i64,
    pub currency: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee_id: Option<String>,
    pub txn_ids: Vec<String>,
    pub opened_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_serializes_camel_case_with_renamed_enums() {
        let s = Source {
            id: "src-acme-cross".into(),
            tenant_id: "tenant-acme".into(),
            kind: SourceKind::CrossSystem,
            name: "Acme Cross".into(),
            currency: "USD".into(),
            format_dialect: None,
            pdf_profile: None,
            disabled: false,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["tenantId"], "tenant-acme");
        assert_eq!(v["kind"], "cross_system");
    }

    #[test]
    fn break_status_and_ageing_bucket_wire_values() {
        assert_eq!(
            serde_json::to_value(BreakStatus::PendingApproval).unwrap(),
            "pending_approval"
        );
        assert_eq!(
            serde_json::to_value(AgeingBucket::EightToThirty).unwrap(),
            "8-30d"
        );
        assert_eq!(serde_json::to_value(BreakType::Break).unwrap(), "break");
        assert_eq!(
            serde_json::to_value(MatchType::Duplicate).unwrap(),
            "duplicate"
        );
    }

    #[test]
    fn user_serializes_camel_case_with_email() {
        let u = User { id: "u1".into(), name: "Mia".into(), email: "mia@acme.test".into(), disabled: false, role: UserRole::Operator };
        let j = serde_json::to_value(&u).unwrap();
        assert_eq!(j["email"], "mia@acme.test");
        assert_eq!(j["disabled"], false);
        assert_eq!(j["role"], "operator");
    }
    #[test]
    fn membership_camel_case() {
        let m = Membership { tenant_id: "t1".into(), tenant_name: "Acme".into(), role: UserRole::Admin };
        let j = serde_json::to_value(&m).unwrap();
        assert_eq!(j["tenantId"], "t1");
        assert_eq!(j["tenantName"], "Acme");
        assert_eq!(j["role"], "admin");
    }

    #[test]
    fn canonical_transaction_has_optional_counterparty_bic_and_account() {
        let t = CanonicalTransaction {
            id: "txn-x".into(),
            tenant_id: "t".into(),
            source_id: "s".into(),
            external_ref: "r".into(),
            value_date: "2026-01-01".into(),
            posted_at: "2026-01-01T00:00:00Z".into(),
            amount_minor: 100,
            currency: "EUR".into(),
            direction: Direction::Credit,
            counterparty: None,
            description: "".into(),
            counterparty_bic: Some("DEUTDEFF".into()),
            counterparty_account: Some("DE89370400440532013000".into()),
        };
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["counterpartyBic"], "DEUTDEFF");
        assert_eq!(v["counterpartyAccount"], "DE89370400440532013000");
    }
}

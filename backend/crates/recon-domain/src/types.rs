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
}

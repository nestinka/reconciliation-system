use recon_domain::*;
use sqlx::FromRow;
use time::format_description::FormatItem;
use time::macros::format_description;
use time::{format_description::well_known::Rfc3339, Date, OffsetDateTime};

const YMD: &[FormatItem<'static>] = format_description!("[year]-[month]-[day]");

pub fn date_to_string(d: Date) -> String {
    d.format(YMD).unwrap_or_default()
}
pub fn ts_to_string(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

fn parse_role(s: &str) -> UserRole {
    match s {
        "approver" => UserRole::Approver,
        "admin" => UserRole::Admin,
        _ => UserRole::Operator,
    }
}
fn parse_source_kind(s: &str) -> SourceKind {
    match s {
        "bank" => SourceKind::Bank,
        "ledger" => SourceKind::Ledger,
        _ => SourceKind::CrossSystem,
    }
}
fn parse_direction(s: &str) -> Direction {
    if s == "credit" {
        Direction::Credit
    } else {
        Direction::Debit
    }
}
fn parse_run_status(s: &str) -> RunStatus {
    match s {
        "completed" => RunStatus::Completed,
        "failed" => RunStatus::Failed,
        _ => RunStatus::Running,
    }
}
fn parse_match_type(s: &str) -> MatchType {
    match s {
        "matched" => MatchType::Matched,
        "duplicate" => MatchType::Duplicate,
        _ => MatchType::Partial,
    }
}
fn parse_break_type(s: &str) -> BreakType {
    match s {
        "unmatched" => BreakType::Unmatched,
        "partial" => BreakType::Partial,
        "duplicate" => BreakType::Duplicate,
        _ => BreakType::Break,
    }
}
pub fn parse_break_status(s: &str) -> BreakStatus {
    match s {
        "investigating" => BreakStatus::Investigating,
        "pending_approval" => BreakStatus::PendingApproval,
        "resolved" => BreakStatus::Resolved,
        "written_off" => BreakStatus::WrittenOff,
        _ => BreakStatus::Open,
    }
}

#[derive(FromRow)]
pub struct TenantRow {
    pub id: String,
    pub name: String,
    pub slug: String,
}
impl From<TenantRow> for Tenant {
    fn from(r: TenantRow) -> Self {
        Tenant {
            id: r.id,
            name: r.name,
            slug: r.slug,
        }
    }
}

#[derive(FromRow)]
pub struct UserRow {
    pub id: String,
    pub name: String,
    pub email: String,
    pub disabled: bool,
    pub role: String,
}
impl From<UserRow> for User {
    fn from(r: UserRow) -> Self {
        User {
            id: r.id,
            name: r.name,
            email: r.email,
            disabled: r.disabled,
            role: parse_role(&r.role),
        }
    }
}

#[derive(FromRow)]
pub struct SourceRow {
    pub id: String,
    pub tenant_id: String,
    pub kind: String,
    pub name: String,
    pub currency: String,
    pub format_dialect: Option<String>,
}
impl From<SourceRow> for Source {
    fn from(r: SourceRow) -> Self {
        Source {
            id: r.id,
            tenant_id: r.tenant_id,
            kind: parse_source_kind(&r.kind),
            name: r.name,
            currency: r.currency,
            format_dialect: r.format_dialect,
        }
    }
}

#[derive(FromRow)]
pub struct TxnRow {
    pub id: String,
    pub tenant_id: String,
    pub source_id: String,
    pub external_ref: String,
    pub value_date: Date,
    pub posted_at: OffsetDateTime,
    pub amount_minor: i64,
    pub currency: String,
    pub direction: String,
    pub counterparty: Option<String>,
    pub description: String,
}
impl From<TxnRow> for CanonicalTransaction {
    fn from(r: TxnRow) -> Self {
        CanonicalTransaction {
            id: r.id,
            tenant_id: r.tenant_id,
            source_id: r.source_id,
            external_ref: r.external_ref,
            value_date: date_to_string(r.value_date),
            posted_at: ts_to_string(r.posted_at),
            amount_minor: r.amount_minor,
            currency: r.currency,
            direction: parse_direction(&r.direction),
            counterparty: r.counterparty,
            description: r.description,
        }
    }
}

#[derive(FromRow)]
pub struct RunRow {
    pub id: String,
    pub tenant_id: String,
    pub name: String,
    pub source_a_id: String,
    pub source_b_id: String,
    pub status: String,
    pub started_at: OffsetDateTime,
    pub completed_at: Option<OffsetDateTime>,
    pub config_version: String,
    pub stats: serde_json::Value,
}
impl TryFrom<RunRow> for ReconciliationRun {
    type Error = serde_json::Error;
    fn try_from(r: RunRow) -> Result<Self, Self::Error> {
        Ok(ReconciliationRun {
            id: r.id,
            tenant_id: r.tenant_id,
            name: r.name,
            source_a_id: r.source_a_id,
            source_b_id: r.source_b_id,
            status: parse_run_status(&r.status),
            started_at: ts_to_string(r.started_at),
            completed_at: r.completed_at.map(ts_to_string),
            config_version: r.config_version,
            stats: serde_json::from_value(r.stats)?,
        })
    }
}

#[derive(FromRow)]
pub struct DecisionRow {
    pub id: String,
    pub run_id: String,
    #[sqlx(rename = "type")]
    pub type_: String,
    pub txn_ids: Vec<String>,
    pub score: f64,
    pub config_version: String,
}
impl From<DecisionRow> for MatchDecision {
    fn from(r: DecisionRow) -> Self {
        MatchDecision {
            id: r.id,
            run_id: r.run_id,
            match_type: parse_match_type(&r.type_),
            txn_ids: r.txn_ids,
            score: r.score,
            config_version: r.config_version,
        }
    }
}

#[derive(FromRow)]
pub struct BreakRow {
    pub id: String,
    pub tenant_id: String,
    pub run_id: String,
    pub case_id: String,
    #[sqlx(rename = "type")]
    pub type_: String,
    pub status: String,
    pub value_minor: i64,
    pub currency: String,
    pub assignee_id: Option<String>,
    pub txn_ids: Vec<String>,
    pub opened_at: OffsetDateTime,
}
impl BreakRow {
    /// Ageing is computed at read time relative to `now`.
    pub fn into_break(self, now: OffsetDateTime) -> Break {
        let days = ((now - self.opened_at).whole_days()).max(0);
        Break {
            id: self.id,
            tenant_id: self.tenant_id,
            run_id: self.run_id,
            case_id: self.case_id,
            break_type: parse_break_type(&self.type_),
            status: parse_break_status(&self.status),
            ageing_days: days,
            ageing_bucket: recon_domain::ageing::ageing_bucket(days),
            value_minor: self.value_minor,
            currency: self.currency,
            assignee_id: self.assignee_id,
            txn_ids: self.txn_ids,
            opened_at: ts_to_string(self.opened_at),
        }
    }
}

#[derive(FromRow)]
pub struct CaseRow {
    pub id: String,
    pub break_id: String,
    pub assignee_id: Option<String>,
    pub status: String,
}

#[derive(FromRow)]
pub struct EventRow {
    pub id: String,
    pub actor_id: String,
    pub at: OffsetDateTime,
    pub kind: String,
    pub payload: serde_json::Value,
}
impl TryFrom<EventRow> for CaseEvent {
    type Error = serde_json::Error;
    fn try_from(r: EventRow) -> Result<Self, Self::Error> {
        // Reconstruct the body from {kind, payload} via the adjacently-tagged enum.
        let body: CaseEventBody =
            serde_json::from_value(serde_json::json!({ "kind": r.kind, "payload": r.payload }))?;
        Ok(CaseEvent {
            id: r.id,
            actor_id: r.actor_id,
            at: ts_to_string(r.at),
            body,
        })
    }
}

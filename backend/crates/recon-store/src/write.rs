use crate::rows::BreakRow;
use crate::{Store, StoreError};
use recon_domain::*;
use time::OffsetDateTime;
use uuid::Uuid;

impl Store {
    async fn next_seq(
        &self,
        tx: &mut sqlx::PgConnection,
        case_id: &str,
    ) -> Result<i32, StoreError> {
        let max: Option<i32> =
            sqlx::query_scalar("SELECT max(seq) FROM case_events WHERE case_id = $1")
                .bind(case_id)
                .fetch_one(&mut *tx)
                .await?;
        Ok(max.unwrap_or(0) + 1)
    }

    pub async fn assign_break(
        &self,
        tenant_id: &str,
        break_id: &str,
        assignee_id: &str,
        actor_id: &str,
    ) -> Result<Break, StoreError> {
        let now = OffsetDateTime::now_utc();
        let mut tx = self.pool.begin().await?;

        // The assignee must belong to the caller's tenant.
        let assignee_ok: Option<String> =
            sqlx::query_scalar("SELECT id FROM users WHERE id = $1 AND tenant_id = $2")
                .bind(assignee_id)
                .bind(tenant_id)
                .fetch_optional(&mut *tx)
                .await?;
        if assignee_ok.is_none() {
            return Err(StoreError::NotFound);
        }

        let brow: Option<BreakRow> =
            sqlx::query_as("SELECT * FROM breaks WHERE id = $1 AND tenant_id = $2 FOR UPDATE")
                .bind(break_id)
                .bind(tenant_id)
                .fetch_optional(&mut *tx)
                .await?;
        let brk = brow.ok_or(StoreError::NotFound)?;
        let new_status = if brk.status == "open" {
            "investigating"
        } else {
            brk.status.as_str()
        };

        sqlx::query(
            "UPDATE breaks SET assignee_id = $1, status = $2 WHERE id = $3 AND tenant_id = $4",
        )
        .bind(assignee_id)
        .bind(new_status)
        .bind(break_id)
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE cases SET assignee_id = $1, status = CASE WHEN status = 'open' THEN 'investigating' ELSE status END WHERE id = $2 AND tenant_id = $3")
            .bind(assignee_id).bind(&brk.case_id).bind(tenant_id).execute(&mut *tx).await?;

        let seq = self.next_seq(&mut tx, &brk.case_id).await?;
        sqlx::query("INSERT INTO case_events(id,tenant_id,case_id,seq,kind,actor_id,at,payload) VALUES ($1,$2,$3,$4,'assignment',$5,$6,$7)")
            .bind(Uuid::new_v4().to_string()).bind(tenant_id).bind(&brk.case_id).bind(seq)
            .bind(actor_id).bind(now)
            .bind(serde_json::json!({ "assigneeId": assignee_id }))
            .execute(&mut *tx).await?;

        let updated: BreakRow = sqlx::query_as("SELECT * FROM breaks WHERE id = $1")
            .bind(break_id)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(updated.into_break(now))
    }

    pub async fn append_case_event(
        &self,
        tenant_id: &str,
        case_id: &str,
        ev: NewCaseEvent,
    ) -> Result<Case, StoreError> {
        let now = OffsetDateTime::now_utc();
        let mut tx = self.pool.begin().await?;

        // Lock + load the case state WITHIN the tx so the four-eyes gate and the write are atomic.
        let crow: Option<crate::rows::CaseRow> = sqlx::query_as("SELECT id, break_id, assignee_id, status FROM cases WHERE id = $1 AND tenant_id = $2 FOR UPDATE")
            .bind(case_id).bind(tenant_id).fetch_optional(&mut *tx).await?;
        let crow = crow.ok_or(StoreError::NotFound)?;
        let status = crate::rows::parse_break_status(&crow.status);
        let erows: Vec<crate::rows::EventRow> = sqlx::query_as("SELECT id, actor_id, at, kind, payload FROM case_events WHERE case_id = $1 AND tenant_id = $2 ORDER BY seq")
            .bind(case_id).bind(tenant_id).fetch_all(&mut *tx).await?;
        let events: Vec<CaseEvent> = erows
            .into_iter()
            .map(CaseEvent::try_from)
            .collect::<Result<_, _>>()?;
        let case_snapshot = Case {
            id: crow.id,
            break_id: crow.break_id,
            assignee_id: crow.assignee_id,
            status,
            events,
        };

        let new_status: Option<BreakStatus> = match &ev.body {
            CaseEventBody::ApprovalRequested { .. } => {
                if !matches!(status, BreakStatus::Open | BreakStatus::Investigating) {
                    return Err(StoreError::Conflict(
                        "case is not open for an approval request".into(),
                    ));
                }
                Some(BreakStatus::PendingApproval)
            }
            CaseEventBody::Approved {} => {
                if status != BreakStatus::PendingApproval {
                    return Err(StoreError::Conflict("case is not pending approval".into()));
                }
                let actor: Option<crate::rows::UserRow> = sqlx::query_as(
                    "SELECT id, name, role FROM users WHERE id = $1 AND tenant_id = $2",
                )
                .bind(&ev.actor_id)
                .bind(tenant_id)
                .fetch_optional(&mut *tx)
                .await?;
                let actor: User = actor.ok_or(StoreError::NotFound)?.into();
                recon_domain::can_approve(&case_snapshot, &actor)
                    .map_err(|e| StoreError::Forbidden(e.to_string()))?;
                Some(BreakStatus::Resolved)
            }
            CaseEventBody::Rejected { .. } => {
                if status != BreakStatus::PendingApproval {
                    return Err(StoreError::Conflict("case is not pending approval".into()));
                }
                Some(BreakStatus::Investigating)
            }
            CaseEventBody::Assignment { .. } => {
                if status == BreakStatus::Open {
                    Some(BreakStatus::Investigating)
                } else {
                    None
                }
            }
            _ => None,
        };

        let kind_val = serde_json::to_value(&ev.body)?;
        let kind = kind_val["kind"].as_str().unwrap().to_string();
        let payload = kind_val["payload"].clone();
        let seq = self.next_seq(&mut tx, case_id).await?;
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO case_events(id,tenant_id,case_id,seq,kind,actor_id,at,payload) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(&id).bind(tenant_id).bind(case_id).bind(seq).bind(&kind).bind(&ev.actor_id).bind(now).bind(&payload)
            .execute(&mut *tx).await?;

        if let Some(status) = new_status {
            let status_str = serde_json::to_value(status)?.as_str().unwrap().to_string();
            let assignee = if let CaseEventBody::Assignment { assignee_id } = &ev.body {
                Some(assignee_id.clone())
            } else {
                None
            };
            sqlx::query("UPDATE cases SET status = $1, assignee_id = COALESCE($2, assignee_id) WHERE id = $3 AND tenant_id = $4")
                .bind(&status_str).bind(&assignee).bind(case_id).bind(tenant_id).execute(&mut *tx).await?;
            sqlx::query("UPDATE breaks SET status = $1, assignee_id = COALESCE($2, assignee_id) WHERE case_id = $3 AND tenant_id = $4")
                .bind(&status_str).bind(&assignee).bind(case_id).bind(tenant_id).execute(&mut *tx).await?;
        }

        tx.commit().await?;
        self.load_case(tenant_id, case_id).await
    }
}

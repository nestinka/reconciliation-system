use crate::{Store, StoreError};
use recon_auth::password::hash_password;
use recon_domain::CanonicalTransaction;
use recon_matching::{reconcile, MatchConfig};

impl Store {
    /// Reset (idempotent) and load the deterministic demo dataset.
    pub async fn seed(&self) -> Result<(), StoreError> {
        self.migrate().await?;
        let mut tx = self.pool.begin().await?;

        // Idempotent reset (children first to respect FKs).
        for t in [
            "audit_anchors",
            "audit_events",
            "case_events",
            "breaks",
            "match_decisions",
            "cases",
            "reconciliation_runs",
            "canonical_transactions",
            "sources",
            "refresh_tokens",
            "password_reset_tokens",
            "user_credentials",
            "memberships",
            "users",
            "tenants",
        ] {
            sqlx::query(&format!("DELETE FROM {t}"))
                .execute(&mut *tx)
                .await?;
        }

        // Tenants
        for (id, name, slug) in [
            ("tenant-acme", "Acme Capital", "acme-capital"),
            ("tenant-globex", "Globex Markets", "globex-markets"),
        ] {
            sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ($1,$2,$3)")
                .bind(id)
                .bind(name)
                .bind(slug)
                .execute(&mut *tx)
                .await?;
        }
        // Users — global identities with per-tenant memberships.
        // dev login: <email> / Password123!
        let pw_hash = hash_password("Password123!").map_err(|e| {
            StoreError::Db(sqlx::Error::Protocol(format!("hash_password failed: {e}")))
        })?;

        // (id, name, email)
        for (id, name, email) in [
            ("user-mia", "Mia", "mia@acme.test"),
            ("user-theo", "Theo", "theo@acme.test"),
            ("user-ada", "Ada", "ada@acme.test"),
        ] {
            sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ($1,$2,$3,false)")
                .bind(id)
                .bind(name)
                .bind(email)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO user_credentials(user_id,password_hash) VALUES ($1,$2)")
                .bind(id)
                .bind(&pw_hash)
                .execute(&mut *tx)
                .await?;
        }

        // Memberships: (user_id, tenant_id, role)
        for (uid, tid, role) in [
            ("user-mia", "tenant-acme", "operator"),
            ("user-theo", "tenant-acme", "approver"),
            ("user-ada", "tenant-acme", "admin"),
            ("user-ada", "tenant-globex", "admin"),
        ] {
            sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ($1,$2,$3)")
                .bind(uid)
                .bind(tid)
                .bind(role)
                .execute(&mut *tx)
                .await?;
        }
        // Sources
        for (id, tid, kind, name, cur) in [
            (
                "src-acme-bank",
                "tenant-acme",
                "bank",
                "Acme Bank Statement",
                "GBP",
            ),
            (
                "src-acme-ledger",
                "tenant-acme",
                "ledger",
                "Acme General Ledger",
                "GBP",
            ),
            (
                "src-globex-bank",
                "tenant-globex",
                "bank",
                "Globex Bank Statement",
                "USD",
            ),
            (
                "src-globex-ledger",
                "tenant-globex",
                "ledger",
                "Globex General Ledger",
                "USD",
            ),
        ] {
            sqlx::query(
                "INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ($1,$2,$3,$4,$5)",
            )
            .bind(id)
            .bind(tid)
            .bind(kind)
            .bind(name)
            .bind(cur)
            .execute(&mut *tx)
            .await?;
        }

        // Raw transactions: (id, source, ref, date, amount_minor, direction, tenant, currency)
        let txns = [
            (
                "txn-a001",
                "src-acme-bank",
                "BANK-1",
                "2026-05-01",
                1_000_000i64,
                "debit",
                "tenant-acme",
                "GBP",
            ),
            (
                "txn-b001",
                "src-acme-ledger",
                "BANK-1",
                "2026-05-01",
                1_000_000,
                "debit",
                "tenant-acme",
                "GBP",
            ),
            (
                "txn-a002",
                "src-acme-bank",
                "BANK-2",
                "2026-05-01",
                500_000,
                "credit",
                "tenant-acme",
                "GBP",
            ),
            (
                "txn-b002",
                "src-acme-ledger",
                "BANK-2",
                "2026-05-01",
                500_000,
                "credit",
                "tenant-acme",
                "GBP",
            ),
            (
                "txn-a003",
                "src-acme-bank",
                "BANK-3",
                "2026-05-02",
                250_000,
                "debit",
                "tenant-acme",
                "GBP",
            ),
            (
                "txn-b003",
                "src-acme-ledger",
                "BANK-3",
                "2026-05-02",
                250_000,
                "debit",
                "tenant-acme",
                "GBP",
            ),
            (
                "txn-a005",
                "src-acme-bank",
                "BANK-5",
                "2026-05-04",
                320_000,
                "debit",
                "tenant-acme",
                "GBP",
            ),
            (
                "txn-b005",
                "src-acme-ledger",
                "BANK-5",
                "2026-05-04",
                319_500,
                "debit",
                "tenant-acme",
                "GBP",
            ),
            (
                "txn-c004",
                "src-acme-ledger",
                "DUP-9-a",
                "2026-05-10",
                95_000,
                "debit",
                "tenant-acme",
                "GBP",
            ),
            (
                "txn-c005",
                "src-acme-ledger",
                "DUP-9-b",
                "2026-05-10",
                95_000,
                "debit",
                "tenant-acme",
                "GBP",
            ),
            (
                "txn-brk001",
                "src-acme-bank",
                "BANK-99",
                "2026-05-15",
                125_000,
                "debit",
                "tenant-acme",
                "GBP",
            ),
            (
                "txn-brk002",
                "src-acme-bank",
                "BANK-10",
                "2026-05-16",
                67_500,
                "credit",
                "tenant-acme",
                "GBP",
            ),
            (
                "txn-brk005",
                "src-acme-bank",
                "BANK-18",
                "2026-05-18",
                210_000,
                "debit",
                "tenant-acme",
                "GBP",
            ),
            (
                "txn-brk006",
                "src-acme-bank",
                "BANK-19",
                "2026-05-23",
                88_000,
                "credit",
                "tenant-acme",
                "GBP",
            ),
            (
                "txn-g001",
                "src-globex-bank",
                "GB-1",
                "2026-05-01",
                2_000_000,
                "debit",
                "tenant-globex",
                "USD",
            ),
            (
                "txn-g002",
                "src-globex-ledger",
                "GB-1",
                "2026-05-01",
                2_000_000,
                "debit",
                "tenant-globex",
                "USD",
            ),
            (
                "txn-g005",
                "src-globex-bank",
                "GB-9",
                "2026-05-10",
                390_000,
                "debit",
                "tenant-globex",
                "USD",
            ),
        ];
        for (id, src, eref, date, amt, dir, tid, cur) in txns {
            sqlx::query("INSERT INTO canonical_transactions(id,tenant_id,source_id,external_ref,value_date,posted_at,amount_minor,currency,direction,description) VALUES ($1,$2,$3,$4,$5::date,($5||'T09:00:00Z')::timestamptz,$6,$7,$8,$9)")
                .bind(id).bind(tid).bind(src).bind(eref).bind(date).bind(amt).bind(cur).bind(dir).bind(format!("Txn {id}"))
                .execute(&mut *tx).await?;
        }

        // Run definitions: (run_id, tenant, name, src_a, src_b, started_at, from_date, to_date).
        // Disjoint date windows so case-pending (txn-brk001) is produced exactly once (run-acme-006).
        let runs = [
            (
                "run-acme-001",
                "tenant-acme",
                "Daily Bank-GL 2026-05-05",
                "src-acme-bank",
                "src-acme-ledger",
                "2026-05-05T18:00:00Z",
                "2026-05-01",
                "2026-05-09",
            ),
            (
                "run-acme-006",
                "tenant-acme",
                "Daily Bank-GL 2026-05-23",
                "src-acme-bank",
                "src-acme-ledger",
                "2026-05-23T18:00:00Z",
                "2026-05-10",
                "2026-05-31",
            ),
            (
                "run-globex-001",
                "tenant-globex",
                "Globex Daily 2026-05-10",
                "src-globex-bank",
                "src-globex-ledger",
                "2026-05-10T19:00:00Z",
                "2026-05-01",
                "2026-05-31",
            ),
        ];
        let cfg = MatchConfig::v1();
        for (run_id, tid, name, sa, sb, started, from, to) in runs {
            let a = self.load_window(&mut tx, tid, sa, from, to).await?;
            let b = self.load_window(&mut tx, tid, sb, from, to).await?;
            let result = reconcile(&a, &b, &cfg);
            let stats = serde_json::to_value(&result.stats)?;
            sqlx::query("INSERT INTO reconciliation_runs(id,tenant_id,name,source_a_id,source_b_id,status,started_at,completed_at,config_version,stats) VALUES ($1,$2,$3,$4,$5,'completed',$6::timestamptz,$6::timestamptz,$7,$8)")
                .bind(run_id).bind(tid).bind(name).bind(sa).bind(sb).bind(started).bind(&cfg.version).bind(&stats).execute(&mut *tx).await?;

            for (i, d) in result.decisions.iter().enumerate() {
                let type_str = serde_json::to_value(d.match_type)?
                    .as_str()
                    .unwrap()
                    .to_string();
                sqlx::query("INSERT INTO match_decisions(id,tenant_id,run_id,type,txn_ids,score,config_version) VALUES ($1,$2,$3,$4,$5,$6,$7)")
                    .bind(format!("md-{run_id}-{i}")).bind(tid).bind(run_id).bind(type_str).bind(&d.txn_ids).bind(d.score).bind(&cfg.version).execute(&mut *tx).await?;
            }

            for (i, bd) in result.breaks.iter().enumerate() {
                let is_pending = bd.txn_ids.iter().any(|t| t == "txn-brk001");
                let case_id = if is_pending {
                    "case-pending".to_string()
                } else {
                    format!("case-{run_id}-{i}")
                };
                let break_id = if is_pending {
                    "break-pending".to_string()
                } else {
                    format!("break-{run_id}-{i}")
                };
                let type_str = serde_json::to_value(bd.break_type)?
                    .as_str()
                    .unwrap()
                    .to_string();
                let opened: &str = if is_pending {
                    "2026-05-15T10:30:00Z"
                } else {
                    started
                };
                let (status, assignee): (&str, Option<&str>) = if is_pending {
                    ("pending_approval", Some("user-mia"))
                } else {
                    ("open", None)
                };

                sqlx::query("INSERT INTO cases(id,tenant_id,break_id,assignee_id,status) VALUES ($1,$2,$3,$4,$5)")
                    .bind(&case_id).bind(tid).bind(&break_id).bind(assignee).bind(status).execute(&mut *tx).await?;
                sqlx::query("INSERT INTO breaks(id,tenant_id,run_id,case_id,type,status,value_minor,currency,assignee_id,txn_ids,opened_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11::timestamptz)")
                    .bind(&break_id).bind(tid).bind(run_id).bind(&case_id).bind(type_str).bind(status).bind(bd.value_minor).bind(&bd.currency).bind(assignee).bind(&bd.txn_ids).bind(opened).execute(&mut *tx).await?;

                if is_pending {
                    let evs = [
                        (
                            1i32,
                            "assignment",
                            "user-ada",
                            "2026-05-15T11:00:00Z",
                            serde_json::json!({"assigneeId":"user-mia"}),
                        ),
                        (
                            2,
                            "comment",
                            "user-mia",
                            "2026-05-16T09:00:00Z",
                            serde_json::json!({"text":"Reviewed; looks like a write-off candidate."}),
                        ),
                        (
                            3,
                            "write_off_proposed",
                            "user-mia",
                            "2026-05-16T09:30:00Z",
                            serde_json::json!({"reason":"Counterparty confirmed unmatched; below materiality."}),
                        ),
                        (
                            4,
                            "approval_requested",
                            "user-mia",
                            "2026-05-16T09:35:00Z",
                            serde_json::json!({"resolution":"write_off"}),
                        ),
                    ];
                    for (seq, kind, actor, at, payload) in evs {
                        sqlx::query("INSERT INTO case_events(id,tenant_id,case_id,seq,kind,actor_id,at,payload) VALUES ($1,$2,$3,$4,$5,$6,$7::timestamptz,$8)")
                            .bind(format!("evt-pending-{seq}")).bind(tid).bind(&case_id).bind(seq).bind(kind).bind(actor).bind(at).bind(payload).execute(&mut *tx).await?;
                    }
                }
            }
        }

        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn load_window(
        &self,
        tx: &mut sqlx::PgConnection,
        tenant_id: &str,
        source_id: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<CanonicalTransaction>, StoreError> {
        let rows: Vec<crate::rows::TxnRow> = sqlx::query_as("SELECT * FROM canonical_transactions WHERE tenant_id = $1 AND source_id = $2 AND value_date BETWEEN $3::date AND $4::date ORDER BY id")
            .bind(tenant_id).bind(source_id).bind(from).bind(to).fetch_all(&mut *tx).await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }
}

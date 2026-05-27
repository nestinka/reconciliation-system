-- Phase 7 — Add counterparty BIC + account to canonical_transactions.
-- Purely additive: two nullable columns + one shape CHECK on BIC.
-- Postgres adds nullable columns without table rewrite (metadata-only).

ALTER TABLE canonical_transactions
    ADD COLUMN counterparty_bic     TEXT NULL,
    ADD COLUMN counterparty_account TEXT NULL;

ALTER TABLE canonical_transactions
    ADD CONSTRAINT chk_counterparty_bic_shape
    CHECK (
        counterparty_bic IS NULL
        OR counterparty_bic ~ '^[A-Z0-9]{8}([A-Z0-9]{3})?$'
    );

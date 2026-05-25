-- Prevent the same transaction being ingested twice into one source.
--
-- DEPLOY PRECONDITION: this ALTER fails if the table already contains duplicate
-- (source_id, external_ref) rows. On a populated production DB, run a dedup check
-- BEFORE applying this migration, e.g.:
--   SELECT source_id, external_ref, count(*)
--   FROM canonical_transactions GROUP BY 1,2 HAVING count(*) > 1;
-- and remove/repair any duplicates first. A fresh or reseeded DB applies cleanly.
ALTER TABLE canonical_transactions
  ADD CONSTRAINT uq_txn_source_ref UNIQUE (source_id, external_ref);

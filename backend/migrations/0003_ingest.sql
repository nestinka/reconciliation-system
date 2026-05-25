-- Prevent the same transaction being ingested twice into one source.
ALTER TABLE canonical_transactions
  ADD CONSTRAINT uq_txn_source_ref UNIQUE (source_id, external_ref);

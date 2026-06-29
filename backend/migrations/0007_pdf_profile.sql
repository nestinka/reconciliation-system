-- Phase 8: per-source PDF bank-statement profile. Additive; no CHECK constraint
-- because valid profile names are owned by the recon-ingest registry and
-- validated at the API (avoids a migration per new profile).
ALTER TABLE sources ADD COLUMN pdf_profile TEXT NULL;

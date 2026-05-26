-- Phase 6: bank-format dialect annotation on sources.
-- Only meaningful for MT940 sources today. NULL for all other formats.
ALTER TABLE sources ADD COLUMN format_dialect TEXT NULL;
ALTER TABLE sources ADD CONSTRAINT chk_format_dialect
  CHECK (format_dialect IS NULL OR format_dialect IN ('generic', 'subfielded'));

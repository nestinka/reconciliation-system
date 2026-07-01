-- Phase 9: soft-delete / archive sources. Additive with a default (safe on
-- populated tables); mirrors users.disabled.
ALTER TABLE sources ADD COLUMN disabled BOOLEAN NOT NULL DEFAULT false;

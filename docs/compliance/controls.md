# Compliance controls — audit-event mapping

This document maps ISO 27001 / SOC 2 / FCA control items to the audit-event kinds
that demonstrate them. The same mapping is exposed programmatically by
`GET /api/audit/controls` and rendered in the Controls admin screen.

## ISO 27001

### A.9.2.1 — User registration and de-registration
**Evidence:** `admin.user.created`, `admin.user.disabled`, `admin.user.enabled`, `admin.user.removed`.
Filter the audit log to these kinds to enumerate all on/off-boarding events.

### A.9.2.3 — Management of privileged access rights
**Evidence:** `admin.user.role_changed`. Every role transition (operator ↔ approver ↔ admin) is recorded with `from`/`to`.

### A.9.4.2 — Secure log-on procedures
**Evidence:** `auth.login.success`, `auth.login.failure`, `auth.lockout`. Brute-force protection and lockout events are visible per account.

### A.9.4.3 — Password management system
**Evidence:** `auth.password.changed`, `auth.password.reset_requested`, `auth.password.reset_completed`.

### A.12.4.1 — Event logging
**Evidence:** `system.anchor.created`, `data.ingest.completed`, `data.run.created`, `data.source.created`. The audit chain itself is hash-anchored periodically.

## SOC 2

### CC6.1 — Logical access security software, infrastructure, and architectures
**Evidence:** `auth.login.success`, `auth.login.failure`, `auth.lockout`, `auth.tenant.switched`, `auth.refresh.reused`.

### CC6.2 — Prior to issuing system credentials and granting access
**Evidence:** `admin.user.created`, `admin.user.role_changed`.

### CC6.3 — Authorize, modify, or remove access to data, software, functions
**Evidence:** `admin.user.role_changed`, `admin.user.disabled`, `admin.user.enabled`, `admin.user.removed`.

### CC7.2 — Monitors system components and operation
**Evidence:** `auth.refresh.reused` (theft detection), `auth.lockout` (brute-force detection), `system.anchor.created` (audit-chain integrity).

## FCA

### SYSC 9.1 — Record keeping
**Evidence:** `data.ingest.completed`, `data.run.created`, `case.assigned`, `case.event_appended`. Every reconciliation action that creates or modifies records is captured.

### SYSC 4.1.10 — Four-eyes / segregation of duties
**Evidence:** `case.event_appended` (carries the `event_kind` field — auditor filters to `approval_requested` / `approved` to verify maker-checker separation).

## Chain integrity

The audit log is per-tenant hash-chained (SHA-256), with `prev_hash` and `hash`
on every row. A periodic (hourly by default) `system.anchor.created` ties every
tenant's current head into a global anchor chain, providing wholesale-deletion
detection. The admin **Audit Log** screen exposes a **Verify chain** action that
walks any time range and reports the first broken entry (if any).

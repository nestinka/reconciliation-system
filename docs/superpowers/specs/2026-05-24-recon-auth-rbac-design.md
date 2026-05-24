# Recon Auth & RBAC Slice — Design

**Date:** 2026-05-24
**Status:** Approved (brainstorming) → ready for implementation plan
**Depends on:** UI slice (`2026-05-23-recon-ui-slice-design.md`), Backend slice (`2026-05-24-recon-backend-slice-design.md`)

## Goal

Make identity real. Replace the trusted-header auth seam (`X-User-Id` / `X-Tenant-Id`)
with email + password login that issues self-signed JWTs, multi-tenant membership with
per-tenant roles, server-enforced RBAC, plus user management, self-service password
change, password reset by email, and brute-force protection.

## Decisions (locked during brainstorming)

1. **Mechanism:** self-hosted email + password; we hash credentials (argon2id) and issue
   our own JWTs. No external IdP.
2. **Token transport:** access + refresh hybrid. Short-lived access JWT held in memory on
   the client, sent as `Authorization: Bearer`. Refresh token in an httpOnly cookie, with
   server-side rotation and revocation.
3. **Scope:** core (login / refresh / logout, RBAC enforcement, seeded users, login page)
   **plus** brute-force protection, self-service password change, admin user management,
   and password reset by email.
4. **User ↔ tenant:** multi-tenant membership. Users are global identities with per-tenant
   memberships, each carrying its own role. The top-bar switcher re-scopes the active token.

---

## Section 1 — Architecture & code organization

Auth logic lives in a dedicated, IO-light **`recon-auth`** crate so the security-critical
primitives are unit/property-testable in isolation and `recon-api` stays thin. Layering
matches the existing inward-pointing dependency graph.

**Crate layout:**

- `recon-domain` — unchanged except the `User`/role types evolve for membership (see §2).
- **`recon-auth`** (new) — depends only on `recon-domain`:
  - `password` — argon2id hash / verify (constant-time).
  - `token` — access-JWT claims via `jsonwebtoken`, HS256 with `JWT_SECRET`.
  - `refresh` — opaque 256-bit token generation + the hashed-at-rest model.
  - `rbac` — role → permission matrix + guard helpers.
  - `lockout` — failed-attempt / lockout policy (pure state machine).
- **`recon-mail`** (new, small) — a `Mailer` trait with an SMTP impl (`lettre`) and a
  log-only fallback. Dev uses **Mailpit** (containerized SMTP catcher + web UI) so reset
  links work locally; prod points `SMTP_*` at a real server.
- `recon-store` — new tables/queries (credentials, memberships, refresh tokens, reset
  tokens) + lockout state.
- `recon-api` — auth routes, admin user routes, and the existing `AuthContext` extractor
  switched from trusted headers to **validating the Bearer access token**. The seam was
  built for this, so endpoint handlers barely change.
- `web` — `AuthProvider` (in-memory access token + silent refresh), login / forgot / reset
  pages, route guards, Bearer + 401-refresh in `HttpApiClient`, tenant switcher re-scoping,
  admin Users screen, password-change UI.

---

## Section 2 — Data model

Structural change: **`users.tenant_id` moves out into a `memberships` table** — a user is
now a global identity with per-tenant roles. Delivered as migration `0002_auth.sql`
(additive + backfill). Existing immutable / insert-only tables are unchanged; new **mutable**
tables hold credential/session state only.

**Changed table:**

- `users` — drop `tenant_id` and `role`; add `email` (citext, unique) and
  `disabled boolean not null default false`. Identity becomes global. Backfill keeps
  `id`/`name`, synthesizes `email` from seed data, and migrates `tenant_id`/`role` into
  `memberships`.

**New tables:**

- `memberships` — `(user_id, tenant_id, role)`, PK `(user_id, tenant_id)`, FKs to
  users/tenants. Role is **per-tenant** (`operator | approver | admin`).
- `user_credentials` (mutable) — `user_id PK/FK`, `password_hash text`,
  `password_updated_at timestamptz`, `failed_attempts int not null default 0`,
  `locked_until timestamptz null`. Separate from immutable `users` so password
  changes/lockout do not violate immutability.
- `refresh_tokens` — `id`, `user_id`, `tenant_id` (active scope), `token_hash`
  (sha-256 of the opaque token), `expires_at`, `revoked_at null`, `rotated_from null`,
  `created_at`. Enables rotation + server-side revocation + reuse detection.
- `password_reset_tokens` — `id`, `user_id`, `token_hash`, `expires_at`, `used_at null`.
  Single-use, short TTL.

**Identity-flow implications:**

- Tenant is no longer a request header — it is a claim in the access token, set at login and
  changed via switch-tenant. `AuthContext` becomes `{ user_id, tenant_id, role }`, all
  derived from the validated JWT.
- Brute force = per-account (`failed_attempts` / `locked_until` in `user_credentials`) plus
  a per-IP login rate limiter (in-memory token bucket; fine for single-instance, noted as a
  scale-out follow-up).
- Seed updates: Mia → `operator` in tenant-acme, Theo → `approver` in tenant-acme, plus an
  `admin` user; each seeded user gets a known dev password (argon2id hash) and ≥1
  membership. One user has memberships in **both** tenants so the switcher is demonstrable.

---

## Section 3 — API surface & token lifetimes

**Lifetimes:** access JWT **15 min** (HS256, in-memory on client); refresh token **30 days**
(opaque, rotated on every use, revocable). Refresh cookie: `httpOnly`, `Secure` (prod),
`SameSite=Strict`, `Path=/auth`.

**Auth endpoints** (public unless noted):

| Method · Path | Body → Result | Notes |
|---|---|---|
| `POST /auth/login` | `{email, password}` → `{accessToken, user, activeTenant, memberships[]}` + Set-Cookie refresh | Generic 401 on bad creds; 429 on lockout/rate-limit (no enumeration) |
| `POST /auth/refresh` | cookie → `{accessToken}` + rotated cookie | 401 if missing/expired/revoked → client bounces to login |
| `POST /auth/logout` | cookie → `204` | Revokes refresh row, clears cookie |
| `POST /auth/switch-tenant` *(auth)* | `{tenantId}` → `{accessToken}` + rotated cookie | 403 if no membership for that tenant |
| `POST /auth/password` *(auth)* | `{currentPassword, newPassword}` → `204` | Verifies current; revokes other refresh tokens |
| `POST /auth/forgot` | `{email}` → `202` always | Emails reset link via Mailpit if user exists |
| `POST /auth/reset` | `{token, newPassword}` → `204` | Single-use token; revokes all refresh tokens |

**Admin user management** (auth + `admin` role, scoped to the caller's active tenant):

| Method · Path | Purpose |
|---|---|
| `GET /api/users` | List users + their role in this tenant (replaces today's unsecured list) |
| `POST /api/users` | Create user (+ membership) with role and a temp password, or attach an existing global user to this tenant |
| `PATCH /api/users/:id` | Change this tenant's membership role, or disable/enable |
| `DELETE /api/users/:id` | Soft-remove the membership in this tenant (global user preserved) |

**Existing endpoints** keep their shapes — they read tenant/role/user from the validated
token instead of headers. RBAC guard per route (approval requires `approver`/`admin`;
`can_approve` still enforces four-eyes: not your own proposal). Error mapping extends:
`401` (missing/invalid token), `403` (role/tenant), `429` (lockout/rate-limit).

---

## Section 4 — Frontend

**`AuthProvider`** (new top-level context) — single source of truth:

- Access token **in memory only** (never localStorage), plus `user`, `memberships`,
  `activeTenant`, derived `role`.
- **Bootstrap on load:** call `POST /auth/refresh` (cookie rides along) — success restores
  the session silently; failure → unauthenticated. This keeps a reload logged in without a
  readable token.
- **Silent refresh:** timer refreshes ~1 min before the 15-min access token expires.
- Exposes `login`, `logout`, `switchTenant`, `changePassword`.

**`HttpApiClient` changes:**

- Adds `Authorization: Bearer <accessToken>`.
- On a **401**, attempts one `/auth/refresh` then retries the original request; if refresh
  fails, logs out → redirect to `/login`.
- `/auth/*` calls use `credentials: "include"` so the refresh cookie is sent.

**Routing & pages:**

- **Route guard:** unauthenticated users redirected to `/login` (public allowlist:
  `/login`, `/forgot`, `/reset`). Authenticated users hitting `/login` bounce to `/dashboard`.
- **`/login`** — email + password (react-hook-form + zod), inline error on 401/429.
- **`/forgot`** — email field → always "if that account exists, we've sent a link."
- **`/reset?token=…`** — new-password form → success redirects to `/login`.
- **Admin "Users" screen** (`/users`, `admin` only) — table of users + tenant role;
  create/edit-role/disable using existing list/table + dialog components.
- **Password change** — small dialog from the user menu.

**Top-bar changes:**

- **Tenant switcher** lists the user's `memberships`; selecting one calls `switchTenant` →
  new access token → React Query cache invalidated → data refetched under the new tenant.
- **`UserMenu`** shows the real logged-in user with a **Logout** action; the old localStorage
  user-switcher (`recon:currentUserId`) and `recon:activeTenantId` are **removed**.

---

## Section 5 — Security specifics & testing

**Security:**

- **Password hashing:** argon2id, sane params; constant-time verify; passwords never logged.
- **JWT:** HS256 signed with `JWT_SECRET` (required in prod; dev falls back to a fixed dev
  secret with a loud warning). Claims: `sub`, `tid`, `role`, `jti`, `iat`, `exp`,
  `typ:"access"`. Validated on every request by the `AuthContext` extractor.
- **Refresh tokens:** opaque 256-bit random, stored only as a SHA-256 hash; **rotation** on
  every refresh (old row revoked, `rotated_from` chained); reuse of a revoked token revokes
  the whole chain (theft detection). Cookie `httpOnly`/`Secure`/`SameSite=Strict`/`Path=/auth`.
- **Brute force:** per-account `failed_attempts` → `locked_until` (exponential backoff);
  per-IP in-memory rate limit on `/auth/login`. Both return `429` with a generic message.
- **No enumeration:** login and forgot-password give identical responses regardless of
  whether the account exists.
- **Tenant isolation:** every query stays scoped to the token's `tid`; switch-tenant
  verifies membership before re-issuing.
- **CSRF:** the only cookie is the refresh token, confined to `/auth` with `SameSite=Strict`;
  all state-changing API calls use the `Bearer` header (not the cookie), so they are not
  CSRF-able.

**Testing (TDD throughout):**

- **`recon-auth`** — unit + property: hash/verify round-trip & rejection,
  token encode/decode/expiry/tamper, RBAC matrix (every role × permission), lockout state
  machine.
- **`recon-store`** (`#[sqlx::test]`) — credentials CRUD, membership lookups, refresh
  rotation/revocation/reuse-detection, reset-token single-use, lockout counters.
- **`recon-api`** (integration) — full flows: login → refresh → logout; switch-tenant;
  password change; forgot → reset; RBAC `403`s; four-eyes enforced via **token** role (the
  old impersonation vector is gone); `429` on lockout/rate-limit.
- **`recon-mail`** — Mailer trait with a capturing test double; SMTP impl smoke-tested
  against Mailpit.
- **Frontend** (vitest) — AuthProvider bootstrap/refresh/logout, 401-retry interceptor,
  route guard, login/forgot/reset forms.
- **E2E** (Playwright, live stack) — login as Mia (operator) → approve disabled; logout →
  login as Theo (approver) → approve succeeds; tenant switch re-scopes data; admin creates a
  user; password reset reading the link from **Mailpit's API**; reload stays logged in via
  the refresh cookie.

---

## Out of scope (candidate later slices)

- External SSO / OIDC (the password mechanism is the seam).
- Distributed/multi-instance rate limiting (current limiter is in-memory, single-instance).
- Email infrastructure beyond dev Mailpit + SMTP config (deliverability, templates, DKIM).
- Org/tenant self-signup and billing.

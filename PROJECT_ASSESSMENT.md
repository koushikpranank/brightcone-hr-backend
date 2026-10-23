# Brightcone People Plus — Repository and Production Assessment

**Audit date:** 15 October 2026
**Scope:** `repos/brightcone-hr-frontend` and `repos/brightcone-hr-backend` in this workspace. This document describes the code currently present, not the full product shown in the design mockups. It is an engineering review, not a penetration test or a production certification.

## Executive assessment

**Production status: not ready for production HR or payroll data.** The repositories provide a useful authentication, RBAC, audit, and dashboard foundation. The advertised People Plus suite is not yet a complete HR system: core business modules such as payroll, leave, expenses, recruitment, files, and approvals do not have active durable API implementations. Several UI modules are explicitly “Coming soon.” Previously committed status documents overstated completion; use this audit as the current source of truth.

The most important work completed during this audit was to remove unsafe/inaccurate defaults and prevent prototype routes from appearing live. The approval and file handlers present in the backend are in-memory prototypes. They were removed from HTTP route registration because restart loss and cross-instance inconsistencies make them unsafe for real records. Approval routes also lacked adequate data scoping. These features need durable schemas and authorization before being re-enabled.

## What is implemented today

### Frontend

The frontend is a React/TypeScript application organized into API clients, authentication, shared components, feature areas, People Plus shell, routes, and styles. Its dashboard and navigation reflect the supplied dark Brightcone design. Authentication uses an in-memory access token and an HttpOnly refresh cookie; the API client attempts shared silent refresh after a 401. The app presents unavailable suite modules as coming soon rather than inventing records.

Current UI routes and supported business operations should be read from `src/routes` and `src/features` alongside the actual API client. A visual module or card is not evidence of a working backend feature. The Careers copy and authentication E2E expectations were corrected during this audit to remove fake role/pay claims and align with the real greeting/sign-out controls.

### Backend

The backend is Rust with Loco, Axum, and PostgreSQL. Implemented foundations include login/session rotation, password recovery, current-user details, RBAC, account administration, audit logs, health check, queued email infrastructure, and refresh-token cleanup. The current API contract is published in `openapi.json`; the live route list below reflects the registered handlers after this audit.

## Live API inventory

All paths are relative to the API origin. Authenticated endpoints use `Authorization: Bearer <access-token>`. Cookie-authenticated mutation endpoints require the configured CSRF header/token pair in addition to the refresh cookie. Exact JSON field validation and response shapes are defined by the Rust DTOs and OpenAPI document.

| Method and path | Access | Purpose |
|---|---|---|
| `GET /api/health` | Public | Health and database connectivity check. |
| `POST /api/auth/login` | Public | Verify email/password; issue short-lived access JWT and refresh cookie. |
| `POST /api/auth/refresh` | Refresh cookie + CSRF | Rotate refresh token; detect reuse and revoke its family. |
| `POST /api/auth/logout?all={bool}` | Refresh cookie + CSRF | Revoke current session or all user sessions. |
| `POST /api/auth/forgot` | Public | Request password recovery; response text is uniform. |
| `POST /api/auth/reset` | Public reset token | Set a new password and revoke sessions. |
| `GET /api/me` | Authenticated | Current profile, role, and effective permissions. |
| `PUT /api/me/password` | Authenticated | Change the current account password. |
| `GET /api/admin/users` | `users:manage` | Paginated user listing. |
| `POST /api/admin/users` | `users:manage` | Create an account; response includes a one-time temporary password. Protect that response and force a password change. |
| `POST /api/admin/users/{pid}/unlock` | `users:manage` | Unlock an account. |
| `POST /api/admin/users/{pid}/disable` | `users:manage` | Disable an account and revoke sessions. |
| `PUT /api/admin/users/{pid}/role` | `users:manage` | Change account role. |
| `POST /api/admin/users/{pid}/reset-password` | `users:manage` | Issue a temporary password and revoke sessions. |
| `GET /api/audit` | `audit:read` | Paginated/filterable audit events. |

There are **no active HTTP endpoints** for `/api/files` or `/api/approvals` after this audit. The old README described endpoints that were not in OpenAPI and were backed by in-process memory; those claims were removed from its API summary. The UI should keep these features unavailable until durable handlers are implemented.

### Authentication and session notes

- Access token lifetime is configured as a short-lived JWT; refresh token values are random, stored as hashes, rotated, and held in an HttpOnly cookie.
- The refresh cookie is scoped to the auth API path and configured for SameSite Strict; Secure is enabled in production configuration.
- Password reset tokens are hashed before persistence. Logout/disable/reset paths revoke sessions.
- Login has account lockout behavior. The current rate limiter is process-local, so it does not provide a consistent limit across multiple API replicas.
- Email enumeration response wording is uniform, but the synchronous lookup/send path may still have observable timing differences. Move delivery to a queue and evaluate timing behavior before production.

## Database schema currently present

The product schema is declared in `src/database/schema/manifest.rs` and synchronized additively at startup with Diesel. Existing databases may retain the legacy `citext` email column; queries cast it to text. The Loco queue may create its own framework-managed queue tables; those are not product domain entities.

| Table | Main fields / constraints | Purpose |
|---|---|---|
| `users` | `id` PK, `pid` UUID, normalized unique `email`, `password`, `name`, timestamps, `role`, `status`, `failed_logins`, `locked_at`, `last_login_at`, `must_change_password`, `password_changed_at`, `api_key`; role/status checks | Identity, credentials, account state and RBAC role. |
| `refresh_tokens` | `id` PK, `user_id` FK → users (cascade), unique `token_hash`, `family_id`, persistent flag, expiry/use/revoke timestamps, IP, creation timestamp; indexes on user/family/expiry | Hashed refresh-session state and replay detection. |
| `password_resets` | `id` PK, `user_id` FK → users (cascade), unique `token_hash`, expiry/use/create timestamps; user index | One-time password recovery tokens. |
| `audit_logs` | `id` PK, nullable actor id/email, action, target type/id, summary, IP, request id, timestamp; append-only trigger/privileges | Security and administrative event trail. No actor FK by design so audit history survives user deletion. |

There are **no durable product tables** for employees beyond login users, org/team structure, leave, payroll, expenses, recruitment, documents, approvals, benefits, schedules, learning, or social feed. The startup sync is deliberately additive: it creates missing tables and columns, indexes, and constraints; it does not drop or rewrite existing data. Review the manifest for every schema change. Before adding HR domains, define ownership/tenant boundaries, retention, indexes, audit events, and a safe forward/backfill plan; code-driven sync does not replace data backfills or operational rollback planning.

## Jobs and background work

- Email delivery is wired through the Loco mailer/queue infrastructure; local development uses Mailpit. Verify the worker is running and delivery/retry/dead-letter behavior is observable in the deployed environment.
- `TokenCleanupWorker` is intended to remove expired refresh-token records (including old records past expiry); confirm its scheduler is enabled in the production process configuration.
- There are no verified scheduled payroll, leave, or HR business jobs in the current persistent domain model.

## Architecture and recommended structure

### Current layout (simplified)

```text
Brightcon Internship/
├── PROJECT_ASSESSMENT.md
├── repos/
│   ├── brightcone-hr-frontend/
│   │   └── src/{api,auth,components,features,people-plus,routes,styles}/
│   └── brightcone-hr-backend/
│       ├── src/{controllers,dtos,mailers,models,tasks,views,workers}/
│       ├── src/database/{schema.rs,schema/manifest.rs}
│       ├── config/
│       ├── tests/
│       └── .github/workflows/
├── Brightcone People Plus.html
├── brightcone_people_plus_design_spec.md
└── newCe.xlsx
```

### Target structure as product domains grow

Keep the existing repositories; migrate incrementally rather than creating a large all-at-once rewrite.

```text
frontend/src/
  app/                 # bootstrap, providers, route composition
  shared/{api,ui,auth,utils}/
  features/{people,recruit,payroll,expense,sign,connect,learn,shift}/
  shell/               # global app navigation and suite shell
backend/src/
  app/                  # config, middleware, error and request context
  shared/{auth,rbac,audit,db,queue,storage}/
  domains/<domain>/     # controller, DTO, service, model/repository, policy
  jobs/                 # named jobs, schedules and retry policy
  main.rs
backend/src/database/  # Diesel pool and additive startup schema manifest
```

Keep controllers thin (parse/authenticate/validate/serialize), put business rules in services, database access behind repositories/models, and authorization in explicit policies. Each domain should own its DTOs, schema declarations, tests, and API contract. Shared code should remain small and dependency direction should flow from domain code to shared interfaces, not between unrelated feature modules.

## Running locally

Commands below assume PowerShell from the workspace root and a native PostgreSQL installation. Never use production data or production credentials in the demo seed workflow.

1. Start the local PostgreSQL service:

   ```powershell
   cd repos/brightcone-hr-backend
   Get-Service *postgres*
   Start-Service postgresql-x64-16
   ```

2. Create the backend `.env` from `.env.example`, run `psql -U postgres -f scripts/native-postgres-setup.sql`, and generate a unique random `JWT_SECRET` (for example, `openssl rand -base64 48`). Keep `.env` untracked. Set separate `DATABASE_URL` (runtime role), `SCHEMA_SYNC_DATABASE_URL` (DDL role), and `QUEUE_URL` values. Do not reuse development credentials outside an isolated local database. For the frontend, `.env.example` documents `VITE_PROXY_TARGET=http://localhost:5150` and `VITE_API_URL=/api`; copy it to the Vite env file used by the project if overrides are needed.

3. Start the API. It syncs schema before serving requests; seed demo users only in disposable local development:

   ```powershell
   cargo run -- start --server-and-worker
   # In another terminal, after schema sync completes:
   cargo run -- task seed_demo
   ```

4. The API listens on `http://localhost:5150` by default.

5. In another terminal, install and run the frontend:

   ```powershell
   cd repos/brightcone-hr-frontend
   npm ci
   npm run dev
   ```

   Set the frontend API base URL using the documented frontend environment variable and point it at the local API. Open the Vite URL printed by the command.

6. Useful checks from each repo:

   ```powershell
   # frontend (from repos/brightcone-hr-frontend)
   npm run lint
   npm test
   npm run build
   # backend (from repos/brightcone-hr-backend)
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-features
   ```

Some scripts vary by the installed package manager/test runner; check `package.json`. The seed is for local-only use. Do not expose a local SMTP catcher's ports publicly.

## Bugs and fixes made during this audit

| Finding | Impact | Change / status |
|---|---|---|
| Files and approval handlers used global in-memory maps and were mounted as if usable | Data loss on restart, inconsistent replicas; approval records were not safely scoped | Removed the HTTP routes. Implement durable DB-backed, tenant/team-scoped feature before release. |
| Approval rejection omitted the self-approval guard used by approval | Submitter could reject their own request | Added the same self-decision prohibition to rejection and a regression test. |
| CORS was fixed to the Vite localhost origin | Deployed frontend origin could not call API; production environment coupling | CORS now derives origin from configured `frontend_url` and rejects invalid configuration instead of silently using localhost. |
| Development config had a committed JWT secret fallback | Anyone with repository access could mint development tokens | Removed fallback; require locally generated secret. `.env.example` is blank for this secret. |
| Seed credentials were present in project documentation | Unintended account access if copied/reused | Sanitized the tracked docs. **Git history may still contain old values**; rotate any credentials ever used/shared and rewrite history if required by your incident policy. |
| CI database URL YAML key was malformed | CI configuration could parse incorrectly | Corrected the mapping syntax. |
| Careers UI showed unsupported role and compensation facts | Misleading product information | Replaced with neutral copy until backed by real API data. |
| Auth E2E expected an obsolete greeting and sign-out control | Test no longer matched current interface | Updated selectors/expectations to current UI. |
| API CSP allowed inline script/style sources | Larger injection surface for API responses | Replaced with a restrictive API-only CSP. |

These changes do not complete the suite backend or establish a security certification. The user-provided project/design files describe desired behavior; they are requirements/reference material, not proof that a feature exists.

## Production readiness gaps and priorities

### Must resolve before production HR records

1. **Implement the product domains.** Design schemas and APIs for employee/org/team, leave, payroll, expenses, recruitment, approvals, and document metadata. Every read/write needs role and row/team/tenant authorization. Keep the current “Coming soon” state until endpoints are durable and tested.
2. **Define tenant and authorization boundaries.** The current user model has global roles but no tenant/org membership model. Add tenant-scoped policies before serving multiple customers or teams.
3. **Move rate limiting to shared infrastructure.** Process-local counters are bypassed by round-robin replicas/restarts. Use Redis or a gateway limiter with trusted client IP extraction and explicit key policies.
4. **Harden password recovery against timing enumeration.** Queue mail uniformly and measure response-time distributions. Keep responses generic; avoid waiting for SMTP in the request path.
5. **Remove reusable demo credentials from build/deploy pathways.** Rotate any credentials used outside disposable local data; use a secrets manager and non-public seed accounts. Secret-scan the repository and history in CI.
6. **Production deployment and operations.** Use a managed PostgreSQL service, a native Rust build pipeline, TLS termination, encrypted backups, restore drills, schema-sync deployment ordering, readiness/liveness probes, metrics, centralized logs, alerting, and incident procedures.

### Strongly recommended

- Verify account lockout is paired with IP/device limits so a malicious caller cannot easily lock a victim out; document support/admin recovery.
- Expand compromised/common-password screening (current list is small) and benchmark password hashing parameters. Add MFA for administrators and step-up authentication for sensitive changes.
- Review HSTS `includeSubDomains`/`preload` against actual domain ownership and TLS coverage; emit only when deployment guarantees HTTPS. Ensure reverse-proxy trust is restricted to controlled proxy hops.
- Use object storage with private buckets, short-lived signed downloads, malware scanning, encryption, retention, size quotas, and DB-backed file metadata before enabling uploads. Local disk is not horizontally scalable.
- Add API contract/authorization tests for every role and object scope; concurrency tests for refresh replay and approval decisions; startup schema sync tested against clean and upgrade databases; browser E2E against a real local API.
- Add pagination limits, request size limits, timeouts, database pool/queue capacity tuning, graceful shutdown, and safe error redaction. Maintain dependency and container vulnerability scanning.
- Document recovery, user provisioning, password reset, audit retention, privacy/legal retention, and data export/deletion procedures.

## Verification performed and limitations

- Repository source, routes/OpenAPI, schema sync, configuration, docs, and frontend structure were inspected. Existing working-tree changes were preserved; repos already contained substantial uncommitted work.
- Frontend `npm run lint`, `npm test` (2 files / 16 tests), and `npm run build` pass in this environment.
- Prior verification status; rerun backend checks and database integration tests after applying the current schema-sync changes.
- Playwright E2E did not complete: the default results directory was a read-only/reparse directory, and a retry with an isolated output directory stalled before producing test results. E2E verification remains open.
- No live production environment, deployed database, SMTP provider, cloud storage, load test, external penetration test, or browser screenshot comparison was available for this assessment.

## Decision

Use the current code for local development and controlled evaluation with disposable data. **Do not call it production-ready or store employee/payroll documents in it yet.** The authentication and platform foundation is promising, but domain persistence, tenant isolation, distributed abuse controls, deployment operations, and a clean backend verification run remain release gates.

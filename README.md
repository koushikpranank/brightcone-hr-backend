# Brightcone People API

Rust and Loco API with PostgreSQL. The frontend is in `../brightcone-hr-frontend`.

## Native setup

Install Rust with rustup, PostgreSQL 16, and optionally a local SMTP catcher. Start PostgreSQL using your operating system's service manager. Create the development roles and databases from an administrative `psql` session:

```powershell
Copy-Item .env.example .env
psql -U postgres -f scripts/native-postgres-setup.sql
cargo run --bin brightcone_hr_backend-cli -- start
```

On Linux/macOS, replace `Copy-Item` with `cp .env.example .env`. Then run `psql -U postgres -f scripts/native-postgres-setup.sql` and `cargo run --bin brightcone_hr_backend-cli -- start`.

On Windows, find the installed service name with `Get-Service *postgres*`, then start it with `Start-Service <service-name>`. On Linux, use `sudo systemctl start postgresql`.

The API listens on `http://localhost:5150`; verify it with `curl http://localhost:5150/api/health`. Schema synchronization runs before Loco assembles the HTTP router. `DATABASE_URL` is for the restricted application account. `SCHEMA_SYNC_DATABASE_URL` is a separate schema-owner account used during startup. Never use the sample credentials outside local development.

## Schema synchronization

The `SchemaSyncInitializer` connects to PostgreSQL through SeaORM with `SCHEMA_SYNC_DATABASE_URL`. It takes transaction-scoped advisory lock `88421038`, inspects `information_schema`, then creates missing tables, adds compatible columns, and creates idempotent indexes from the Rust schema definitions. It never drops tables or columns. Existing mapped columns with incompatible types or nullability stop startup for DBA review. The transaction completes before Loco serves HTTP routes; PostgreSQL releases the lock on both commit and rollback.

All application database access uses SeaORM. Loco's default migration runner is configured with an empty migrator so the startup synchronizer owns schema setup.

## Database roles

| Role | Purpose |
| --- | --- |
| `app_migrator` | Schema synchronization and isolated test setup |
| `app_runtime` | Application reads and writes; no schema changes |
| `app_queue` | Loco PostgreSQL background queue |

The local PostgreSQL setup script creates `brightcone_dev` and `brightcone_test`. Tests use `TEST_DATABASE_URL` and reset that database; never point it at development or production data.

## Session behavior

The browser keeps the access token in memory and the server stores the rotating refresh token only as an `HttpOnly`, `SameSite=Strict` cookie. On page reload the frontend calls `/api/auth/refresh` with credentials and the CSRF header, then restores the current user. API requests retry once after a 401 if the refresh cookie is still valid. The browser signs out and calls the revocation endpoint after 15 minutes without pointer, keyboard, touch, or wheel activity.

## Checks

```bash
cargo fmt --all -- --check
cargo clippy --all-features -- -D warnings
cargo test --all-features
cargo audit && cargo deny check
```

API endpoints are described in `src/openapi.rs` and `openapi.json`. In development, `GET /api/openapi.json` serves the specification. Use `make openapi` after contract changes, then regenerate frontend types with `npm run gen:api` in the frontend repository.

## Layout

```text
src/controllers/   HTTP handlers
src/models/        Loco/SeaORM models
src/initializers/  Loco startup initializers
src/database/      typed schema manifest and synchronizer
src/people/        employee and organization domain logic
config/            environment-specific application settings
scripts/           native PostgreSQL development setup
tests/             API and database tests
```

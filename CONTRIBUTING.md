# Contributing

## Branches and commits
- Branch from `main`: `feature/P1-04-login-api`, `fix/P1-04-lockout-count`.
- Start each commit message with the ticket ID: `P1-04: add login endpoint`.
- Open a pull request that says `Closes #<issue number>`. `main` is protected: one approval and green CI are required.

## Rules (short version of the security plan)
1. The browser is never trusted. Check permissions and rules on the server.
2. Every endpoint declares the permission it needs. No label, no merge.
3. Records that are not yours return `404`, not `403`.
4. Money is an integer in paise. Never a float.
5. Passwords, tokens and salary figures never go into logs.
6. Use SeaORM queries. Never build SQL by joining strings.
7. The app runs as `app_runtime` and cannot change table structure. Startup schema sync uses `SCHEMA_SYNC_DATABASE_URL` and a schema-owner role.
8. `audit_logs` is append-only.
9. Secrets come from environment variables, never from Git.
10. Generate, then edit: `cargo loco generate ...` writes the file and the wiring.

## Changing the API
1. Add the handler to `src/openapi.rs` (`paths`, and `schemas` for new types).
2. Run `make openapi` and commit `openapi.json`.
3. Copy `openapi.json` into the frontend repo and run `npm run gen:api` there.

## Commands
`make run` · `make test` · `make lint` · `make openapi`

# Branch notes — People Plus hardening

## Included

- Remove committed JWT fallback and plaintext demo credentials from maintained docs.
- Configure CORS from `FRONTEND_URL`, tighten API CSP, and keep CI aligned with startup schema sync.
- Keep in-memory Files and Approvals prototypes off the API; prevent submitter self-rejection in the prototype engine.
- Register refresh-token cleanup worker and update local developer commands.
- Replace stale completion claims with current backend scope, API list, run steps, and production gaps.
- Keep schema changes in the additive startup synchronizer; port API models, tasks, workers, and tests to the typed persistence layer.

## Validation

- Rust formatting passes.
- Backend test/clippy run could not be confirmed on this Windows host because the `ring` native build failed with linker `LNK1114`.
- `storage/uploads/*.pdf` and machine-specific Cargo target configuration are excluded from this branch.

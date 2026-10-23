# People data security operations

The People API stores employee names, work email, and phone as AES-256-GCM ciphertext. AAD binds each ciphertext to its employee field. Work-email uniqueness uses an independent HMAC-SHA-256 token, so normalized email is not persisted in a searchable column. Employee JSON records are excluded from the generic record API and generic exports.

## Required secrets

- `PEOPLE_DATA_ACTIVE_KEY_ID`: a short identifier without `:`.
- `PEOPLE_DATA_KEYS_JSON`: JSON object mapping every retained key ID to a 32-byte key encoded as 64 hexadecimal characters.
- `PEOPLE_EMAIL_LOOKUP_KEY_HEX`: independent secret of at least 32 bytes encoded as hexadecimal.

Generate keys with a cryptographically secure secret manager or `openssl rand -hex 32`. Do not commit keys or copy development keys to production. The API fails closed for People PII if these settings are missing, malformed, or refer to an unknown key.

For rotation, add a new key to the retained key map, deploy that map everywhere, switch the active key ID, and then run a separately reviewed re-encryption process before retiring an old key. The application currently supports reading retained key IDs and writing with the active key; it does not yet include the re-encryption worker or automated rotation verification. Backups contain ciphertext and require the matching key material.

Keep the email lookup HMAC key stable for now. Rotating it requires a controlled re-index of every employee email token (with duplicate checks) before the new token key is activated; that job is not implemented yet.

## Current authorization boundary

Only HR Admin can use the People endpoints. Employee self-service and manager team access remain denied until an account-to-employee mapping, scope resolver, and tests exist. PII is returned only on authenticated People API responses for HR Admin. Audit summaries contain changed field names and employee IDs, never employee field values.

## Release checks

Before loading real data, configure independent production keys in the deployment secret manager, validate backup restoration with the matching key set, review database roles and encrypted-column access, and complete database-backed tests plus security review. Do not expose generic record backup/import or CSV paths for employee records; the current generic endpoints explicitly filter People-shaped records.

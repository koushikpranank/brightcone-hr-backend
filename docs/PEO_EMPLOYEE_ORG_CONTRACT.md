# People operations: employee and organization contract

**Version:** 1.0 proposal, frozen for implementation pending product-owner sign-off  
**Scope:** PEO-01 through PEO-18  
**Data boundary:** Do not enter real employee or dependent data until PEO-03 encryption and production database checks pass.

This is the working contract used to implement the PEO tasks. Where the business has not provided a policy, the conservative default below applies. A policy change requires updating this document and the API contract before implementation changes.

## Employee record

| Field | Rule | Confidentiality |
|---|---|---|
| `id` | Server-generated UUID; immutable | Internal identifier |
| `employee_number` | Required, unique, immutable after onboarding | Internal identifier |
| `first_name`, `last_name` | Required; maximum 100 characters each | Personal; encrypted at rest |
| `work_email` | Required, valid email, unique; normalized lookup token | Personal; encrypted at rest |
| `phone` | Optional; validated E.164 or empty | Sensitive; encrypted at rest |
| `department_id`, `grade_id`, `location_id` | Optional during draft; must reference active setup records before activation | Internal |
| `manager_id` | Optional; must reference another active employee; no self-reference or reporting cycles | Internal |
| `employment_type` | `full_time`, `part_time`, `contract`, `intern` | Internal |
| `status` | `draft`, `preboarding`, `active`, `on_leave`, `probation`, `notice`, `exited` | Internal |
| `start_date`, `end_date` | ISO date; end cannot precede start; end required for exit | Internal |

V1 excludes personal email, home address, government identifiers, bank details, medical data, and free-form personal notes from the employee profile. Those fields require a separately approved purpose, access policy, retention, encryption, and UI masking before implementation.

## Organization records

- **Department:** UUID, unique code, name, optional parent department, active/archived status.
- **Grade:** UUID, unique code, label, integer rank, active/archived status. Grades are independent of job titles.
- **Location:** UUID, unique code, label, country code, optional region, active/archived status.
- Reference codes are case-insensitively unique. Referenced setup records cannot be hard-deleted; archive them instead.
- A department hierarchy must be acyclic. Employee manager links must also be acyclic. A manager may manage across departments unless a later policy says otherwise.

## Authorization and audit

- HR Admin can create and manage people and organization records and view decrypted PII.
- Employees can read only their own approved profile fields. Managers can read non-sensitive fields for their direct/indirect reports. The first release must deny these scopes until the identity-to-employee mapping and reporting resolver are implemented.
- Recruiter, Finance, and Payroll Admin receive no People PII access by default. Payroll access is a separate approved permission.
- Every create, update, archive, restore, import, and export is audited with actor, target, request ID, timestamp, and changed field names. Audit summaries and logs must never contain plaintext PII.
- No hard delete is available in the API. Restore is HR Admin only and audited.

## Sensitive-data protection

- Sensitive fields are encrypted by the application with authenticated encryption before storage. Ciphertext includes a format version and key identifier; a keyed lookup token supports exact email uniqueness without storing its normalized value.
- Encryption keys come from a production secret manager or deployment secret. There is no source-code or default key. The service fails closed for PII writes and reads if the active key is missing or unknown.
- Keys must support rotation with old-key reads and controlled re-encryption. Backups contain ciphertext and require the matching key material to restore usable PII.
- Search, logs, errors, CSV exports, history, and notifications must not disclose ciphertext internals or plaintext PII beyond the authorized response.

## CSV import/export

- CSV import begins with a dry-run preview: normalized headers, row-level validation, duplicate employee numbers/emails, relationship resolution, and no writes.
- Confirmation imports the complete accepted batch transactionally. Any invalid row rejects the batch; retries are idempotent by employee number. The result reports row numbers and safe error codes, never values from sensitive columns.
- CSV export is HR Admin only, bounded, `no-store`, audited, and excludes sensitive fields by default. A separate audited export grant is required to include encrypted personal fields; export output is decrypted only in memory.

## Acceptance gates

1. Employee and reference-data APIs enforce the schemas, uniqueness, foreign references, status transitions, and manager/department cycle checks.
2. Protected routes authenticate and authorize every request server-side; UI visibility alone is not access control.
3. PII is encrypted before database writes and decrypted only for authorized responses. Missing keys, tampering, and unknown key versions fail closed.
4. Audit/history records are transactional, append-only, and contain no plaintext PII.
5. UI list, detail, create, edit, archive, and empty/error states use live API responses and never seeded demo records.
6. API contract and generated TypeScript types match. Backend unit/request tests, frontend tests, line limit checks, and database-backed tests pass.
7. Real-data release remains blocked until key management, database backup/restore, access policy, and security review are approved.

## Implementation boundary

The existing generic record engine remains a shared validation/history foundation. PEO features must have named typed services, API routes, permissions, and dedicated screens. The engine's current HR-Admin-only access is not a substitute for employee ownership or manager-team authorization.

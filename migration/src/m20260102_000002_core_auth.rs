//! P1-01 / P1-09: access-control tables and the append-only audit log.
//!
//! Plain SQL on purpose: constraints, `citext`, triggers and privileges are PostgreSQL features
//! that the schema builder does not express.
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const UP: &str = r"
CREATE EXTENSION IF NOT EXISTS citext;

-- Emails compare case-insensitively: Rahul@x.com and rahul@x.com are the same account.
ALTER TABLE users ALTER COLUMN email TYPE citext;

ALTER TABLE users
  ADD COLUMN role                 text        NOT NULL DEFAULT 'employee',
  ADD COLUMN status               text        NOT NULL DEFAULT 'active',
  ADD COLUMN failed_logins        integer     NOT NULL DEFAULT 0,
  ADD COLUMN locked_at            timestamptz,
  ADD COLUMN last_login_at        timestamptz,
  ADD COLUMN must_change_password boolean     NOT NULL DEFAULT false,
  ADD COLUMN password_changed_at  timestamptz,
  ADD CONSTRAINT users_role_check   CHECK (role   IN ('employee', 'manager', 'finance', 'hr_admin')),
  ADD CONSTRAINT users_status_check CHECK (status IN ('active', 'locked', 'disabled')),
  ADD CONSTRAINT users_failed_logins_check CHECK (failed_logins >= 0);

-- Self-registration, e-mail verification and magic links are not part of this product:
-- HR creates every account. Password reset tokens move to their own hashed table.
ALTER TABLE users
  DROP COLUMN reset_token,
  DROP COLUMN reset_sent_at,
  DROP COLUMN email_verification_token,
  DROP COLUMN email_verification_sent_at,
  DROP COLUMN email_verified_at,
  DROP COLUMN magic_link_token,
  DROP COLUMN magic_link_expiration;

-- Only the SHA-256 hash of a token is stored, never the token itself.
CREATE TABLE refresh_tokens (
  id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  user_id     bigint      NOT NULL REFERENCES users (id) ON DELETE CASCADE,
  token_hash  text        NOT NULL UNIQUE,
  family_id   uuid        NOT NULL,
  persistent  boolean     NOT NULL,
  expires_at  timestamptz NOT NULL,
  used_at     timestamptz,
  revoked_at  timestamptz,
  created_ip  text,
  created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ix_refresh_tokens_user    ON refresh_tokens (user_id);
CREATE INDEX ix_refresh_tokens_family  ON refresh_tokens (family_id);
CREATE INDEX ix_refresh_tokens_expires ON refresh_tokens (expires_at);

CREATE TABLE password_resets (
  id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  user_id     bigint      NOT NULL REFERENCES users (id) ON DELETE CASCADE,
  token_hash  text        NOT NULL UNIQUE,
  expires_at  timestamptz NOT NULL,
  used_at     timestamptz,
  created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ix_password_resets_user ON password_resets (user_id);

-- Who did what and when. No foreign key on purpose: the record must outlive the user.
CREATE TABLE audit_logs (
  id             bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  actor_user_id  bigint,
  actor_email    text,
  action         text        NOT NULL,
  target_type    text,
  target_id      text,
  summary        text,
  ip             text,
  request_id     text,
  created_at     timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ix_audit_logs_created ON audit_logs (created_at DESC, id DESC);
CREATE INDEX ix_audit_logs_actor   ON audit_logs (actor_user_id, created_at DESC);
CREATE INDEX ix_audit_logs_action  ON audit_logs (action, created_at DESC);

-- Append-only, enforced twice: by privileges (below) and by triggers (even the owner is blocked).
CREATE FUNCTION audit_logs_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  RAISE EXCEPTION 'audit_logs is append-only';
END;
$$;
CREATE TRIGGER audit_logs_no_change   BEFORE UPDATE OR DELETE ON audit_logs
  FOR EACH ROW EXECUTE FUNCTION audit_logs_append_only();
CREATE TRIGGER audit_logs_no_truncate BEFORE TRUNCATE ON audit_logs
  FOR EACH STATEMENT EXECUTE FUNCTION audit_logs_append_only();

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'app_runtime') THEN
    REVOKE UPDATE, DELETE, TRUNCATE ON audit_logs FROM app_runtime;
  END IF;
END
$$;
";

const DOWN: &str = r"
DROP TABLE IF EXISTS audit_logs;
DROP FUNCTION IF EXISTS audit_logs_append_only();
DROP TABLE IF EXISTS password_resets;
DROP TABLE IF EXISTS refresh_tokens;
ALTER TABLE users
  DROP CONSTRAINT users_role_check,
  DROP CONSTRAINT users_status_check,
  DROP CONSTRAINT users_failed_logins_check,
  DROP COLUMN role,
  DROP COLUMN status,
  DROP COLUMN failed_logins,
  DROP COLUMN locked_at,
  DROP COLUMN last_login_at,
  DROP COLUMN must_change_password,
  DROP COLUMN password_changed_at,
  ADD COLUMN reset_token text,
  ADD COLUMN reset_sent_at timestamptz,
  ADD COLUMN email_verification_token text,
  ADD COLUMN email_verification_sent_at timestamptz,
  ADD COLUMN email_verified_at timestamptz,
  ADD COLUMN magic_link_token text,
  ADD COLUMN magic_link_expiration timestamptz;
ALTER TABLE users ALTER COLUMN email TYPE varchar;
";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.get_connection().execute_unprepared(UP).await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.get_connection().execute_unprepared(DOWN).await?;
        Ok(())
    }
}

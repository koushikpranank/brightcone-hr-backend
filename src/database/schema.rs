mod manifest;
#[cfg(test)]
mod tests;
pub use manifest::{Column, ColumnType, Table, TABLES};

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, FromQueryResult, Statement, TransactionTrait,
};

#[derive(FromQueryResult)]
struct ColumnRow {
    name: String,
    udt_name: String,
    is_nullable: String,
    is_identity: String,
}

pub async fn run_autonomous_sync(database: &DatabaseConnection) -> Result<(), String> {
    database
        .transaction::<_, (), sea_orm::DbErr>(|tx| {
            Box::pin(async move {
                tx.execute_unprepared("SELECT pg_advisory_xact_lock(88421038);")
                    .await?;
                for table in TABLES {
                    sync_table(tx, table)
                        .await
                        .map_err(sea_orm::DbErr::Custom)?;
                }
                ensure_constraints(tx)
                    .await
                    .map_err(sea_orm::DbErr::Custom)?;
                ensure_record_store(tx)
                    .await
                    .map_err(sea_orm::DbErr::Custom)?;
                ensure_people_schema(tx)
                    .await
                    .map_err(sea_orm::DbErr::Custom)?;
                ensure_audit_append_only(tx)
                    .await
                    .map_err(sea_orm::DbErr::Custom)?;
                Ok(())
            })
        })
        .await
        .map_err(|error| format!("synchronize database schema: {error}"))
}

async fn ensure_people_schema(db: &impl ConnectionTrait) -> Result<(), String> {
    db.execute_unprepared(r#"
    CREATE TABLE IF NOT EXISTS people_departments (id UUID PRIMARY KEY DEFAULT gen_random_uuid(), code TEXT NOT NULL, name TEXT NOT NULL, parent_id UUID REFERENCES people_departments(id), archived_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), updated_at TIMESTAMPTZ NOT NULL DEFAULT now());
    CREATE UNIQUE INDEX IF NOT EXISTS ux_people_departments_code ON people_departments(lower(code));
    CREATE TABLE IF NOT EXISTS people_grades (id UUID PRIMARY KEY DEFAULT gen_random_uuid(), code TEXT NOT NULL, label TEXT NOT NULL, rank INTEGER NOT NULL, archived_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), updated_at TIMESTAMPTZ NOT NULL DEFAULT now());
    CREATE UNIQUE INDEX IF NOT EXISTS ux_people_grades_code ON people_grades(lower(code));
    CREATE TABLE IF NOT EXISTS people_locations (id UUID PRIMARY KEY DEFAULT gen_random_uuid(), code TEXT NOT NULL, label TEXT NOT NULL, country_code TEXT NOT NULL, region TEXT, archived_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), updated_at TIMESTAMPTZ NOT NULL DEFAULT now());
    CREATE UNIQUE INDEX IF NOT EXISTS ux_people_locations_code ON people_locations(lower(code));
    CREATE TABLE IF NOT EXISTS people_employees (id UUID PRIMARY KEY DEFAULT gen_random_uuid(), employee_number TEXT NOT NULL UNIQUE, first_name_cipher TEXT NOT NULL, last_name_cipher TEXT NOT NULL, work_email_cipher TEXT NOT NULL, email_lookup TEXT NOT NULL UNIQUE, phone_cipher TEXT, department_id UUID REFERENCES people_departments(id), grade_id UUID REFERENCES people_grades(id), location_id UUID REFERENCES people_locations(id), manager_id UUID REFERENCES people_employees(id), employment_type TEXT NOT NULL, status TEXT NOT NULL, start_date DATE NOT NULL, end_date DATE, archived_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), updated_at TIMESTAMPTZ NOT NULL DEFAULT now());
    DO $$ BEGIN
      IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname='people_employment_type_check') THEN ALTER TABLE people_employees ADD CONSTRAINT people_employment_type_check CHECK (employment_type IN ('full_time','part_time','contract','intern')); END IF;
      IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname='people_employee_status_check') THEN ALTER TABLE people_employees ADD CONSTRAINT people_employee_status_check CHECK (status IN ('draft','preboarding','active','on_leave','probation','notice','exited')); END IF;
      IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname='people_employee_dates_check') THEN ALTER TABLE people_employees ADD CONSTRAINT people_employee_dates_check CHECK (end_date IS NULL OR end_date >= start_date); END IF;
      IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname='people_employee_exit_check') THEN ALTER TABLE people_employees ADD CONSTRAINT people_employee_exit_check CHECK (status <> 'exited' OR end_date IS NOT NULL); END IF;
      IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname='people_employee_active_refs_check') THEN ALTER TABLE people_employees ADD CONSTRAINT people_employee_active_refs_check CHECK (status <> 'active' OR (department_id IS NOT NULL AND grade_id IS NOT NULL AND location_id IS NOT NULL)); END IF;
      IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname='people_employee_manager_check') THEN ALTER TABLE people_employees ADD CONSTRAINT people_employee_manager_check CHECK (manager_id IS NULL OR manager_id <> id); END IF;
    END $$;
    CREATE INDEX IF NOT EXISTS ix_people_employees_active ON people_employees(status, employee_number) WHERE archived_at IS NULL;
    CREATE INDEX IF NOT EXISTS ix_people_employees_manager ON people_employees(manager_id) WHERE archived_at IS NULL;
    CREATE TABLE IF NOT EXISTS people_employee_history (id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, employee_id UUID NOT NULL, actor_user_id BIGINT NOT NULL REFERENCES users(id), action TEXT NOT NULL, changed_fields TEXT[] NOT NULL DEFAULT '{}', before_data JSONB, after_data JSONB, reason_code TEXT NOT NULL DEFAULT 'unspecified', created_at TIMESTAMPTZ NOT NULL DEFAULT now());
    ALTER TABLE people_employee_history ADD COLUMN IF NOT EXISTS before_data JSONB;
    ALTER TABLE people_employee_history ADD COLUMN IF NOT EXISTS after_data JSONB;
    ALTER TABLE people_employee_history ADD COLUMN IF NOT EXISTS reason_code TEXT NOT NULL DEFAULT 'unspecified';
    CREATE INDEX IF NOT EXISTS ix_people_employee_history ON people_employee_history(employee_id,id DESC);
    DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_proc WHERE proname='people_employee_history_append_only' AND pg_function_is_visible(oid)) THEN EXECUTE 'CREATE FUNCTION people_employee_history_append_only() RETURNS trigger LANGUAGE plpgsql AS $body$ BEGIN RAISE EXCEPTION ''people_employee_history is append-only''; END; $body$'; END IF; END $$;
    DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname='people_employee_history_no_change' AND NOT tgisinternal) THEN CREATE TRIGGER people_employee_history_no_change BEFORE UPDATE OR DELETE ON people_employee_history FOR EACH ROW EXECUTE FUNCTION people_employee_history_append_only(); END IF; IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname='people_employee_history_no_truncate' AND NOT tgisinternal) THEN CREATE TRIGGER people_employee_history_no_truncate BEFORE TRUNCATE ON people_employee_history FOR EACH STATEMENT EXECUTE FUNCTION people_employee_history_append_only(); END IF; END $$;
    "#).await.map_err(|error| format!("synchronize People schema: {error}"))?;
    for table in [
        "people_departments",
        "people_grades",
        "people_locations",
        "people_employees",
        "people_employee_history",
    ] {
        verify_table(db, table).await?;
    }
    Ok(())
}

async fn ensure_record_store(db: &impl ConnectionTrait) -> Result<(), String> {
    db.execute_unprepared(r#"
    CREATE TABLE IF NOT EXISTS hr_records (id UUID PRIMARY KEY DEFAULT gen_random_uuid(), record_type TEXT NOT NULL, source_id TEXT NOT NULL, data JSONB NOT NULL CHECK (jsonb_typeof(data) = 'object'), archived_at TIMESTAMPTZ, created_by BIGINT REFERENCES users(id), updated_by BIGINT REFERENCES users(id), created_at TIMESTAMPTZ NOT NULL DEFAULT now(), updated_at TIMESTAMPTZ NOT NULL DEFAULT now(), UNIQUE(record_type, source_id));
    CREATE INDEX IF NOT EXISTS ix_hr_records_type_active ON hr_records(record_type, created_at DESC) WHERE archived_at IS NULL;
    CREATE TABLE IF NOT EXISTS hr_record_history (id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, record_id UUID NOT NULL, action TEXT NOT NULL CHECK (action IN ('created','updated','archived','restored')), actor_user_id BIGINT NOT NULL REFERENCES users(id), before_data JSONB, after_data JSONB, created_at TIMESTAMPTZ NOT NULL DEFAULT now());
    CREATE INDEX IF NOT EXISTS ix_hr_record_history_record ON hr_record_history(record_id, id DESC);
    CREATE TABLE IF NOT EXISTS hr_record_unique_claims (record_type TEXT NOT NULL, field_key TEXT NOT NULL, normalized_value TEXT NOT NULL, record_id UUID NOT NULL REFERENCES hr_records(id) ON DELETE CASCADE, PRIMARY KEY(record_type, field_key, normalized_value), UNIQUE(record_id, field_key));
    CREATE INDEX IF NOT EXISTS ix_hr_record_unique_record ON hr_record_unique_claims(record_id);
    ALTER TABLE hr_record_history ADD COLUMN IF NOT EXISTS source_actor_user_id BIGINT;
    CREATE TABLE IF NOT EXISTS hr_rules (rule_key TEXT PRIMARY KEY, enabled BOOLEAN NOT NULL DEFAULT false, updated_by BIGINT REFERENCES users(id), updated_at TIMESTAMPTZ NOT NULL DEFAULT now());
    CREATE TABLE IF NOT EXISTS hr_rule_events (event_id UUID NOT NULL, action_key TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','complete','failed')), attempts INTEGER NOT NULL DEFAULT 0, payload JSONB NOT NULL DEFAULT '{}'::jsonb, last_error TEXT, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), completed_at TIMESTAMPTZ, PRIMARY KEY(event_id, action_key));
    ALTER TABLE hr_rule_events ADD COLUMN IF NOT EXISTS payload JSONB NOT NULL DEFAULT '{}'::jsonb;
    ALTER TABLE hr_rule_events ADD COLUMN IF NOT EXISTS last_error TEXT;
    CREATE INDEX IF NOT EXISTS ix_hr_rule_events_pending ON hr_rule_events(created_at) WHERE status='pending';
    CREATE TABLE IF NOT EXISTS hr_notifications (id UUID PRIMARY KEY DEFAULT gen_random_uuid(), user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE, event_id UUID NOT NULL, rule_key TEXT NOT NULL, title TEXT NOT NULL, body TEXT NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), read_at TIMESTAMPTZ, UNIQUE(event_id,user_id));
    CREATE INDEX IF NOT EXISTS ix_hr_notifications_user_unread ON hr_notifications(user_id, created_at DESC) WHERE read_at IS NULL;
    INSERT INTO hr_rules(rule_key) VALUES ('employee.created'),('employee.updated'),('employee.archived'),('leave.submitted'),('leave.decided'),('expense.submitted'),('expense.decided'),('candidate.stage_changed'),('payroll.completed') ON CONFLICT DO NOTHING;
    DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_proc WHERE proname='hr_record_history_append_only' AND pg_function_is_visible(oid)) THEN EXECUTE 'CREATE FUNCTION hr_record_history_append_only() RETURNS trigger LANGUAGE plpgsql AS $body$ BEGIN RAISE EXCEPTION ''hr_record_history is append-only''; END; $body$'; END IF; END $$;
    DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname='hr_record_history_no_change' AND NOT tgisinternal) THEN CREATE TRIGGER hr_record_history_no_change BEFORE UPDATE OR DELETE ON hr_record_history FOR EACH ROW EXECUTE FUNCTION hr_record_history_append_only(); END IF; IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname='hr_record_history_no_truncate' AND NOT tgisinternal) THEN CREATE TRIGGER hr_record_history_no_truncate BEFORE TRUNCATE ON hr_record_history FOR EACH STATEMENT EXECUTE FUNCTION hr_record_history_append_only(); END IF; END $$;
    "#).await.map_err(|error| format!("synchronize records and rules schema: {error}"))?;
    for table in [
        "hr_records",
        "hr_record_history",
        "hr_record_unique_claims",
        "hr_rules",
        "hr_rule_events",
        "hr_notifications",
    ] {
        verify_table(db, table).await?;
    }
    Ok(())
}

pub async fn reset_test_data(database_url: &str) -> Result<(), String> {
    let db = sea_orm::Database::connect(database_url)
        .await
        .map_err(|error| format!("connect to isolated test database: {error}"))?;
    if !database_url
        .rsplit('/')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .eq("brightcone_test")
    {
        return Err("TEST_DATABASE_URL must target brightcone_test".into());
    }
    db.execute_unprepared("BEGIN; ALTER TABLE audit_logs DISABLE TRIGGER USER; ALTER TABLE hr_record_history DISABLE TRIGGER USER; ALTER TABLE people_employee_history DISABLE TRIGGER USER; TRUNCATE TABLE people_employee_history, people_employees, people_departments, people_grades, people_locations, hr_rule_events, hr_record_unique_claims, hr_record_history, hr_records, refresh_tokens, password_resets, audit_logs, users RESTART IDENTITY CASCADE; UPDATE hr_rules SET enabled=false, updated_by=NULL, updated_at=now(); ALTER TABLE people_employee_history ENABLE TRIGGER USER; ALTER TABLE hr_record_history ENABLE TRIGGER USER; ALTER TABLE audit_logs ENABLE TRIGGER USER; COMMIT;").await.map_err(|error| format!("reset isolated test database: {error}"))?;
    db.close()
        .await
        .map_err(|error| format!("close isolated test database: {error}"))
}

async fn sync_table(db: &impl ConnectionTrait, table: &Table) -> Result<(), String> {
    let statement = Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT column_name AS name, udt_name, is_nullable, is_identity FROM information_schema.columns WHERE table_schema=current_schema() AND table_name=$1",
        [table.name.into()]);
    let columns = db
        .query_all_raw(statement)
        .await
        .map_err(|error| format!("inspect {} columns: {error}", table.name))?;
    let existing = columns
        .iter()
        .map(|row| {
            ColumnRow::from_query_result(row, "").map(|column| (column.name.clone(), column))
        })
        .collect::<Result<std::collections::HashMap<_, _>, _>>()
        .map_err(|error| format!("decode {} columns: {error}", table.name))?;
    if existing.is_empty() {
        let definitions = table
            .columns
            .iter()
            .map(column_ddl)
            .collect::<Vec<_>>()
            .join(", ");
        db.execute_unprepared(&format!(
            "CREATE TABLE IF NOT EXISTS {} ({definitions})",
            quote(table.name)
        ))
        .await
        .map_err(|error| format!("create {}: {error}", table.name))?;
    }
    for column in table.columns {
        if let Some(found) = existing.get(column.name) {
            let expected = match column.kind {
                ColumnType::Identity | ColumnType::BigInt => "int8",
                ColumnType::Integer => "int4",
                ColumnType::Text => "text",
                ColumnType::Boolean => "bool",
                ColumnType::Uuid => "uuid",
                ColumnType::Timestamptz => "timestamptz",
            };
            let identity_matches =
                (column.kind == ColumnType::Identity) == (found.is_identity == "YES");
            if found.udt_name != expected
                || !identity_matches
                || (column.nullable && found.is_nullable != "YES")
                || (!column.nullable && found.is_nullable != "NO")
            {
                return Err(format!("{}.{}, existing type/nullability ({}, {}) differs from declared ({}, {}); DBA review required", table.name, column.name, found.udt_name, found.is_nullable, expected, if column.nullable { "nullable" } else { "not null" }));
            }
        } else {
            db.execute_unprepared(&format!(
                "ALTER TABLE {} ADD COLUMN IF NOT EXISTS {}",
                quote(table.name),
                column_ddl(column)
            ))
            .await
            .map_err(|error| format!("add {}.{}: {error}", table.name, column.name))?;
        }
    }
    for index in table.indexes {
        db.execute_unprepared(index)
            .await
            .map_err(|error| format!("synchronize index for {}: {error}", table.name))?;
    }
    verify_table(db, table.name).await
}

async fn verify_table(db: &impl ConnectionTrait, table: &str) -> Result<(), String> {
    db.query_one_raw(Statement::from_string(
        DbBackend::Postgres,
        format!("SELECT 1 FROM {} LIMIT 1", quote(table)),
    ))
    .await
    .map_err(|error| format!("verify {table} query: {error}"))?;
    Ok(())
}

fn column_ddl(column: &Column) -> String {
    let kind = match column.kind {
        ColumnType::Identity => "BIGINT GENERATED BY DEFAULT AS IDENTITY",
        ColumnType::BigInt => "BIGINT",
        ColumnType::Integer => "INTEGER",
        ColumnType::Text => "TEXT",
        ColumnType::Boolean => "BOOLEAN",
        ColumnType::Uuid => "UUID",
        ColumnType::Timestamptz => "TIMESTAMPTZ",
    };
    let mut ddl = format!("{} {kind}", quote(column.name));
    if column.primary_key {
        ddl.push_str(" PRIMARY KEY");
    }
    if !column.nullable {
        ddl.push_str(" NOT NULL");
    }
    if let Some(default) = column.default {
        ddl.push_str(" DEFAULT ");
        ddl.push_str(default);
    }
    ddl
}
fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

async fn ensure_constraints(db: &impl ConnectionTrait) -> Result<(), String> {
    db.execute_unprepared(r#"DO $$ BEGIN
      IF EXISTS (SELECT 1 FROM pg_constraint WHERE conname='users_role_check' AND pg_get_constraintdef(oid) NOT LIKE '%recruiter%') THEN RAISE EXCEPTION 'legacy users_role_check blocks new roles; apply the reviewed manual schema change before startup'; END IF;
      IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname='users_role_check') THEN ALTER TABLE users ADD CONSTRAINT users_role_check CHECK (role IN ('employee','manager','finance','hr_admin','recruiter','payroll_admin')); END IF;
      IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname='users_status_check') THEN ALTER TABLE users ADD CONSTRAINT users_status_check CHECK (status IN ('active','locked','disabled')); END IF;
      IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname='users_failed_logins_check') THEN ALTER TABLE users ADD CONSTRAINT users_failed_logins_check CHECK (failed_logins >= 0); END IF;
      IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname='refresh_tokens_user_id_fkey') THEN ALTER TABLE refresh_tokens ADD CONSTRAINT refresh_tokens_user_id_fkey FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE; END IF;
      IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname='password_resets_user_id_fkey') THEN ALTER TABLE password_resets ADD CONSTRAINT password_resets_user_id_fkey FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE; END IF;
    END $$;"#).await.map(|_| ()).map_err(|error| format!("synchronize schema constraints: {error}"))
}
async fn ensure_audit_append_only(db: &impl ConnectionTrait) -> Result<(), String> {
    db.execute_unprepared(r#"DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_proc WHERE proname='audit_logs_append_only' AND pg_function_is_visible(oid)) THEN EXECUTE 'CREATE FUNCTION audit_logs_append_only() RETURNS trigger LANGUAGE plpgsql AS $body$ BEGIN RAISE EXCEPTION ''audit_logs is append-only''; END; $body$'; END IF; END $$;
    DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname='audit_logs_no_change' AND NOT tgisinternal) THEN CREATE TRIGGER audit_logs_no_change BEFORE UPDATE OR DELETE ON audit_logs FOR EACH ROW EXECUTE FUNCTION audit_logs_append_only(); END IF; IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname='audit_logs_no_truncate' AND NOT tgisinternal) THEN CREATE TRIGGER audit_logs_no_truncate BEFORE TRUNCATE ON audit_logs FOR EACH STATEMENT EXECUTE FUNCTION audit_logs_append_only(); END IF; END $$;"#)
        .await.map(|_| ()).map_err(|error| format!("secure audit log: {error}"))
}

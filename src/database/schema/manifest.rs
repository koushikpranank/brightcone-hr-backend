#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    Identity,
    BigInt,
    Integer,
    Text,
    Boolean,
    Uuid,
    Timestamptz,
}

#[derive(Clone, Copy)]
pub struct Column {
    pub name: &'static str,
    pub kind: ColumnType,
    pub nullable: bool,
    pub default: Option<&'static str>,
    pub primary_key: bool,
}

#[derive(Clone, Copy)]
pub struct Table {
    pub name: &'static str,
    pub columns: &'static [Column],
    pub indexes: &'static [&'static str],
}

const fn c(
    name: &'static str,
    kind: ColumnType,
    nullable: bool,
    default: Option<&'static str>,
    primary_key: bool,
) -> Column {
    Column {
        name,
        kind,
        nullable,
        default,
        primary_key,
    }
}

const USERS: &[Column] = &[
    c("id", ColumnType::Identity, false, None, true),
    c(
        "created_at",
        ColumnType::Timestamptz,
        false,
        Some("now()"),
        false,
    ),
    c(
        "updated_at",
        ColumnType::Timestamptz,
        false,
        Some("now()"),
        false,
    ),
    c(
        "pid",
        ColumnType::Uuid,
        false,
        Some("gen_random_uuid()"),
        false,
    ),
    c(
        "email",
        ColumnType::Text,
        false,
        Some("'legacy-' || gen_random_uuid() || '@invalid.local'"),
        false,
    ),
    c(
        "password",
        ColumnType::Text,
        false,
        Some("'!invalid-legacy-password!'"),
        false,
    ),
    c(
        "api_key",
        ColumnType::Text,
        false,
        Some("'disabled-' || gen_random_uuid()"),
        false,
    ),
    c(
        "name",
        ColumnType::Text,
        false,
        Some("'Migrated user'"),
        false,
    ),
    c("role", ColumnType::Text, false, Some("'employee'"), false),
    c("status", ColumnType::Text, false, Some("'disabled'"), false),
    c(
        "failed_logins",
        ColumnType::Integer,
        false,
        Some("0"),
        false,
    ),
    c("locked_at", ColumnType::Timestamptz, true, None, false),
    c("last_login_at", ColumnType::Timestamptz, true, None, false),
    c(
        "must_change_password",
        ColumnType::Boolean,
        false,
        Some("true"),
        false,
    ),
    c(
        "password_changed_at",
        ColumnType::Timestamptz,
        true,
        None,
        false,
    ),
];

const REFRESH_TOKENS: &[Column] = &[
    c("id", ColumnType::Identity, false, None, true),
    c("user_id", ColumnType::BigInt, false, None, false),
    c("token_hash", ColumnType::Text, false, None, false),
    c("family_id", ColumnType::Uuid, false, None, false),
    c(
        "persistent",
        ColumnType::Boolean,
        false,
        Some("false"),
        false,
    ),
    c("expires_at", ColumnType::Timestamptz, false, None, false),
    c("used_at", ColumnType::Timestamptz, true, None, false),
    c("revoked_at", ColumnType::Timestamptz, true, None, false),
    c("created_ip", ColumnType::Text, true, None, false),
    c(
        "created_at",
        ColumnType::Timestamptz,
        false,
        Some("now()"),
        false,
    ),
];

const PASSWORD_RESETS: &[Column] = &[
    c("id", ColumnType::Identity, false, None, true),
    c("user_id", ColumnType::BigInt, false, None, false),
    c("token_hash", ColumnType::Text, false, None, false),
    c("expires_at", ColumnType::Timestamptz, false, None, false),
    c("used_at", ColumnType::Timestamptz, true, None, false),
    c(
        "created_at",
        ColumnType::Timestamptz,
        false,
        Some("now()"),
        false,
    ),
];

const AUDIT_LOGS: &[Column] = &[
    c("id", ColumnType::Identity, false, None, true),
    c("actor_user_id", ColumnType::BigInt, true, None, false),
    c("actor_email", ColumnType::Text, true, None, false),
    c(
        "action",
        ColumnType::Text,
        false,
        Some("'system.schema_sync'"),
        false,
    ),
    c("target_type", ColumnType::Text, true, None, false),
    c("target_id", ColumnType::Text, true, None, false),
    c("summary", ColumnType::Text, true, None, false),
    c("ip", ColumnType::Text, true, None, false),
    c("request_id", ColumnType::Text, true, None, false),
    c(
        "created_at",
        ColumnType::Timestamptz,
        false,
        Some("now()"),
        false,
    ),
];

pub const TABLES: &[Table] = &[
    Table {
        name: "users",
        columns: USERS,
        indexes: &[
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_users_email_lower ON users (lower(email))",
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_users_api_key ON users (api_key)",
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_users_pid ON users (pid)",
        ],
    },
    Table {
        name: "refresh_tokens",
        columns: REFRESH_TOKENS,
        indexes: &[
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_refresh_tokens_token_hash ON refresh_tokens (token_hash)",
            "CREATE INDEX IF NOT EXISTS ix_refresh_tokens_user ON refresh_tokens (user_id)",
            "CREATE INDEX IF NOT EXISTS ix_refresh_tokens_family ON refresh_tokens (family_id)",
            "CREATE INDEX IF NOT EXISTS ix_refresh_tokens_expires ON refresh_tokens (expires_at)",
        ],
    },
    Table {
        name: "password_resets",
        columns: PASSWORD_RESETS,
        indexes: &[
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_password_resets_token_hash ON password_resets (token_hash)",
            "CREATE INDEX IF NOT EXISTS ix_password_resets_user ON password_resets (user_id)",
        ],
    },
    Table {
        name: "audit_logs",
        columns: AUDIT_LOGS,
        indexes: &[
            "CREATE INDEX IF NOT EXISTS ix_audit_logs_created ON audit_logs (created_at DESC, id DESC)",
            "CREATE INDEX IF NOT EXISTS ix_audit_logs_actor ON audit_logs (actor_user_id, created_at DESC)",
            "CREATE INDEX IF NOT EXISTS ix_audit_logs_action ON audit_logs (action, created_at DESC)",
        ],
    },
];

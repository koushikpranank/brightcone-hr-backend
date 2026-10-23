use super::*;

#[test]
fn generated_column_ddl_keeps_nullability_and_defaults() {
    let column = Column {
        name: "created_at",
        kind: ColumnType::Timestamptz,
        nullable: false,
        default: Some("now()"),
        primary_key: false,
    };
    assert_eq!(
        column_ddl(&column),
        "\"created_at\" TIMESTAMPTZ NOT NULL DEFAULT now()"
    );
}

#[test]
fn every_manifest_index_is_additive_and_idempotent() {
    for table in TABLES {
        for index in table.indexes {
            assert!(
                index.starts_with("CREATE INDEX IF NOT EXISTS ")
                    || index.starts_with("CREATE UNIQUE INDEX IF NOT EXISTS ")
            );
        }
    }
}

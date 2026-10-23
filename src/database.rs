mod empty_migrator;
mod schema;
pub mod sql;

use loco_rs::{app::AppContext, prelude::Error};
use sea_orm::DatabaseConnection;

pub use empty_migrator::EmptyMigrator;
pub use schema::{reset_test_data, run_autonomous_sync, Column, ColumnType, Table, TABLES};

pub struct Database;

impl Database {
    pub async fn connect(test: bool) -> loco_rs::Result<DatabaseConnection> {
        let sync_url = if test {
            let url = std::env::var("TEST_DATABASE_URL")
                .map_err(|_| Error::string("TEST_DATABASE_URL is required in test environment"))?;
            if database_name(&url) != Some("brightcone_test") {
                return Err(Error::string(
                    "TEST_DATABASE_URL must target the isolated brightcone_test database",
                ));
            }
            url
        } else {
            std::env::var("SCHEMA_SYNC_DATABASE_URL")
                .map_err(|_| Error::string("SCHEMA_SYNC_DATABASE_URL is required"))?
        };
        let sync_database = sea_orm::Database::connect(&sync_url)
            .await
            .map_err(database_error)?;
        run_autonomous_sync(&sync_database)
            .await
            .map_err(database_error)?;
        sync_database.close().await.map_err(database_error)?;
        let runtime_url = if test {
            std::env::var("TEST_DATABASE_URL")
                .map_err(|_| Error::string("TEST_DATABASE_URL is required"))?
        } else {
            std::env::var("DATABASE_URL").map_err(|_| Error::string("DATABASE_URL is required"))?
        };
        sea_orm::Database::connect(runtime_url)
            .await
            .map_err(database_error)
    }

    pub fn from_context(ctx: &AppContext) -> loco_rs::Result<DatabaseConnection> {
        Ok(ctx.db.clone())
    }

    pub fn map_error(error: impl std::fmt::Display) -> Error {
        database_error(error)
    }
}

fn database_name(url: &str) -> Option<&str> {
    url.rsplit('/')
        .next()?
        .split('?')
        .next()
        .filter(|name| !name.is_empty())
}

fn database_error(error: impl std::fmt::Display) -> Error {
    tracing::error!(error = %error, "SeaORM database operation failed");
    Error::string("Database operation failed")
}

use async_trait::async_trait;
use loco_rs::{
    app::{AppContext, Initializer},
    environment::Environment,
    Result,
};

use crate::database::Database;

pub struct SchemaSyncInitializer;

#[async_trait]
impl Initializer for SchemaSyncInitializer {
    fn name(&self) -> String {
        "schema_sync".to_owned()
    }

    async fn before_run(&self, ctx: &AppContext) -> Result<()> {
        let _database = Database::connect(ctx.environment == Environment::Test).await?;
        tracing::info!("database schema synchronized before HTTP routes are served");
        Ok(())
    }
}

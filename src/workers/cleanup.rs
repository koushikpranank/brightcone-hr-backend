use async_trait::async_trait;
use loco_rs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::models::refresh_tokens::Model as RefreshTokens;

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct TokenCleanupArgs {}

pub struct TokenCleanupWorker {
    pub ctx: AppContext,
}

#[async_trait]
impl BackgroundWorker<TokenCleanupArgs> for TokenCleanupWorker {
    fn build(ctx: &AppContext) -> Self {
        Self { ctx: ctx.clone() }
    }

    async fn perform(&self, _args: TokenCleanupArgs) -> Result<()> {
        let deleted = RefreshTokens::delete_expired(&self.ctx.db).await?;
        tracing::info!(deleted = deleted, "purged expired refresh tokens");
        Ok(())
    }
}

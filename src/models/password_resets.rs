//! Single-use password-reset links (ticket P1-11). Only the hash of the token is stored.
use chrono::{DateTime, Duration, FixedOffset, Local};
use loco_rs::{hash, prelude::*};

pub use super::_entities::password_resets::{self, ActiveModel, Entity, Model};
use super::refresh_tokens::hash_token;

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    /// Creates a reset token valid for `minutes` and returns the secret to e-mail. Older unused
    /// links for the same user stop working.
    ///
    /// # Errors
    /// Database errors.
    pub async fn issue(db: &DatabaseConnection, user_id: i64, minutes: i64) -> ModelResult<String> {
        let txn = db.begin().await?;
        Entity::update_many()
            .col_expr(
                password_resets::Column::UsedAt,
                Expr::current_timestamp().into(),
            )
            .filter(password_resets::Column::UserId.eq(user_id))
            .filter(password_resets::Column::UsedAt.is_null())
            .exec(&txn)
            .await?;

        let secret = hash::random_string(48);
        let expires_at: DateTime<FixedOffset> = (Local::now() + Duration::minutes(minutes)).into();
        ActiveModel {
            user_id: ActiveValue::set(user_id),
            token_hash: ActiveValue::set(hash_token(&secret)),
            expires_at: ActiveValue::set(expires_at),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        txn.commit().await?;
        Ok(secret)
    }

    /// Spends a token. Returns the user id, or `None` when the token is unknown, used or expired.
    ///
    /// # Errors
    /// Database errors.
    pub async fn consume(db: &DatabaseConnection, secret: &str) -> ModelResult<Option<i64>> {
        let txn = db.begin().await?;
        let found = Entity::find()
            .filter(password_resets::Column::TokenHash.eq(hash_token(secret)))
            .lock_exclusive()
            .one(&txn)
            .await?;
        let Some(row) = found else { return Ok(None) };
        if row.used_at.is_some() || row.expires_at < Local::now() {
            return Ok(None);
        }
        let user_id = row.user_id;
        let mut used = row.into_active_model();
        used.used_at = ActiveValue::set(Some(Local::now().into()));
        used.update(&txn).await?;
        txn.commit().await?;
        Ok(Some(user_id))
    }
}

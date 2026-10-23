//! Refresh tokens (ticket P1-05). The browser holds a random secret in an `HttpOnly` cookie; the
//! database holds only its SHA-256 hash. Every use swaps it for a new one ("rotation"). Presenting a
//! token that was already used means it leaked, so the whole chain (the "family") is revoked.
use chrono::{DateTime, Duration, FixedOffset, Local};
use loco_rs::{hash, prelude::*};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub use super::_entities::refresh_tokens::{self, ActiveModel, Entity, Model};
use crate::settings::AppSettings;

impl ActiveModelBehavior for ActiveModel {}

/// A freshly created token. `secret` is the only copy of the value and is never stored.
#[derive(Debug)]
pub struct Issued {
    pub secret: String,
    pub expires_at: DateTime<FixedOffset>,
    pub persistent: bool,
    pub user_id: i64,
}

#[derive(Debug)]
pub enum Rotation {
    Rotated(Issued),
    /// A used or revoked token came back. The family has been revoked.
    Reused {
        user_id: i64,
    },
    Invalid,
}

#[must_use]
pub fn hash_token(secret: &str) -> String {
    format!("{:x}", Sha256::digest(secret.as_bytes()))
}

fn ttl(settings: &AppSettings, persistent: bool) -> Duration {
    if persistent {
        Duration::days(settings.refresh_ttl_days)
    } else {
        Duration::hours(settings.session_ttl_hours)
    }
}

impl Model {
    /// Creates a token. Pass the `family` of the token being replaced when rotating.
    ///
    /// # Errors
    /// Database errors.
    pub async fn issue<C: ConnectionTrait>(
        db: &C,
        settings: &AppSettings,
        user_id: i64,
        family: Option<Uuid>,
        persistent: bool,
        ip: Option<String>,
    ) -> ModelResult<Issued> {
        let secret = hash::random_string(64);
        let expires_at: DateTime<FixedOffset> = (Local::now() + ttl(settings, persistent)).into();
        ActiveModel {
            user_id: ActiveValue::set(user_id),
            token_hash: ActiveValue::set(hash_token(&secret)),
            family_id: ActiveValue::set(family.unwrap_or_else(Uuid::new_v4)),
            persistent: ActiveValue::set(persistent),
            expires_at: ActiveValue::set(expires_at),
            created_ip: ActiveValue::set(ip),
            ..Default::default()
        }
        .insert(db)
        .await?;
        Ok(Issued {
            secret,
            expires_at,
            persistent,
            user_id,
        })
    }

    /// Swaps a presented token for a new one, atomically.
    ///
    /// # Errors
    /// Database errors.
    pub async fn rotate(
        db: &DatabaseConnection,
        settings: &AppSettings,
        presented: &str,
        ip: Option<String>,
    ) -> ModelResult<Rotation> {
        let txn = db.begin().await?;
        let found = Entity::find()
            .filter(refresh_tokens::Column::TokenHash.eq(hash_token(presented)))
            .lock_exclusive()
            .one(&txn)
            .await?;
        let Some(row) = found else {
            return Ok(Rotation::Invalid);
        };

        if row.used_at.is_some() || row.revoked_at.is_some() {
            Self::revoke_family(&txn, row.family_id).await?;
            txn.commit().await?;
            return Ok(Rotation::Reused {
                user_id: row.user_id,
            });
        }
        if row.expires_at < Local::now() {
            return Ok(Rotation::Invalid);
        }

        let (user_id, family, persistent) = (row.user_id, row.family_id, row.persistent);
        let mut used = row.into_active_model();
        used.used_at = ActiveValue::set(Some(Local::now().into()));
        used.update(&txn).await?;

        let issued = Self::issue(&txn, settings, user_id, Some(family), persistent, ip).await?;
        txn.commit().await?;
        Ok(Rotation::Rotated(issued))
    }

    /// Revokes the chain a token belongs to. Returns the user it belonged to, if the token is known.
    ///
    /// # Errors
    /// Database errors.
    pub async fn revoke_by_secret(
        db: &DatabaseConnection,
        secret: &str,
    ) -> ModelResult<Option<i64>> {
        let found = Entity::find()
            .filter(refresh_tokens::Column::TokenHash.eq(hash_token(secret)))
            .one(db)
            .await?;
        match found {
            Some(row) => {
                Self::revoke_family(db, row.family_id).await?;
                Ok(Some(row.user_id))
            }
            None => Ok(None),
        }
    }

    /// # Errors
    /// Database errors.
    pub async fn revoke_family<C: ConnectionTrait>(db: &C, family: Uuid) -> ModelResult<()> {
        Entity::update_many()
            .col_expr(
                refresh_tokens::Column::RevokedAt,
                Expr::current_timestamp().into(),
            )
            .filter(refresh_tokens::Column::FamilyId.eq(family))
            .filter(refresh_tokens::Column::RevokedAt.is_null())
            .exec(db)
            .await?;
        Ok(())
    }

    /// Signs the user out everywhere (password change, disable, lock).
    ///
    /// # Errors
    /// Database errors.
    pub async fn revoke_all_for_user<C: ConnectionTrait>(db: &C, user_id: i64) -> ModelResult<()> {
        Entity::update_many()
            .col_expr(
                refresh_tokens::Column::RevokedAt,
                Expr::current_timestamp().into(),
            )
            .filter(refresh_tokens::Column::UserId.eq(user_id))
            .filter(refresh_tokens::Column::RevokedAt.is_null())
            .exec(db)
            .await?;
        Ok(())
    }

    /// Housekeeping: removes tokens that expired more than a day ago.
    ///
    /// # Errors
    /// Database errors.
    pub async fn delete_expired(db: &DatabaseConnection) -> ModelResult<u64> {
        let cutoff: DateTime<FixedOffset> = (Local::now() - Duration::days(1)).into();
        let res = Entity::delete_many()
            .filter(refresh_tokens::Column::ExpiresAt.lt(cutoff))
            .exec(db)
            .await?;
        Ok(res.rows_affected)
    }
}

//! Audit log service (ticket P1-09). Every write endpoint records what happened through [`record`].
//!
//! Never put passwords, tokens or full salary figures in `summary`.
use loco_rs::prelude::*;

use crate::{
    models::{_entities::audit_logs, users},
    request_meta::RequestMeta,
};

/// One thing that happened.
pub struct Entry<'a> {
    /// Dotted verb, e.g. `auth.login`, `user.disabled`.
    pub action: &'a str,
    pub target_type: Option<&'a str>,
    pub target_id: Option<String>,
    pub summary: Option<String>,
}

impl<'a> Entry<'a> {
    #[must_use]
    pub const fn new(action: &'a str) -> Self {
        Self {
            action,
            target_type: None,
            target_id: None,
            summary: None,
        }
    }

    #[must_use]
    pub fn target(mut self, kind: &'a str, id: impl ToString) -> Self {
        self.target_type = Some(kind);
        self.target_id = Some(id.to_string());
        self
    }

    #[must_use]
    pub fn summary(mut self, text: impl Into<String>) -> Self {
        self.summary = Some(text.into());
        self
    }
}

/// Appends an entry. `actor` is `None` for events before anyone is signed in (a failed login).
///
/// # Errors
/// When the insert fails. Callers on security-critical paths should propagate this: an action that
/// cannot be recorded should not silently succeed.
pub async fn record<C: ConnectionTrait>(
    db: &C,
    actor: Option<&users::Model>,
    meta: &RequestMeta,
    entry: Entry<'_>,
) -> Result<()> {
    audit_logs::ActiveModel {
        actor_user_id: ActiveValue::set(actor.map(|u| u.id)),
        actor_email: ActiveValue::set(actor.map(|u| u.email.clone())),
        action: ActiveValue::set(entry.action.to_owned()),
        target_type: ActiveValue::set(entry.target_type.map(str::to_owned)),
        target_id: ActiveValue::set(entry.target_id),
        summary: ActiveValue::set(entry.summary),
        ip: ActiveValue::set(meta.ip.clone()),
        request_id: ActiveValue::set(meta.request_id.clone()),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(())
}

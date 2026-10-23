use loco_rs::prelude::*;

use crate::authz::AuthUser;

/// Central policy entry point for People writes and HR-wide reads.
/// The users permission remains accepted during the transition from the prototype role table.
pub fn require_people_manage(auth: &AuthUser) -> Result<()> {
    auth.require("people:manage")
        .or_else(|_| auth.require("users:manage"))
        .map(|_| ())
}

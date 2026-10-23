//! Roles, permissions and the authenticated-user extractor (ticket P1-08).
//!
//! Rules, in order of importance:
//! 1. Deny by default: a permission that is not listed below belongs to nobody.
//! 2. Every protected handler takes an [`AuthUser`] and calls [`AuthUser::require`].
//! 3. "Which role" is only half of the check. For a record that belongs to a person, the handler
//!    must also check ownership/team (see `can_access_employee`, added with the employee records in
//!    Phase 2) and answer `404`, not `403`, when the record is not theirs.
use axum::http::StatusCode;
use axum::{extract::FromRequestParts, http::request::Parts};
use loco_rs::{
    controller::{extractor::auth::extract_jwt_from_request_parts, ErrorDetail},
    prelude::*,
};
use serde::Serialize;

use crate::models::users;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Employee,
    Manager,
    Finance,
    HrAdmin,
}

impl Role {
    pub const ALL: [Self; 4] = [Self::Employee, Self::Manager, Self::Finance, Self::HrAdmin];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Employee => "employee",
            Self::Manager => "manager",
            Self::Finance => "finance",
            Self::HrAdmin => "hr_admin",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_str() == value)
    }
}

/// How far a permission reaches. Ordered: `Own` < `Team` < `All`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Own,
    Team,
    All,
}

impl Scope {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Own => "own",
            Self::Team => "team",
            Self::All => "all",
        }
    }
}

/// Every permission in the system as `resource:action`.
pub const ALL_PERMISSIONS: &[&str] = &[
    "profile:read",
    "approvals:submit",
    "approvals:read",
    "approvals:decide",
    "files:upload",
    "files:read",
    "users:manage",
    "audit:read",
];

/// The role → permission table. Add a line here when a ticket adds a permission.
#[must_use]
pub const fn scope_for(role: Role, permission: &str) -> Option<Scope> {
    // `match` on strings is not const-friendly everywhere, so compare bytes by hand.
    use Role::{Employee, Finance, HrAdmin, Manager};
    let p = permission.as_bytes();
    if eq(p, b"profile:read") || eq(p, b"approvals:submit") || eq(p, b"files:upload") {
        return Some(Scope::Own);
    }
    if eq(p, b"approvals:read") || eq(p, b"approvals:decide") {
        return match role {
            Employee => None,
            Manager => Some(Scope::Team),
            Finance | HrAdmin => Some(Scope::All),
        };
    }
    if eq(p, b"files:read") {
        return match role {
            HrAdmin => Some(Scope::All),
            Employee | Manager | Finance => Some(Scope::Own),
        };
    }
    if eq(p, b"users:manage") || eq(p, b"audit:read") {
        return match role {
            HrAdmin => Some(Scope::All),
            Employee | Manager | Finance => None,
        };
    }
    None
}

const fn eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// The permissions of a role as `resource:action:scope` strings, for `/api/me`.
#[must_use]
pub fn permissions_of(role: Role) -> Vec<String> {
    ALL_PERMISSIONS
        .iter()
        .filter_map(|p| scope_for(role, p).map(|s| format!("{p}:{}", s.as_str())))
        .collect()
}

#[must_use]
pub fn forbidden(code: &str, message: &str) -> Error {
    Error::CustomError(StatusCode::FORBIDDEN, ErrorDetail::new(code, message))
}

/// A signed-in, active user. Putting this in a handler signature makes the handler unreachable
/// without a valid access token, and a disabled or locked account is rejected immediately even if
/// its access token has not expired yet.
pub struct AuthUser {
    pub user: users::Model,
    pub role: Role,
}

impl AuthUser {
    /// Checks a permission and returns its scope.
    ///
    /// # Errors
    /// `403` when the role lacks the permission, or when the user must change their password first.
    pub fn require(&self, permission: &str) -> Result<Scope> {
        if self.user.must_change_password {
            return Err(forbidden(
                "password_change_required",
                "You must change your password before continuing.",
            ));
        }
        scope_for(self.role, permission)
            .ok_or_else(|| forbidden("forbidden", "You do not have access to this."))
    }
}

impl FromRequestParts<AppContext> for AuthUser {
    type Rejection = Error;

    async fn from_request_parts(
        parts: &mut Parts,
        ctx: &AppContext,
    ) -> std::result::Result<Self, Self::Rejection> {
        let jwt = extract_jwt_from_request_parts(parts, ctx)?;
        let unauthorized = || Error::Unauthorized("Please sign in.".to_owned());
        let user = users::Model::find_by_pid(&ctx.db, &jwt.claims.pid)
            .await
            .map_err(|_| unauthorized())?;
        if user.status != "active" {
            return Err(unauthorized());
        }
        let role = Role::parse(&user.role).ok_or_else(unauthorized)?;
        Ok(Self { user, role })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_permission_is_denied_to_everyone() {
        for role in Role::ALL {
            assert_eq!(scope_for(role, "payroll:run"), None);
        }
    }

    #[test]
    fn only_hr_reads_the_audit_log() {
        assert_eq!(scope_for(Role::HrAdmin, "audit:read"), Some(Scope::All));
        for role in [Role::Employee, Role::Manager, Role::Finance] {
            assert_eq!(scope_for(role, "audit:read"), None);
        }
    }

    #[test]
    fn employees_cannot_decide_approvals() {
        assert_eq!(scope_for(Role::Employee, "approvals:decide"), None);
        assert_eq!(
            scope_for(Role::Manager, "approvals:decide"),
            Some(Scope::Team)
        );
    }

    #[test]
    fn every_listed_permission_is_known_to_someone() {
        for p in ALL_PERMISSIONS {
            assert!(
                Role::ALL.into_iter().any(|r| scope_for(r, p).is_some()),
                "{p} has no owner"
            );
        }
    }
}

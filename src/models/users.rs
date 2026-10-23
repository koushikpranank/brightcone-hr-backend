use async_trait::async_trait;
use chrono::Local;
use loco_rs::{auth::jwt, hash, prelude::*};
use serde_json::Map;
use uuid::Uuid;

pub use super::_entities::users::{self, ActiveModel, Entity, Model};
use crate::authz::Role;

pub const MIN_PASSWORD_LEN: usize = 12;

/// Passwords nobody may use even if they are long enough. A real list is added with P1-07 review.
const COMMON_PASSWORDS: &[&str] = &[
    "password1234",
    "password12345",
    "123456789012",
    "qwertyuiop12",
    "welcome12345",
    "letmein12345",
    "administrator",
    "iloveyou1234",
    "changeme1234",
    "brightcone123",
];

/// What HR supplies to create an account.
#[derive(Debug)]
pub struct NewUser {
    pub email: String,
    pub name: String,
    pub password: String,
    pub role: Role,
    pub must_change_password: bool,
}

#[derive(Debug, Validate)]
pub struct Validator {
    #[validate(length(min = 2, max = 200, message = "Name must be 2 to 200 characters long."))]
    pub name: String,
    #[validate(email(message = "invalid email"), length(max = 254))]
    pub email: String,
}

impl Validatable for ActiveModel {
    fn validator(&self) -> Box<dyn Validate> {
        Box::new(Validator {
            name: self.name.as_ref().to_owned(),
            email: self.email.as_ref().to_owned(),
        })
    }
}

/// Lower-case and trim. Used for every e-mail we store or look up, so `Rahul@X.com` finds the same
/// account as `rahul@x.com`.
#[must_use]
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// Password rules (ticket P1-07): at least 12 characters, not a well-known password, not one
/// repeated character.
///
/// # Errors
/// A message that is safe to show to the user.
pub fn check_password_strength(password: &str) -> Result<(), &'static str> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err("Password must be at least 12 characters long.");
    }
    let lower = password.to_lowercase();
    if COMMON_PASSWORDS.contains(&lower.as_str()) {
        return Err("That password is too common. Choose another.");
    }
    let first = password.chars().next();
    if password.chars().all(|c| Some(c) == first) {
        return Err("Password must not be a single repeated character.");
    }
    Ok(())
}

#[async_trait::async_trait]
impl ActiveModelBehavior for super::_entities::users::ActiveModel {
    async fn before_save<C>(self, _db: &C, insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        let mut this = self;
        if let ActiveValue::Set(email) = &this.email {
            this.email = ActiveValue::Set(normalize_email(email));
        }
        this.validate()?;
        if insert {
            this.pid = ActiveValue::Set(Uuid::new_v4());
            this.api_key = ActiveValue::Set(format!("lo-{}", Uuid::new_v4()));
        }
        Ok(this)
    }
}

#[async_trait]
impl Authenticable for Model {
    async fn find_by_api_key(db: &DatabaseConnection, api_key: &str) -> ModelResult<Self> {
        Self::find_by_api_key(db, api_key).await
    }

    async fn find_by_claims_key(db: &DatabaseConnection, claims_key: &str) -> ModelResult<Self> {
        Self::find_by_pid(db, claims_key).await
    }
}

impl Model {
    /// # Errors
    /// `EntityNotFound` when there is no such account.
    pub async fn find_by_email(db: &DatabaseConnection, email: &str) -> ModelResult<Self> {
        users::Entity::find()
            .filter(users::Column::Email.eq(normalize_email(email)))
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// # Errors
    /// `EntityNotFound` when the id is not a UUID or there is no such account.
    pub async fn find_by_pid(db: &DatabaseConnection, pid: &str) -> ModelResult<Self> {
        let parse_uuid = Uuid::parse_str(pid).map_err(|e| ModelError::Any(e.into()))?;
        users::Entity::find()
            .filter(users::Column::Pid.eq(parse_uuid))
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// # Errors
    /// `EntityNotFound` when there is no such account.
    pub async fn find_by_api_key(db: &DatabaseConnection, api_key: &str) -> ModelResult<Self> {
        users::Entity::find()
            .filter(users::Column::ApiKey.eq(api_key))
            .one(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    #[must_use]
    pub fn verify_password(&self, password: &str) -> bool {
        hash::verify_password(password, &self.password)
    }

    #[must_use]
    pub fn role(&self) -> Option<Role> {
        Role::parse(&self.role)
    }

    /// Creates an account. Hashing happens here, never in a controller.
    ///
    /// # Errors
    /// `EntityAlreadyExists` when the e-mail is taken; other database errors.
    pub async fn create_with_password(
        db: &DatabaseConnection,
        params: &NewUser,
    ) -> ModelResult<Self> {
        let txn = db.begin().await?;
        let email = normalize_email(&params.email);

        if users::Entity::find()
            .filter(users::Column::Email.eq(&email))
            .one(&txn)
            .await?
            .is_some()
        {
            return Err(ModelError::EntityAlreadyExists {});
        }

        let password_hash =
            hash::hash_password(&params.password).map_err(|e| ModelError::Any(e.into()))?;
        let user = users::ActiveModel {
            email: ActiveValue::set(email),
            password: ActiveValue::set(password_hash),
            name: ActiveValue::set(params.name.trim().to_owned()),
            role: ActiveValue::set(params.role.as_str().to_owned()),
            must_change_password: ActiveValue::set(params.must_change_password),
            ..Default::default()
        }
        .insert(&txn)
        .await?;

        txn.commit().await?;
        Ok(user)
    }

    /// Creates the short-lived access token.
    ///
    /// # Errors
    /// When the token cannot be signed.
    pub fn generate_jwt(&self, secret: &str, expiration: u64) -> ModelResult<String> {
        jwt::JWT::new(secret)
            .generate_token(expiration, self.pid.to_string(), Map::new())
            .map_err(ModelError::from)
    }

    /// Counts a wrong password and locks the account when `threshold` is reached. The row is
    /// locked for the duration, so parallel guesses cannot slip under the limit.
    ///
    /// # Errors
    /// Database errors.
    pub async fn register_failed_login(
        db: &DatabaseConnection,
        id: i64,
        threshold: i32,
    ) -> ModelResult<Self> {
        let txn = db.begin().await?;
        let user = users::Entity::find_by_id(id)
            .lock_exclusive()
            .one(&txn)
            .await?
            .ok_or(ModelError::EntityNotFound)?;
        let failed = user.failed_logins.saturating_add(1);
        let lock_now = failed >= threshold && user.status == "active";
        let mut active = user.into_active_model();
        active.failed_logins = ActiveValue::set(failed);
        if lock_now {
            active.status = ActiveValue::set("locked".to_owned());
            active.locked_at = ActiveValue::set(Some(Local::now().into()));
        }
        let updated = active.update(&txn).await?;
        txn.commit().await?;
        Ok(updated)
    }
}

impl ActiveModel {
    /// # Errors
    /// Database errors.
    pub async fn record_login_success(mut self, db: &DatabaseConnection) -> ModelResult<Model> {
        self.failed_logins = ActiveValue::set(0);
        self.last_login_at = ActiveValue::set(Some(Local::now().into()));
        Ok(self.update(db).await?)
    }

    /// Saves a new password. `must_change` forces the user to pick their own at next sign-in
    /// (used for temporary passwords handed out by HR).
    ///
    /// # Errors
    /// Database or hashing errors.
    pub async fn set_password<C: ConnectionTrait>(
        mut self,
        db: &C,
        password: &str,
        must_change: bool,
    ) -> ModelResult<Model> {
        self.password =
            ActiveValue::set(hash::hash_password(password).map_err(|e| ModelError::Any(e.into()))?);
        self.must_change_password = ActiveValue::set(must_change);
        self.password_changed_at = ActiveValue::set(Some(Local::now().into()));
        Ok(self.update(db).await?)
    }

    /// Moves the account between `active`, `locked` and `disabled`, clearing the failure counter.
    ///
    /// # Errors
    /// Database errors.
    pub async fn set_status<C: ConnectionTrait>(
        mut self,
        db: &C,
        status: &str,
    ) -> ModelResult<Model> {
        self.status = ActiveValue::set(status.to_owned());
        self.failed_logins = ActiveValue::set(0);
        self.locked_at = ActiveValue::set(if status == "locked" {
            Some(Local::now().into())
        } else {
            None
        });
        Ok(self.update(db).await?)
    }
}

use loco_rs::prelude::*;
use sea_orm::FromQueryResult;
use uuid::Uuid;

use crate::{
    audit::{self, Entry},
    authz::AuthUser,
    database::sql,
    people::{policy::require_people_manage, security},
    request_meta::RequestMeta,
};

#[derive(FromQueryResult)]
struct CipherRow {
    id: Uuid,
    first_name_cipher: String,
    last_name_cipher: String,
    work_email_cipher: String,
    phone_cipher: Option<String>,
}

#[utoipa::path(post, path = "/api/people/security/re-encrypt", security(("bearer" = [])), responses((status = 200, body = usize)))]
#[debug_handler]
pub async fn re_encrypt(
    auth: AuthUser,
    State(ctx): State<AppContext>,
    meta: RequestMeta,
) -> Result<Response> {
    require_people_manage(&auth)?;
    let actor = auth.user.id;
    let changed = ctx.db.transaction::<_, usize, Error>(|tx| {
        Box::pin(async move {
            let rows = sql::all::<CipherRow>(tx,
                "SELECT id,first_name_cipher,last_name_cipher,work_email_cipher,phone_cipher FROM people_employees WHERE archived_at IS NULL FOR UPDATE", Vec::new()).await?;
            for row in &rows {
                let first = security::rotate("employee.first_name", &row.first_name_cipher)
                    .map_err(|_| Error::string("Employee key rotation failed"))?;
                let last = security::rotate("employee.last_name", &row.last_name_cipher)
                    .map_err(|_| Error::string("Employee key rotation failed"))?;
                let email = security::rotate("employee.work_email", &row.work_email_cipher)
                    .map_err(|_| Error::string("Employee key rotation failed"))?;
                let phone = row.phone_cipher.as_deref()
                    .map(|value| security::rotate("employee.phone", value))
                    .transpose().map_err(|_| Error::string("Employee key rotation failed"))?;
                sql::execute(tx,
                    "UPDATE people_employees SET first_name_cipher=$1,last_name_cipher=$2,work_email_cipher=$3,phone_cipher=$4,updated_at=now() WHERE id=$5",
                    vec![first.into(), last.into(), email.into(), phone.into(), row.id.into()]).await?;
                sql::execute(tx,
                    "INSERT INTO people_employee_history(employee_id,actor_user_id,action,changed_fields,reason_code) VALUES ($1,$2,'re_encrypted',ARRAY['encrypted_fields']::text[],'key_rotation')",
                    vec![row.id.into(), actor.into()]).await?;
            }
            Ok(rows.len())
        })
    }).await.map_err(|error| {
        tracing::error!(%error, "People data re-encryption transaction failed");
        Error::string("People data re-encryption failed")
    })?;
    audit::record(
        &ctx.db,
        Some(&auth.user),
        &meta,
        Entry::new("people.security.re_encrypted").summary("reason=key_rotation"),
    )
    .await?;
    format::json(changed)
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/people/security")
        .add("/re-encrypt", post(re_encrypt))
}

use diesel::{
    prelude::QueryableByName,
    sql_query,
    sql_types::{Nullable, Text, Uuid as SqlUuid},
};
use diesel_async::{AsyncConnection, RunQueryDsl};
use loco_rs::prelude::*;
use uuid::Uuid;

use crate::{
    audit::{self, Entry},
    authz::AuthUser,
    database::Database,
    people::{policy::require_people_manage, security},
    request_meta::RequestMeta,
};

#[derive(QueryableByName)]
struct CipherRow {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
    #[diesel(sql_type = Text)]
    first_name_cipher: String,
    #[diesel(sql_type = Text)]
    last_name_cipher: String,
    #[diesel(sql_type = Text)]
    work_email_cipher: String,
    #[diesel(sql_type = Nullable<Text>)]
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
    let db = Database::from_context(&ctx)?;
    let mut conn = db.get().await?;
    let actor = auth.user.id;
    let count = conn.transaction::<usize, diesel::result::Error, _>(async move |conn| {
        let rows = sql_query("SELECT id,first_name_cipher,last_name_cipher,work_email_cipher,phone_cipher FROM people_employees WHERE archived_at IS NULL FOR UPDATE").load::<CipherRow>(&mut *conn).await?;
        let mut changed = 0;
        for row in rows {
            let first = security::rotate("employee.first_name", &row.first_name_cipher).map_err(|_| diesel::result::Error::RollbackTransaction)?;
            let last = security::rotate("employee.last_name", &row.last_name_cipher).map_err(|_| diesel::result::Error::RollbackTransaction)?;
            let email = security::rotate("employee.work_email", &row.work_email_cipher).map_err(|_| diesel::result::Error::RollbackTransaction)?;
            let phone = row.phone_cipher.as_deref().map(|v| security::rotate("employee.phone", v)).transpose().map_err(|_| diesel::result::Error::RollbackTransaction)?;
            sql_query("UPDATE people_employees SET first_name_cipher=$1,last_name_cipher=$2,work_email_cipher=$3,phone_cipher=$4,updated_at=now() WHERE id=$5").bind::<Text,_>(first).bind::<Text,_>(last).bind::<Text,_>(email).bind::<Nullable<Text>,_>(phone).bind::<SqlUuid,_>(row.id).execute(&mut *conn).await?;
            sql_query("INSERT INTO people_employee_history(employee_id,actor_user_id,action,changed_fields,reason_code) VALUES ($1,$2,'re_encrypted',ARRAY['encrypted_fields']::text[],'key_rotation')").bind::<SqlUuid,_>(row.id).bind::<diesel::sql_types::BigInt,_>(actor).execute(&mut *conn).await?;
            changed += 1;
        }
        Ok(changed)
    }).await.map_err(Database::map_error)?;
    audit::record(
        &db,
        Some(&auth.user),
        &meta,
        Entry::new("people.security.re_encrypted").summary("reason=key_rotation"),
    )
    .await?;
    format::json(count)
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/api/people/security")
        .add("/re-encrypt", post(re_encrypt))
}

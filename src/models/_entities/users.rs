use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    pub pid: Uuid,
    pub email: String,
    pub password: String,
    pub api_key: String,
    pub name: String,
    pub role: String,
    pub status: String,
    pub failed_logins: i32,
    pub locked_at: Option<DateTimeWithTimeZone>,
    pub last_login_at: Option<DateTimeWithTimeZone>,
    pub must_change_password: bool,
    pub password_changed_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

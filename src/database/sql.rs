use sea_orm::{ConnectionTrait, DbBackend, DbErr, FromQueryResult, QueryResult, Statement, Value};

fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Postgres, sql, values)
}

pub async fn one<T: FromQueryResult>(
    connection: &impl ConnectionTrait,
    sql: &str,
    values: Vec<Value>,
) -> Result<T, DbErr> {
    let row = connection
        .query_one_raw(statement(sql, values))
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("query returned no rows".to_owned()))?;
    T::from_query_result(&row, "")
}

pub async fn optional<T: FromQueryResult>(
    connection: &impl ConnectionTrait,
    sql: &str,
    values: Vec<Value>,
) -> Result<Option<T>, DbErr> {
    connection
        .query_one_raw(statement(sql, values))
        .await?
        .map(|row: QueryResult| T::from_query_result(&row, ""))
        .transpose()
}

pub async fn all<T: FromQueryResult>(
    connection: &impl ConnectionTrait,
    sql: &str,
    values: Vec<Value>,
) -> Result<Vec<T>, DbErr> {
    connection
        .query_all_raw(statement(sql, values))
        .await?
        .iter()
        .map(|row| T::from_query_result(row, ""))
        .collect()
}

pub async fn execute(
    connection: &impl ConnectionTrait,
    sql: &str,
    values: Vec<Value>,
) -> Result<u64, DbErr> {
    Ok(connection
        .execute_raw(statement(sql, values))
        .await?
        .rows_affected())
}

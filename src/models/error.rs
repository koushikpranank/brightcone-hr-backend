#[derive(Debug)]
pub enum ModelError {
    EntityNotFound,
    EntityAlreadyExists,
    Validation(String),
    Database(String),
    Other(String),
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EntityNotFound => formatter.write_str("entity not found"),
            Self::EntityAlreadyExists => formatter.write_str("entity already exists"),
            Self::Validation(message) | Self::Database(message) | Self::Other(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl std::error::Error for ModelError {}

pub type ModelResult<T> = std::result::Result<T, ModelError>;

impl From<sea_orm::DbErr> for ModelError {
    fn from(error: sea_orm::DbErr) -> Self {
        let message = error.to_string();
        if matches!(error, sea_orm::DbErr::Exec(_))
            && (message.contains("23505")
                || message.contains("duplicate key value violates unique constraint"))
        {
            return Self::EntityAlreadyExists;
        }
        match error {
            sea_orm::DbErr::RecordNotFound(_) => Self::EntityNotFound,
            error => Self::Database(error.to_string()),
        }
    }
}

impl From<ModelError> for loco_rs::Error {
    fn from(error: ModelError) -> Self {
        match error {
            ModelError::EntityNotFound => Self::NotFound,
            ModelError::EntityAlreadyExists => Self::BadRequest("Record already exists.".into()),
            ModelError::Validation(message) => Self::BadRequest(message),
            ModelError::Database(_) | ModelError::Other(_) => Self::InternalServerError,
        }
    }
}

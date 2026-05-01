use thiserror::Error;

#[derive(Debug, Error)]
pub enum BuildError {
    #[error("invalid schema config: {0}")]
    InvalidSchemaConfig(String),

    #[error("system schema {schema:?} is missing the required table {table:?}; \
             run kronos-migrate or call kronos_client::migrate before starting the worker")]
    SystemSchemaMissing { schema: String, table: String },

    #[error("encryption_key is required but was not provided")]
    EncryptionKeyMissing,

    #[error("database error during worker build: {0}")]
    Database(#[from] sqlx::Error),
}

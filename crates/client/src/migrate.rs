//! Public migration entry point. Library users call `kronos_client::migrate(&pool, &opts)`
//! to render and apply all embedded migrations against their database.

pub use kronos_common::migrations::{apply, MigrateError, Migration, MIGRATIONS};
pub use kronos_common::schema_config::SchemaConfig;

use sqlx::PgPool;

/// Convenience alias matching the spec's API shape.
///
/// Equivalent to `kronos_common::migrations::apply(pool, opts)`.
pub async fn migrate(pool: &PgPool, opts: &SchemaConfig) -> Result<(), MigrateError> {
    apply(pool, opts).await
}

//! Embedded migration templates plus the renderer and apply entry point.

pub mod embedded;
pub mod render;

pub use embedded::{Migration, MIGRATIONS};
pub use render::{render, RenderError};

use crate::schema_config::SchemaConfig;
use sqlx::PgPool;

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("template render failed for {migration}: {source}")]
    Render {
        migration: &'static str,
        #[source]
        source: RenderError,
    },
    #[error("SQL execution failed for {migration}: {source}")]
    Sql {
        migration: &'static str,
        #[source]
        source: sqlx::Error,
    },
}

/// Render and apply every embedded migration against `pool`.
///
/// Each migration's template is rendered using `cfg`, then executed as a
/// single SQL statement-batch. Migrations are idempotent (every CREATE
/// uses `IF NOT EXISTS`), so re-running this function on an already-migrated
/// database is safe.
pub async fn apply(pool: &PgPool, cfg: &SchemaConfig) -> Result<(), MigrateError> {
    for m in MIGRATIONS {
        let rendered = render(m.template, cfg).map_err(|e| MigrateError::Render {
            migration: m.name,
            source: e,
        })?;
        tracing::info!(migration = m.name, "applying migration");
        sqlx::raw_sql(&rendered)
            .execute(pool)
            .await
            .map_err(|e| MigrateError::Sql {
                migration: m.name,
                source: e,
            })?;
    }
    Ok(())
}

//! Kronos library API. Today this crate exposes only the migration entry
//! point; subsequent plans add the enqueue + CRUD surface.

pub mod migrate;

pub use kronos_common::schema_config::SchemaConfig;
pub use migrate::{migrate, MigrateError, Migration, MIGRATIONS};

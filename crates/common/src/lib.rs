// Re-exported so embedders don't need their own sqlx dependency: sqlx types
// (PgPool, Error) appear in this crate's public API (e.g. SchemaProvider),
// and a separately pinned sqlx version would not type-check against them.
pub use sqlx;

pub mod cache;
pub mod config;
pub mod crypto;
pub mod db;
pub mod env;
pub mod error;
#[cfg(feature = "kms")]
pub mod kms;
pub mod metrics;
pub mod models;
pub mod pagination;
/// Shared workspace-mutation logic invoked by both the REST handler and the
/// library client, so the two cannot drift.
pub mod service;
pub mod template;
pub mod tenant;

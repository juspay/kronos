use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Identifies the workspace for a request.
#[derive(Debug, Clone)]
pub struct WorkspaceContext {
    pub org_id: String,
    pub workspace_id: String,
    pub schema_name: String,
}

/// Validates that a schema name contains only safe characters.
pub fn validate_schema_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Validates that a table prefix contains only safe characters (empty is also valid).
pub fn validate_table_prefix(prefix: &str) -> bool {
    prefix.is_empty() || prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Builds the schema name from org_id and workspace slug.
/// Replaces hyphens with underscores since PostgreSQL schema names can't contain hyphens.
pub fn build_schema_name(org_id: &str, workspace_slug: &str) -> String {
    format!(
        "{}_{}",
        org_id.replace('-', "_"),
        workspace_slug.replace('-', "_")
    )
}

/// Trait for discovering active workspace schemas.
/// Implement this to tell Kronos's worker where to find the list of workspaces.
/// Kronos ships `SchemaRegistry` as the default implementation.
#[async_trait]
pub trait SchemaProvider: Send + Sync + 'static {
    async fn get_active_schemas(&self) -> Result<Vec<String>, sqlx::Error>;
}

/// Cached registry of active workspace schemas.
/// Default `SchemaProvider` implementation — queries Kronos's own
/// `public.workspaces` table. Used by standalone Kronos.
#[derive(Clone)]
pub struct SchemaRegistry {
    pool: PgPool,
    cache: Arc<RwLock<CachedSchemas>>,
    ttl: Duration,
}

struct CachedSchemas {
    schemas: Vec<String>,
    fetched_at: Instant,
}

impl SchemaRegistry {
    pub fn new(pool: PgPool, ttl_secs: u64) -> Self {
        Self {
            pool,
            cache: Arc::new(RwLock::new(CachedSchemas {
                schemas: Vec::new(),
                fetched_at: Instant::now() - Duration::from_secs(ttl_secs + 1), // force initial fetch
            })),
            ttl: Duration::from_secs(ttl_secs),
        }
    }
}

#[async_trait]
impl SchemaProvider for SchemaRegistry {
    async fn get_active_schemas(&self) -> Result<Vec<String>, sqlx::Error> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if cache.fetched_at.elapsed() < self.ttl && !cache.schemas.is_empty() {
                return Ok(cache.schemas.clone());
            }
        }

        // Refresh
        let schemas: Vec<(String,)> =
            sqlx::query_as("SELECT schema_name FROM public.workspaces WHERE status = 'ACTIVE'")
                .fetch_all(&self.pool)
                .await?;

        let schemas: Vec<String> = schemas.into_iter().map(|r| r.0).collect();

        let mut cache = self.cache.write().await;
        cache.schemas = schemas.clone();
        cache.fetched_at = Instant::now();

        Ok(schemas)
    }
}

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

/// Builds the per-workspace schema name from `{prefix}{org_id}_{workspace_slug}`.
/// Replaces hyphens with underscores since PostgreSQL schema names can't contain hyphens.
pub fn build_schema_name(prefix: &str, org_id: &str, workspace_slug: &str) -> String {
    format!(
        "{}{}_{}",
        prefix,
        org_id.replace('-', "_"),
        workspace_slug.replace('-', "_")
    )
}

/// Cached registry of active workspace schemas.
/// Used by worker and scheduler to iterate tenants.
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

    pub async fn get_active_schemas(&self) -> Result<Vec<String>, sqlx::Error> {
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

#[cfg(test)]
mod build_schema_name_tests {
    use super::*;

    #[test]
    fn service_mode_no_prefix() {
        assert_eq!(
            build_schema_name("", "myorg", "prod"),
            "myorg_prod"
        );
    }

    #[test]
    fn library_mode_kronos_prefix() {
        assert_eq!(
            build_schema_name("kronos_", "myorg", "prod"),
            "kronos_myorg_prod"
        );
    }

    #[test]
    fn hyphens_in_org_id_become_underscores() {
        assert_eq!(
            build_schema_name("", "abc-123", "prod"),
            "abc_123_prod"
        );
    }

    #[test]
    fn hyphens_in_slug_become_underscores() {
        assert_eq!(
            build_schema_name("kronos_", "myorg", "prod-east"),
            "kronos_myorg_prod_east"
        );
    }
}

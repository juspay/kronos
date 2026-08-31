use sqlx::PgPool;
use std::future::Future;
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

/// Maximum length of an org/workspace slug. Kept well under PostgreSQL's 63-byte
/// identifier limit so a `{org}_{workspace}` schema name stays addressable.
pub const MAX_SLUG_LEN: usize = 25;

/// Validates an organization or workspace slug.
///
/// Slugs are user-supplied identifiers that flow into URLs and tenant schema
/// names, so we restrict them to a safe, readable shape: lowercase ASCII
/// letters, digits, and interior hyphens (no leading/trailing hyphen), 1..=25
/// characters. This guarantees a slug is always a safe schema-name component
/// once hyphens are mapped to underscores by `build_schema_name`.
pub fn validate_slug(slug: &str) -> bool {
    if slug.is_empty() || slug.len() > MAX_SLUG_LEN {
        return false;
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        return false;
    }
    slug.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
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
/// Implement this to tell Invokr's worker where to find the list of workspaces.
/// Invokr ships `SchemaRegistry` as the default implementation.
///
/// Declared with `-> impl Future + Send` (rather than `async fn`) so the
/// returned future is usable inside the spawned worker task; implementors
/// can simply write `async fn get_active_schemas(&self)`.
pub trait SchemaProvider: Send + Sync + 'static {
    fn get_active_schemas(
        &self,
    ) -> impl Future<Output = Result<Vec<String>, sqlx::Error>> + Send;
}

/// Cached registry of active workspace schemas.
/// Default `SchemaProvider` implementation — queries Invokr's own
/// `public.workspaces` table. Used by standalone Invokr.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_well_formed_slugs() {
        for s in ["acme", "acme-corp", "team-1", "a", "ab-cd-ef", "x1y2z3"] {
            assert!(validate_slug(s), "expected '{s}' to be valid");
        }
    }

    #[test]
    fn rejects_malformed_slugs() {
        for s in [
            "",
            "-acme",
            "acme-",
            "Acme",          // uppercase
            "acme_corp",     // underscore
            "acme corp",     // space
            "acme.corp",     // dot
            &"a".repeat(MAX_SLUG_LEN + 1),
        ] {
            assert!(!validate_slug(s), "expected '{s}' to be invalid");
        }
    }
}

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

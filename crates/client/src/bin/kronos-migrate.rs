//! `kronos-migrate` — render and apply Kronos migrations.
//!
//! Designed to drop into the existing `just db-migrate` recipe.

use clap::Parser;
use kronos_client::{migrate, SchemaConfig};
use sqlx::PgPool;

#[derive(Parser, Debug)]
#[command(about = "Render and apply Kronos migrations")]
struct Args {
    /// Postgres connection URL.
    #[arg(long, env = "TE_DATABASE_URL", hide_env_values = true)]
    database_url: String,

    /// System schema for shared tables (organizations, workspaces).
    #[arg(long, env = "TE_SYSTEM_SCHEMA", default_value = "public")]
    system_schema: String,

    /// Prefix prepended to per-workspace schema names.
    #[arg(long, env = "TE_TENANT_SCHEMA_PREFIX", default_value = "")]
    tenant_schema_prefix: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let cfg = SchemaConfig {
        system_schema: args.system_schema,
        tenant_schema_prefix: args.tenant_schema_prefix,
    };
    cfg.validate()
        .map_err(|e| anyhow::anyhow!("invalid schema config: {}", e))?;

    let pool = PgPool::connect(&args.database_url).await?;
    migrate(&pool, &cfg).await?;
    tracing::info!(
        system_schema = %cfg.system_schema,
        tenant_schema_prefix = ?cfg.tenant_schema_prefix,
        "migrations applied"
    );
    Ok(())
}

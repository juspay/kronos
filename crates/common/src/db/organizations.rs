use crate::models::organization::Organization;
use sqlx::PgPool;

pub async fn create(
    pool: &PgPool,
    name: &str,
    slug: &str,
) -> Result<Organization, sqlx::Error> {
    sqlx::query_as::<_, Organization>(
        "INSERT INTO public.organizations (name, slug) VALUES ($1, $2)
         RETURNING *",
    )
    .bind(name)
    .bind(slug)
    .fetch_one(pool)
    .await
}

pub async fn get(pool: &PgPool, org_id: &str) -> Result<Option<Organization>, sqlx::Error> {
    sqlx::query_as::<_, Organization>(
        "SELECT * FROM public.organizations WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_optional(pool)
    .await
}

pub async fn list(pool: &PgPool) -> Result<Vec<Organization>, sqlx::Error> {
    sqlx::query_as::<_, Organization>(
        "SELECT * FROM public.organizations WHERE status = 'ACTIVE' ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
}

pub async fn update(
    pool: &PgPool,
    org_id: &str,
    name: &str,
) -> Result<Option<Organization>, sqlx::Error> {
    sqlx::query_as::<_, Organization>(
        "UPDATE public.organizations SET name = $2, updated_at = now()
         WHERE org_id = $1 AND status = 'ACTIVE'
         RETURNING *",
    )
    .bind(org_id)
    .bind(name)
    .fetch_optional(pool)
    .await
}

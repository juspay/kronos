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

/// Resolve an organization by its `org_id` or its `slug`.
///
/// Accepts either form so clients can address orgs by their human-readable slug
/// while existing clients keyed on the UUID `org_id` keep working. An exact
/// `org_id` match is preferred when a value could match both namespaces.
pub async fn get(pool: &PgPool, org_ref: &str) -> Result<Option<Organization>, sqlx::Error> {
    sqlx::query_as::<_, Organization>(
        "SELECT * FROM public.organizations
         WHERE org_id = $1 OR slug = $1
         ORDER BY (org_id = $1) DESC
         LIMIT 1",
    )
    .bind(org_ref)
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

/// Update an organization addressed by its `org_id` or its `slug`.
///
/// The target row is resolved through a subselect (preferring an exact `org_id`
/// match) so an id/slug value that could match two different rows still updates
/// exactly one.
pub async fn update(
    pool: &PgPool,
    org_ref: &str,
    name: &str,
) -> Result<Option<Organization>, sqlx::Error> {
    sqlx::query_as::<_, Organization>(
        "UPDATE public.organizations SET name = $2, updated_at = now()
         WHERE org_id = (
             SELECT org_id FROM public.organizations
             WHERE (org_id = $1 OR slug = $1) AND status = 'ACTIVE'
             ORDER BY (org_id = $1) DESC
             LIMIT 1
         )
         RETURNING *",
    )
    .bind(org_ref)
    .bind(name)
    .fetch_optional(pool)
    .await
}

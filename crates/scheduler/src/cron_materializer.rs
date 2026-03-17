use chrono::Utc;
use kronos_common::{config::AppConfig, db};
use sqlx::PgPool;
use std::time::Duration;

pub async fn run(pool: PgPool, config: &AppConfig) -> anyhow::Result<()> {
    let interval = Duration::from_secs(config.cron_tick_interval_sec);

    tracing::info!("CRON materializer started (interval: {}s, batch: {})",
        config.cron_tick_interval_sec, config.cron_batch_size);

    loop {
        match materialize_tick(&pool, config.cron_batch_size).await {
            Ok(count) => {
                if count > 0 {
                    tracing::debug!("Materialized {} CRON executions", count);
                    // Don't sleep if we materialized, there might be more
                    continue;
                }
            }
            Err(e) => {
                tracing::error!("CRON materializer error: {}", e);
            }
        }
        tokio::time::sleep(interval).await;
    }
}

async fn materialize_tick(pool: &PgPool, batch_size: i64) -> anyhow::Result<u64> {
    let due_jobs = db::jobs::get_due_cron_jobs(pool, batch_size).await?;
    let mut materialized = 0u64;

    for job in due_jobs {
        let current_tick = match job.cron_next_run_at {
            Some(t) => t,
            None => continue,
        };

        let cron_expr = match &job.cron_expression {
            Some(e) => e.clone(),
            None => continue,
        };

        let tz_str = job.cron_timezone.as_deref().unwrap_or("UTC");
        let tz: chrono_tz::Tz = match tz_str.parse() {
            Ok(tz) => tz,
            Err(_) => {
                tracing::error!(job_id = %job.job_id, "Invalid timezone: {}", tz_str);
                continue;
            }
        };

        // Get retry policy from endpoint
        let max_attempts = match db::endpoints::get(pool, &job.endpoint).await? {
            Some(ep) => ep.get_retry_policy().max_attempts,
            None => 1,
        };

        // Create execution with idempotency key
        let epoch_ms = current_tick.timestamp_millis();
        let idemp_key = format!("cron_{}_{}", job.job_id, epoch_ms);

        let created = db::executions::create_cron_execution(
            pool,
            &job.job_id,
            &job.endpoint,
            &job.endpoint_type,
            &idemp_key,
            job.input.as_ref(),
            current_tick,
            max_attempts,
        ).await?;

        if created {
            materialized += 1;
        }

        // Compute next tick from current tick (not now!) for catch-up
        let schedule: cron::Schedule = match cron_expr.parse() {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(job_id = %job.job_id, "Invalid cron expression: {}", e);
                continue;
            }
        };

        let current_tz = current_tick.with_timezone(&tz);
        let next_tick = schedule.after(&current_tz).next()
            .map(|dt| dt.with_timezone(&Utc));

        if let Some(next) = next_tick {
            // Check if past ends_at
            if let Some(ends_at) = job.cron_ends_at {
                if next > ends_at {
                    tracing::info!(job_id = %job.job_id, "CRON job past ends_at, retiring");
                    let _ = db::jobs::cancel(pool, &job.job_id).await;
                    continue;
                }
            }

            // CAS update
            db::jobs::advance_cron_tick(pool, &job.job_id, current_tick, next).await?;
        }
    }

    Ok(materialized)
}

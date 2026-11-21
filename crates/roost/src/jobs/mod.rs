use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::{Row, SqlitePool};
use tokio::sync::Notify;

use crate::db::Database;
use crate::metrics::MetricsService;

/// Trabajo persistente en SQLite para tareas pesadas (miniaturas, videos, indexado).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: i64,
    pub kind: String,
    pub payload: String,
    pub status: String,
    pub attempts: i64,
    pub created_at: i64,
}

/// Cola persistente de trabajos pesados de baja prioridad (`AGENTS.md` §2).
///
/// - Duerme en `Notify` cuando no hay trabajos (0% CPU en reposo).
/// - Procesa de a 1 trabajo por vez en lotes chicos.
/// - Se pausa si la CPU del equipo supera el 80%.
/// - Si se corta la luz, retoma los trabajos `running` / `pending` al arrancar.
#[derive(Clone)]
pub struct JobQueue {
    pool: SqlitePool,
    wake: Arc<Notify>,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl JobQueue {
    pub fn new(db: &Database) -> Self {
        Self {
            pool: db.pool().clone(),
            wake: Arc::new(Notify::new()),
        }
    }

    /// Reencola trabajos que quedaron a medias tras un corte de luz.
    pub async fn recover_interrupted(&self) -> Result<u64, sqlx::Error> {
        let res = sqlx::query("UPDATE jobs SET status = 'pending' WHERE status = 'running'")
            .execute(&self.pool)
            .await?;
        if res.rows_affected() > 0 {
            self.wake.notify_one();
        }
        Ok(res.rows_affected())
    }

    /// Agrega un trabajo persistente a la cola y despierta al worker.
    pub async fn enqueue(&self, kind: &str, payload: &str) -> Result<i64, sqlx::Error> {
        let res = sqlx::query(
            "INSERT INTO jobs (kind, payload, status, attempts, created_at)
             VALUES (?, ?, 'pending', 0, ?)",
        )
        .bind(kind)
        .bind(payload)
        .bind(now_unix())
        .execute(&self.pool)
        .await?;
        self.wake.notify_one();
        Ok(res.last_insert_rowid())
    }

    /// Toma el siguiente trabajo pendiente (máximo 3 intentos).
    pub async fn claim_next(&self) -> Result<Option<Job>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, kind, payload, status, attempts, created_at
             FROM jobs
             WHERE status = 'pending' AND attempts < 3
             ORDER BY created_at ASC, id ASC
             LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let id: i64 = row.get("id");
        let attempts: i64 = row.get::<i64, _>("attempts") + 1;
        sqlx::query("UPDATE jobs SET status = 'running', attempts = ? WHERE id = ?")
            .bind(attempts)
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(Some(Job {
            id,
            kind: row.get("kind"),
            payload: row.get("payload"),
            status: "running".to_string(),
            attempts,
            created_at: row.get("created_at"),
        }))
    }

    pub async fn mark_done(&self, id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE jobs SET status = 'done' WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn mark_failed(&self, id: i64, retryable: bool) -> Result<(), sqlx::Error> {
        let next_status = if retryable { "pending" } else { "failed" };
        sqlx::query("UPDATE jobs SET status = ? WHERE id = ?")
            .bind(next_status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Cuenta trabajos pendientes o en ejecución.
    pub async fn pending_count(&self) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE status IN ('pending', 'running')")
            .fetch_one(&self.pool)
            .await
    }

    /// Arranca un único worker de baja prioridad que duerme si la cola está vacía
    /// y pausa si la CPU supera el 80%.
    pub fn spawn_worker<F, Fut>(&self, metrics: MetricsService, handler: F)
    where
        F: Fn(Job) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
    {
        let queue = self.clone();
        let handler = Arc::new(handler);

        tokio::spawn(async move {
            let _ = queue.recover_interrupted().await;
            loop {
                match queue.claim_next().await {
                    Ok(Some(job)) => {
                        // Pausa si la CPU supera el 80% (`AGENTS.md` §2).
                        let snap = metrics.snapshot();
                        if !snap.demo && snap.cpu_percent > 80.0 {
                            let _ = queue.mark_failed(job.id, true).await;
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            continue;
                        }

                        let job_id = job.id;
                        let attempts = job.attempts;
                        match handler(job).await {
                            Ok(()) => {
                                let _ = queue.mark_done(job_id).await;
                            }
                            Err(err) => {
                                tracing::warn!(
                                    "Trabajo {job_id} falló (intento {attempts}): {err}"
                                );
                                let _ = queue.mark_failed(job_id, attempts < 3).await;
                            }
                        }
                        // Pequeño respiro entre trabajos para no ahogar una Raspberry Pi.
                        tokio::task::yield_now().await;
                    }
                    Ok(None) => {
                        queue.wake.notified().await;
                    }
                    Err(err) => {
                        tracing::warn!("Error al leer la cola de trabajos: {err}");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn queue_enqueues_claims_and_recovers_after_power_loss() {
        let db = Database::connect_in_memory().await.unwrap();
        let queue = JobQueue::new(&db);

        let id = queue.enqueue("thumb", "42").await.unwrap();
        assert_eq!(queue.pending_count().await.unwrap(), 1);

        let claimed = queue.claim_next().await.unwrap().unwrap();
        assert_eq!(claimed.id, id);
        assert_eq!(claimed.attempts, 1);

        // Simulamos un corte de luz mientras el trabajo estaba `running`.
        let recovered = queue.recover_interrupted().await.unwrap();
        assert_eq!(recovered, 1);

        let reclaimed = queue.claim_next().await.unwrap().unwrap();
        assert_eq!(reclaimed.id, id);
        assert_eq!(reclaimed.attempts, 2);

        queue.mark_done(id).await.unwrap();
        assert_eq!(queue.pending_count().await.unwrap(), 0);
    }
}

use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// Handle to a running worker. The worker continues until `shutdown()` is
/// called (graceful) or the handle is dropped (immediate task abort).
pub struct WorkerHandle {
    pub(crate) shutdown_tx: Option<oneshot::Sender<()>>,
    pub(crate) join: Option<JoinHandle<anyhow::Result<()>>>,
}

impl WorkerHandle {
    /// Send the shutdown signal and wait for the worker loop to drain in-flight
    /// tasks. Returns the worker's final result (`Ok(())` on clean exit).
    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            // Receiver is only dropped if the worker exited early; its result
            // will be surfaced by join.await below.
            let _ = tx.send(());
        }
        // `take()` so the `Drop` impl below doesn't also abort the (already
        // draining) task once we return.
        let join = self
            .join
            .take()
            .expect("join handle present until shutdown consumes self");
        match join.await {
            Ok(res) => res,
            Err(join_err) => Err(anyhow::anyhow!("worker task panicked: {join_err}")),
        }
    }
}

impl Drop for WorkerHandle {
    /// Aborts the spawned worker task. `tokio::task::JoinHandle::drop` only
    /// detaches — without this impl, dropping a `WorkerHandle` would leak the
    /// worker (the loop would keep polling forever). Graceful shutdown is
    /// opt-in via [`WorkerHandle::shutdown`]; bare drop is "fire abort and go."
    fn drop(&mut self) {
        if let Some(join) = self.join.as_ref() {
            join.abort();
        }
    }
}

use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// Handle to a running worker. The worker continues until `shutdown()` is
/// called (graceful) or the handle is dropped (immediate task abort).
pub struct WorkerHandle {
    pub(crate) shutdown_tx: Option<oneshot::Sender<()>>,
    pub(crate) join: JoinHandle<anyhow::Result<()>>,
}

impl WorkerHandle {
    /// Send the shutdown signal and wait for the worker loop to drain in-flight
    /// tasks. Returns the worker's final result (`Ok(())` on clean exit).
    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            // Receiver may have been dropped already; that's fine.
            let _ = tx.send(());
        }
        match self.join.await {
            Ok(res) => res,
            Err(join_err) => Err(anyhow::anyhow!("worker task panicked: {join_err}")),
        }
    }
}

use std::process;
use tokio::sync::mpsc;
use tracing::error;

/// A handle for managing graceful shutdown of the application
pub struct ShutdownManager {
    /// Receiver for shutdown signals
    pub rx: mpsc::Receiver<()>,
}

impl ShutdownManager {
    /// Create a new ShutdownManager and setup signal handlers
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(1);

        // Setup Ctrl+C handler
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            match tokio::signal::ctrl_c().await {
                Ok(()) => {
                    // Do NOT print here - it can cause broken pipe errors
                    let _ = tx_clone.send(()).await;
                }
                Err(err) => {
                    // Just log the error to a file if needed
                    error!("Error listening for ctrl+c: {}", err);
                }
            }
        });

        // Setup SIGTERM handler on Unix platforms
        #[cfg(unix)]
        {
            let tx_clone = tx.clone();
            tokio::spawn(async move {
                let mut term_signal =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("Failed to install SIGTERM handler");

                term_signal.recv().await;
                // Do NOT print here - it can cause broken pipe errors
                let _ = tx_clone.send(()).await;
            });
        }

        ShutdownManager { rx }
    }

    /// Execute the provided future until completion or shutdown signal
    pub async fn run_until_shutdown<F, T>(mut self, future: F) -> Option<T>
    where
        F: std::future::Future<Output = T>,
    {
        tokio::select! {
            _ = self.rx.recv() => {
                // Exit immediately on shutdown signal
                process::exit(0);
            }
            result = future => {
                Some(result)
            }
        }
    }
}

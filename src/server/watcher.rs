use crate::domain::analyze::scan_actual_model;
use crate::store::Store;
use anyhow::Result;
use notify::{Event, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use tracing::{error, info};

pub struct ActualStateWatcher {
    workspace_root: PathBuf,
    store: Arc<Store>,
}

impl ActualStateWatcher {
    pub fn new(workspace_root: impl Into<PathBuf>, store: Arc<Store>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            store,
        }
    }

    /// Spawns the watcher on a background Tokio task
    pub async fn spawn(self) -> Result<()> {
        let (tx, mut rx) = mpsc::channel(100);

        // 1. Initialize the file system watcher
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                // Filter out non-code changes (e.g. only watch .rs files)
                let is_rust_file = event.paths.iter().any(|p| {
                    p.extension().is_some_and(|ext| ext == "rs")
                });

                if is_rust_file {
                    // We just need a generic signal, so try_send is perfect.
                    // If the channel is full, a sync is already queued.
                    let _ = tx.try_send(());
                }
            }
        })?;

        // Watch the src/ directory specifically to avoid target/ build artifacts
        let src_path = self.workspace_root.join("src");
        if src_path.exists() {
            watcher.watch(&src_path, RecursiveMode::Recursive)?;
            info!("Started background AST watcher on {}", src_path.display());
        } else {
            watcher.watch(&self.workspace_root, RecursiveMode::Recursive)?;
            info!("Started background AST watcher on {}", self.workspace_root.display());
        }

        let store = self.store;
        let root = self.workspace_root;

        tokio::spawn(async move {
            // Keep the watcher alive by moving it into the task
            let _watcher = watcher; 

            loop {
                // 2. Wait for the first file-change event
                if rx.recv().await.is_none() {
                    break; 
                }

                // 3. Debounce: wait to see if more events arrive in the next 2 seconds
                // This prevents running the AST parser 10 times during a "Save All"
                let debounce_duration = Duration::from_secs(2);
                loop {
                    tokio::select! {
                        res = rx.recv() => {
                            if res.is_none() {
                                return; // Channel closed, exit the task completely
                            }
                            // Reset the debounce timer if another event comes in
                            continue; 
                        }
                        _ = sleep(debounce_duration) => {
                            // Timer expired, time to sync!
                            info!("Code modification detected. Syncing Actual Model...");
                            if let Err(e) = sync_actual_model(&root, &store).await {
                                error!("Failed to sync actual model: {}", e);
                            }
                            break; // Done with this batch, go back to waiting for the next first event
                        }
                    }
                }
            }
        });

        Ok(())
    }
}

/// Helper function to perform the AST extraction and DB transaction
async fn sync_actual_model(workspace_root: &Path, store: &Arc<Store>) -> Result<()> {
    // We need the Desired model so `scan_actual_model` knows how to map structs to contexts
    let workspace_str = workspace_root.to_string_lossy().to_string();
    let desired_model = store.load_desired(&workspace_str)?.unwrap_or_else(|| crate::domain::model::DomainModel::empty(&workspace_str));

    // Extract structure from source code AST
    let actual_model = scan_actual_model(workspace_root, &desired_model)?;

    // Promote the AST extraction into the Actual state database relation
    store.save_actual(&workspace_str, &actual_model)?;

    info!("Actual model successfully synced with codebase.");
    Ok(())
}

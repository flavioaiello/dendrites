use dendrites::domain;
use dendrites::server;
use dendrites::store;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "dendrites", about = "Domain Model Context Protocol Server")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the MCP stdio server (default when no subcommand given)
    Serve {
        /// Workspace path — auto-detected from VS Code via ${workspaceFolder}
        #[arg(short, long)]
        workspace: String,
    },

    /// Import a dendrites.json file into the store for a workspace
    Import {
        /// Path to the JSON file to import
        file: String,

        /// Workspace path to associate with this model
        #[arg(short, long)]
        workspace: String,
    },

    /// Export a workspace's domain model to a JSON file
    Export {
        /// Output file path
        file: String,

        /// Workspace path whose model to export
        #[arg(short, long)]
        workspace: String,
    },

    /// List all projects stored in the local database
    List,

    /// Check live workspace semantics without prompting LLM
    Check {
        /// Workspace path
        #[arg(short, long)]
        workspace: String,
    },

    /// Scan the workspace source code and populate the actual domain model
    Scan {
        /// Workspace path
        #[arg(short, long)]
        workspace: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        // Default: serve
        None => {
            eprintln!("Usage: dendrites serve --workspace <path>");
            eprintln!("       dendrites import <file> --workspace <path>");
            eprintln!("       dendrites export <file> --workspace <path>");
            eprintln!("       dendrites list");
            std::process::exit(1);
        }

        Some(Commands::Serve { workspace }) => {
            let store = std::sync::Arc::new(store::Store::open_default()?);
            tracing::info!("Dendrites Server starting for workspace: {}", workspace);

            let workspace_path = std::path::PathBuf::from(&workspace);
            let watcher_store = std::sync::Arc::clone(&store);
            let watcher = server::watcher::ActualStateWatcher::new(
                workspace_path,
                watcher_store,
            );

            // Spawn the background watcher
            tokio::spawn(async move {
                if let Err(e) = watcher.spawn().await {
                    tracing::error!("AST Watcher failed: {}", e);
                }
            });

            server::stdio::run(workspace, store).await?;
        }

        Some(Commands::Import { file, workspace }) => {
            let store = store::Store::open_default()?;
            let model = store.import_from_file(&workspace, &file)?;
            eprintln!(
                "Imported '{}' ({} contexts) into store for workspace: {}",
                model.name,
                model.bounded_contexts.len(),
                workspace
            );
        }

        Some(Commands::Export { file, workspace }) => {
            let store = store::Store::open_default()?;
            store.export_to_file(&workspace, &file)?;
            eprintln!("Exported model for workspace '{}' to: {}", workspace, file);
        }

        Some(Commands::List) => {
            let store = store::Store::open_default()?;
            let projects = store.list()?;
            if projects.is_empty() {
                eprintln!("No projects in store.");
            } else {
                eprintln!("{:<50} {:<25} UPDATED", "WORKSPACE", "PROJECT");
                eprintln!("{}", "-".repeat(95));
                for p in &projects {
                    eprintln!(
                        "{:<50} {:<25} {}",
                        p.workspace_path, p.project_name, p.updated_at
                    );
                }
                eprintln!("\n{} project(s) total", projects.len());
            }
        }

        Some(Commands::Check { workspace }) => {
            let store = store::Store::open_default()?;
            let live_deps = domain::analyze::scan_workspace(std::path::Path::new(&workspace))?;
            eprintln!("Extracted {} live imports across the workspace.", live_deps.len());
            
            // Map live imports to the ephemeral CozoDB logic
            match store.check_live_dependencies(&workspace, &live_deps) {
                Ok(violations) => {
                    if violations.is_empty() {
                        eprintln!("No architectural layer violations found during continuous check.");
                    } else {
                        eprintln!("Violations found: {:?}", violations);
                    }
                }
                Err(e) => eprintln!("Failed to test live dependencies: {}", e),
            }
        }

        Some(Commands::Scan { workspace }) => {
            let store = store::Store::open_default()?;
            let desired = store.load_desired(&workspace)?
                .unwrap_or_else(|| domain::model::DomainModel::empty(&workspace));

            if desired.bounded_contexts.is_empty() {
                eprintln!("No bounded contexts in the desired model. Seed the model first with:");
                eprintln!("  dendrites import <file> --workspace {}", workspace);
                eprintln!("  or via the MCP set_model tool.");
                std::process::exit(1);
            }

            let workspace_root = std::path::Path::new(&workspace);
            let actual = domain::analyze::scan_actual_model(workspace_root, &desired)?;

            let entity_count: usize = actual.bounded_contexts.iter().map(|bc| bc.entities.len()).sum();
            let vo_count: usize = actual.bounded_contexts.iter().map(|bc| bc.value_objects.len()).sum();
            let svc_count: usize = actual.bounded_contexts.iter().map(|bc| bc.services.len()).sum();
            let repo_count: usize = actual.bounded_contexts.iter().map(|bc| bc.repositories.len()).sum();
            let event_count: usize = actual.bounded_contexts.iter().map(|bc| bc.events.len()).sum();

            store.save_actual(&workspace, &actual)?;

            eprintln!("Scanned {} contexts → {} entities, {} VOs, {} services, {} repos, {} events",
                actual.bounded_contexts.len(), entity_count, vo_count, svc_count, repo_count, event_count);
            eprintln!("Actual model saved. Run `dendrites export <file> -w {}` or use get_model to see diff.", workspace);
        }
    }

    Ok(())
}

use clap::Parser;
use easyjob_agent::lock::{LockOutcome, SingleInstanceLock};
use easyjob_agent::service::AgentService;
use easyjob_logging::init_logging;
use std::path::PathBuf;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(author, version, about = "easyJob Background Task Scheduling Daemon")]
struct Cli {
    /// Custom directory for DB, sockets, and logs. Default: ~/.easyjob/
    #[arg(long, value_name = "PATH")]
    data_dir: Option<PathBuf>,

    /// Run in daemon mode (suppress console output, write rolling logs to <data-dir>/logs/agent.log)
    #[arg(long, default_value_t = false)]
    daemon: bool,

    /// Global concurrency limit
    #[arg(long, default_value_t = 16)]
    max_concurrent: usize,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let data_dir = cli.data_dir.unwrap_or_else(|| {
        #[cfg(target_os = "windows")]
        {
            let base = std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .unwrap_or_else(|_| ".".to_string());
            PathBuf::from(base).join(".easyjob")
        }
        #[cfg(not(target_os = "windows"))]
        {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".easyjob")
        }
    });
    std::fs::create_dir_all(&data_dir)?;

    let log_dir = data_dir.join("logs");
    let _guard = init_logging(&log_dir, !cli.daemon, "info")?;

    let lock_path = data_dir.join("agent.lock");
    #[cfg(not(target_os = "windows"))]
    let ipc_path = data_dir.join("easyjob.sock");
    #[cfg(target_os = "windows")]
    let ipc_path = easyjob_ipc::default_ipc_path();

    let db_path = data_dir.join("easyjob.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());

    match SingleInstanceLock::acquire(&lock_path, &ipc_path).await? {
        LockOutcome::AlreadyRunning => {
            println!("easyjob-agent is already running.");
            return Ok(());
        }
        LockOutcome::Acquired(_lock) => {
            info!(
                "Starting easyJob Agent Daemon v{}",
                env!("CARGO_PKG_VERSION")
            );
            let service = AgentService::init(&db_url, &ipc_path, cli.max_concurrent).await?;

            tokio::select! {
                res = service.run() => {
                    if let Err(e) = res {
                        error!("Agent service encountered error: {:?}", e);
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("Received SIGINT/Ctrl+C, exiting gracefully");
                }
            }
        }
    }

    Ok(())
}

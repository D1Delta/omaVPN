mod manager;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use omavpn_core::ipc::{DaemonRequest, DaemonResponse};
use omavpn_core::{DEFAULT_SINGBOX_BIN, DEFAULT_SOCKET_PATH, DEFAULT_XRAY_BIN};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{signal, SignalKind};
use tracing::{error, info, warn};

use crate::manager::CoreManager;

#[derive(Debug, Parser)]
#[command(name = "omavpnd", about = "System daemon for omavpn (sing-box + TUN)")]
struct Args {
    /// Unix socket path for IPC
    #[arg(long, default_value = DEFAULT_SOCKET_PATH)]
    socket: PathBuf,

    /// Path to sing-box binary
    #[arg(long, default_value = DEFAULT_SINGBOX_BIN)]
    sing_box: String,

    /// Path to xray-core binary
    #[arg(long, default_value = DEFAULT_XRAY_BIN)]
    xray: String,

    /// Runtime directory for temporary sing-box configs
    #[arg(long, default_value = "/run/omavpn")]
    runtime_dir: PathBuf,

    /// Octal permissions for the Unix socket (e.g. 660 or 666)
    #[arg(long, default_value = "660")]
    socket_mode: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    if let Some(parent) = args.socket.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create socket dir {}", parent.display()))?;
    }
    if args.socket.exists() {
        let _ = fs::remove_file(&args.socket);
    }

    let listener = UnixListener::bind(&args.socket)
        .with_context(|| format!("failed to bind Unix socket {}", args.socket.display()))?;

    let mode = u32::from_str_radix(&args.socket_mode, 8).unwrap_or(0o660);
    fs::set_permissions(&args.socket, fs::Permissions::from_mode(mode))
        .with_context(|| format!("failed to chmod socket {}", args.socket.display()))?;

    info!(
        "omavpnd listening on {} (mode {:o})",
        args.socket.display(),
        mode
    );

    let manager = CoreManager::new(args.sing_box, args.xray, args.runtime_dir);

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    loop {
        tokio::select! {
            accept_res = listener.accept() => {
                match accept_res {
                    Ok((stream, _)) => {
                        let mgr = manager.clone();
                        tokio::spawn(async move {
                            if let Err(err) = handle_client(stream, mgr).await {
                                warn!("IPC client error: {err}");
                            }
                        });
                    }
                    Err(err) => {
                        error!("failed to accept Unix socket connection: {err}");
                    }
                }
            }
            _ = sigterm.recv() => {
                info!("received SIGTERM, shutting down...");
                break;
            }
            _ = sigint.recv() => {
                info!("received SIGINT, shutting down...");
                break;
            }
        }
    }

    if let Err(err) = manager.stop().await {
        error!("error during shutdown cleanup: {err}");
    }
    let _ = fs::remove_file(&args.socket);
    info!("omavpnd exited cleanly");
    Ok(())
}

async fn handle_client(stream: UnixStream, manager: CoreManager) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    if reader.read_line(&mut line).await? == 0 {
        return Ok(());
    }

    let response = match serde_json::from_str::<DaemonRequest>(line.trim()) {
        Ok(req) => dispatch(req, &manager).await,
        Err(err) => DaemonResponse::Error {
            message: format!("invalid request JSON: {err}"),
        },
    };

    let mut payload = serde_json::to_string(&response)?;
    payload.push('\n');
    write_half.write_all(payload.as_bytes()).await?;
    write_half.shutdown().await?;
    Ok(())
}

async fn dispatch(req: DaemonRequest, manager: &CoreManager) -> DaemonResponse {
    match req {
        DaemonRequest::Status => DaemonResponse::Ok {
            status: manager.status().await,
        },
        DaemonRequest::Start {
            profile_id,
            profile_name,
            mode,
            mixed_port,
            config_json,
            xray_config_json,
            engine_label,
        } => match manager
            .start(
                profile_id,
                profile_name,
                mode,
                mixed_port,
                config_json,
                xray_config_json,
                engine_label,
            )
            .await
        {
            Ok(status) => DaemonResponse::Ok { status },
            Err(err) => DaemonResponse::Error {
                message: err.to_string(),
            },
        },
        DaemonRequest::Stop => match manager.stop().await {
            Ok(status) => DaemonResponse::Ok { status },
            Err(err) => DaemonResponse::Error {
                message: err.to_string(),
            },
        },
        DaemonRequest::Logs { limit } => DaemonResponse::Logs {
            lines: manager.recent_logs(limit).await,
        },
    }
}

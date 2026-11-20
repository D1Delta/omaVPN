use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use omavpn_core::ipc::{DaemonRequest, DaemonResponse};
use omavpn_core::models::{AppSettings, DaemonStatus, Profile, RunMode};
use omavpn_core::{singbox, xray, DEFAULT_XRAY_BRIDGE_PORT};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[derive(Debug, Clone)]
pub struct DaemonClient {
    socket_path: PathBuf,
}

impl DaemonClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    #[allow(dead_code)]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub async fn status(&self) -> Result<DaemonStatus> {
        match self.send(&DaemonRequest::Status).await? {
            DaemonResponse::Ok { status } => Ok(status),
            DaemonResponse::Error { message } => bail!("{message}"),
            other => bail!("unexpected daemon response: {other:?}"),
        }
    }

    pub async fn start(&self, profile: &Profile, settings: &AppSettings) -> Result<DaemonStatus> {
        let use_xray = xray::should_use_xray(profile, settings.engine);

        let (config_json, xray_config_json, engine_label) = if use_xray {
            match settings.mode {
                RunMode::Tun => {
                    let bridge_port = if settings.mixed_port == DEFAULT_XRAY_BRIDGE_PORT {
                        DEFAULT_XRAY_BRIDGE_PORT + 1
                    } else {
                        DEFAULT_XRAY_BRIDGE_PORT
                    };
                    let xcfg = xray::build_config_pretty(profile, settings, bridge_port)?;
                    let sb_bridge = singbox::build_tun_bridge_config_pretty(
                        settings,
                        bridge_port,
                        &profile.server,
                    )?;
                    (Some(sb_bridge), Some(xcfg), Some("xray+tun".to_string()))
                }
                RunMode::ProxyOnly => {
                    let xcfg = xray::build_config_pretty(profile, settings, settings.mixed_port)?;
                    (None, Some(xcfg), Some("xray".to_string()))
                }
            }
        } else {
            let sb = singbox::build_config_pretty(profile, settings)?;
            (Some(sb), None, Some("sing-box".to_string()))
        };

        let req = DaemonRequest::Start {
            profile_id: profile.id.clone(),
            profile_name: profile.display_name().to_string(),
            mode: settings.mode,
            mixed_port: settings.mixed_port,
            config_json,
            xray_config_json,
            engine_label,
        };
        match self.send(&req).await? {
            DaemonResponse::Ok { status } => Ok(status),
            DaemonResponse::Error { message } => bail!("{message}"),
            other => bail!("unexpected daemon response: {other:?}"),
        }
    }

    pub async fn stop(&self) -> Result<DaemonStatus> {
        match self.send(&DaemonRequest::Stop).await? {
            DaemonResponse::Ok { status } => Ok(status),
            DaemonResponse::Error { message } => bail!("{message}"),
            other => bail!("unexpected daemon response: {other:?}"),
        }
    }

    pub async fn logs(&self, limit: usize) -> Result<Vec<String>> {
        match self.send(&DaemonRequest::Logs { limit }).await? {
            DaemonResponse::Logs { lines } => Ok(lines),
            DaemonResponse::Error { message } => bail!("{message}"),
            other => bail!("unexpected daemon response: {other:?}"),
        }
    }

    async fn send(&self, req: &DaemonRequest) -> Result<DaemonResponse> {
        let stream = UnixStream::connect(&self.socket_path).await.with_context(|| {
            format!(
                "cannot connect to omavpnd at {} (is `sudo systemctl start omavpnd` running?)",
                self.socket_path.display()
            )
        })?;

        let (read_half, mut write_half) = stream.into_split();
        let mut payload = serde_json::to_string(req)?;
        payload.push('\n');
        write_half.write_all(payload.as_bytes()).await?;
        write_half.shutdown().await?;

        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 {
            return Err(anyhow!("omavpnd closed connection without response"));
        }

        serde_json::from_str::<DaemonResponse>(line.trim())
            .context("failed to decode daemon response")
    }
}

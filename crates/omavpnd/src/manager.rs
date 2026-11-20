use std::collections::VecDeque;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use omavpn_core::models::{DaemonStatus, RunMode};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::{error, info, warn};

const MAX_LOG_LINES: usize = 500;

struct RunningProcess {
    label: &'static str,
    child: Child,
    config_path: PathBuf,
}

#[derive(Clone)]
pub struct CoreManager {
    singbox_bin: String,
    xray_bin: String,
    runtime_dir: PathBuf,
    inner: Arc<Mutex<CoreState>>,
    logs: Arc<Mutex<VecDeque<String>>>,
}

struct CoreState {
    singbox: Option<RunningProcess>,
    xray: Option<RunningProcess>,
    status: DaemonStatus,
}

impl CoreManager {
    pub fn new(singbox_bin: String, xray_bin: String, runtime_dir: PathBuf) -> Self {
        Self {
            singbox_bin,
            xray_bin,
            runtime_dir,
            inner: Arc::new(Mutex::new(CoreState {
                singbox: None,
                xray: None,
                status: DaemonStatus::default(),
            })),
            logs: Arc::new(Mutex::new(VecDeque::with_capacity(MAX_LOG_LINES))),
        }
    }

    pub async fn status(&self) -> DaemonStatus {
        let mut state = self.inner.lock().await;
        Self::refresh_process_state(&mut state);
        state.status.clone()
    }

    pub async fn recent_logs(&self, limit: usize) -> Vec<String> {
        let logs = self.logs.lock().await;
        let n = limit.min(logs.len());
        logs.iter().skip(logs.len() - n).cloned().collect()
    }

    async fn push_log(logs: &Arc<Mutex<VecDeque<String>>>, line: String) {
        let mut guard = logs.lock().await;
        if guard.len() >= MAX_LOG_LINES {
            guard.pop_front();
        }
        guard.push_back(line);
    }

    fn poll_slot(slot: &mut Option<RunningProcess>) -> Option<String> {
        let reason = if let Some(proc) = slot.as_mut() {
            match proc.child.try_wait() {
                Ok(Some(exit_status)) => Some(format!("{} exited ({exit_status})", proc.label)),
                Ok(None) => None,
                Err(err) => Some(format!("failed to poll {}: {err}", proc.label)),
            }
        } else {
            None
        };

        if reason.is_some() {
            if let Some(proc) = slot.take() {
                let _ = fs::remove_file(&proc.config_path);
            }
        }
        reason
    }

    fn refresh_process_state(state: &mut CoreState) {
        let sb_exit = Self::poll_slot(&mut state.singbox);
        let xr_exit = Self::poll_slot(&mut state.xray);

        if let Some(reason) = sb_exit.or(xr_exit) {
            warn!("{reason}");
            state.status.running = false;
            state.status.pid = None;
            state.status.last_error = Some(reason);
        }
    }

    pub async fn start(
        &self,
        profile_id: String,
        profile_name: String,
        mode: RunMode,
        mixed_port: u16,
        config_json: Option<String>,
        xray_config_json: Option<String>,
        engine_label: Option<String>,
    ) -> Result<DaemonStatus> {
        if config_json.is_none() && xray_config_json.is_none() {
            bail!("neither sing-box nor xray config was provided");
        }

        fs::create_dir_all(&self.runtime_dir).with_context(|| {
            format!(
                "failed to create runtime dir {}",
                self.runtime_dir.display()
            )
        })?;

        // 1. Validate xray config first (if requested)
        if let Some(ref xcfg) = xray_config_json {
            let check_path = self.runtime_dir.join("check-xray.json");
            write_private_file(&check_path, xcfg)?;

            let check_out = Command::new(&self.xray_bin)
                .arg("run")
                .arg("-test")
                .arg("-c")
                .arg(&check_path)
                .output()
                .await
                .with_context(|| {
                    format!(
                        "failed to execute '{}' (install xray-core to use XHTTP / VLESS Encryption)",
                        self.xray_bin
                    )
                })?;

            let _ = fs::remove_file(&check_path);

            if !check_out.status.success() {
                let stderr = String::from_utf8_lossy(&check_out.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&check_out.stdout).trim().to_string();
                let detail = if !stderr.is_empty() { stderr } else { stdout };
                bail!("xray config check failed: {detail}");
            }
        }

        // 2. Validate sing-box config (if requested)
        if let Some(ref sbcfg) = config_json {
            let check_path = self.runtime_dir.join("check-config.json");
            write_private_file(&check_path, sbcfg)?;

            let check_out = Command::new(&self.singbox_bin)
                .arg("check")
                .arg("-c")
                .arg(&check_path)
                .output()
                .await
                .with_context(|| {
                    format!(
                        "failed to execute '{}' (is sing-box installed?)",
                        self.singbox_bin
                    )
                })?;

            let _ = fs::remove_file(&check_path);

            if !check_out.status.success() {
                let stderr = String::from_utf8_lossy(&check_out.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&check_out.stdout).trim().to_string();
                let detail = if !stderr.is_empty() { stderr } else { stdout };
                bail!("sing-box config check failed: {detail}");
            }
        }

        // 3. Stop previous instance cleanly before starting new core(s)
        self.stop().await?;

        let label = engine_label.unwrap_or_else(|| match (&xray_config_json, &config_json) {
            (Some(_), Some(_)) => "xray+tun".to_string(),
            (Some(_), None) => "xray".to_string(),
            _ => "sing-box".to_string(),
        });

        Self::push_log(
            &self.logs,
            format!(
                "[omavpnd] starting {label} for '{}' ({})",
                profile_name,
                mode.as_str()
            ),
        )
        .await;

        // 4. Spawn xray first so its local SOCKS port is ready before sing-box TUN attaches
        let mut xray_proc = None;
        if let Some(xcfg) = xray_config_json {
            let config_path = self.runtime_dir.join("runtime-xray.json");
            write_private_file(&config_path, &xcfg)?;

            let mut child = Command::new(&self.xray_bin)
                .arg("run")
                .arg("-c")
                .arg(&config_path)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .with_context(|| format!("failed to spawn '{}'", self.xray_bin))?;

            self.attach_log_readers(&mut child, "[xray] ");

            tokio::time::sleep(Duration::from_millis(250)).await;
            if let Some(exit_status) = child.try_wait()? {
                let _ = fs::remove_file(&config_path);
                let tail = self.recent_logs(5).await.join(" | ");
                return Err(anyhow!("xray terminated immediately ({exit_status}): {tail}"));
            }

            xray_proc = Some(RunningProcess {
                label: "xray",
                child,
                config_path,
            });
        }

        // 5. Spawn sing-box (standalone or TUN bridge)
        let mut singbox_proc = None;
        if let Some(sbcfg) = config_json {
            let config_path = self.runtime_dir.join("runtime-config.json");
            write_private_file(&config_path, &sbcfg)?;

            let spawn_res = Command::new(&self.singbox_bin)
                .arg("run")
                .arg("-c")
                .arg(&config_path)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn();

            let mut child = match spawn_res {
                Ok(c) => c,
                Err(err) => {
                    if let Some(xr) = xray_proc.take() {
                        stop_process(xr).await;
                    }
                    let _ = fs::remove_file(&config_path);
                    return Err(err).with_context(|| format!("failed to spawn '{}'", self.singbox_bin));
                }
            };

            self.attach_log_readers(&mut child, "");

            tokio::time::sleep(Duration::from_millis(250)).await;
            if let Some(exit_status) = child.try_wait()? {
                if let Some(xr) = xray_proc.take() {
                    stop_process(xr).await;
                }
                let _ = fs::remove_file(&config_path);
                let tail = self.recent_logs(5).await.join(" | ");
                return Err(anyhow!(
                    "sing-box terminated immediately ({exit_status}): {tail}"
                ));
            }

            singbox_proc = Some(RunningProcess {
                label: "sing-box",
                child,
                config_path,
            });
        }

        let primary_pid = singbox_proc
            .as_ref()
            .and_then(|p| p.child.id())
            .or_else(|| xray_proc.as_ref().and_then(|p| p.child.id()));

        info!(
            "core started (engine={}, pid={:?}, profile={}, mode={:?})",
            label, primary_pid, profile_name, mode
        );

        let mut state = self.inner.lock().await;
        state.singbox = singbox_proc;
        state.xray = xray_proc;
        state.status = DaemonStatus {
            running: true,
            active_profile_id: Some(profile_id),
            active_profile_name: Some(profile_name),
            mode: Some(mode),
            engine: Some(label),
            mixed_port: Some(mixed_port),
            pid: primary_pid,
            started_at: Some(Utc::now()),
            last_error: None,
        };

        Ok(state.status.clone())
    }

    fn attach_log_readers(&self, child: &mut Child, prefix: &'static str) {
        if let Some(stdout) = child.stdout.take() {
            let logs = Arc::clone(&self.logs);
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    Self::push_log(&logs, format!("{prefix}{line}")).await;
                }
            });
        }

        if let Some(stderr) = child.stderr.take() {
            let logs = Arc::clone(&self.logs);
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    Self::push_log(&logs, format!("{prefix}{line}")).await;
                }
            });
        }
    }

    pub async fn stop(&self) -> Result<DaemonStatus> {
        let (sb, xr) = {
            let mut state = self.inner.lock().await;
            state.status.running = false;
            state.status.pid = None;
            state.status.started_at = None;
            (state.singbox.take(), state.xray.take())
        };

        // Stop sing-box first so TUN routes are torn down before stopping xray
        if let Some(proc) = sb {
            Self::push_log(&self.logs, "[omavpnd] stopping sing-box...".to_string()).await;
            stop_process(proc).await;
        }
        if let Some(proc) = xr {
            Self::push_log(&self.logs, "[omavpnd] stopping xray...".to_string()).await;
            stop_process(proc).await;
        }

        let state = self.inner.lock().await;
        Ok(state.status.clone())
    }
}

async fn stop_process(mut proc: RunningProcess) {
    info!("stopping {} (pid={:?})", proc.label, proc.child.id());

    if let Some(pid) = proc.child.id() {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status()
            .await;
    }

    let waited = timeout(Duration::from_secs(5), proc.child.wait()).await;
    match waited {
        Ok(Ok(status)) => info!("{} stopped cleanly ({status})", proc.label),
        Ok(Err(err)) => error!("error waiting for {}: {err}", proc.label),
        Err(_) => {
            warn!("{} did not exit in 5s, sending SIGKILL", proc.label);
            let _ = proc.child.kill().await;
            let _ = proc.child.wait().await;
        }
    }

    let _ = fs::remove_file(&proc.config_path);
}

fn write_private_file(path: &PathBuf, content: &str) -> Result<()> {
    fs::write(path, content)
        .with_context(|| format!("failed to write {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to chmod 0600 {}", path.display()))?;
    Ok(())
}

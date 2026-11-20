use serde::{Deserialize, Serialize};

use crate::models::{DaemonStatus, RunMode};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum DaemonRequest {
    Status,
    Start {
        profile_id: String,
        profile_name: String,
        mode: RunMode,
        mixed_port: u16,
        /// Optional sing-box config JSON (standalone or TUN bridge to xray)
        #[serde(default)]
        config_json: Option<String>,
        /// Optional xray-core config JSON (used for XHTTP / VLESS Encryption / forced Xray)
        #[serde(default)]
        xray_config_json: Option<String>,
        /// Human-readable engine label ("sing-box", "xray+tun", "xray")
        #[serde(default)]
        engine_label: Option<String>,
    },
    Stop,
    Logs {
        #[serde(default = "default_log_limit")]
        limit: usize,
    },
}

fn default_log_limit() -> usize {
    200
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonResponse {
    Ok {
        status: DaemonStatus,
    },
    Logs {
        lines: Vec<String>,
    },
    Error {
        message: String,
    },
}

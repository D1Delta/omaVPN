use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Vless,
    Vmess,
    Trojan,
    Shadowsocks,
    Hysteria2,
    Socks,
    Http,
}

impl Protocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            Protocol::Vless => "VLESS",
            Protocol::Vmess => "VMess",
            Protocol::Trojan => "Trojan",
            Protocol::Shadowsocks => "SS",
            Protocol::Hysteria2 => "Hy2",
            Protocol::Socks => "SOCKS",
            Protocol::Http => "HTTP",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TlsSettings {
    pub enabled: bool,
    pub server_name: Option<String>,
    pub insecure: bool,
    pub alpn: Vec<String>,
    pub fingerprint: Option<String>,
    pub reality_public_key: Option<String>,
    pub reality_short_id: Option<String>,
    #[serde(default)]
    pub spider_x: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransportSettings {
    /// "tcp" | "ws" | "grpc" | "httpupgrade" | "http" | "xhttp"
    pub network: Option<String>,
    pub host: Option<String>,
    pub path: Option<String>,
    pub service_name: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub xhttp_extra: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub remarks: String,
    pub protocol: Protocol,
    pub server: String,
    pub port: u16,
    /// UUID (VLESS/VMess) or password (Trojan/SS/Hy2/SOCKS/HTTP)
    pub secret: String,
    pub username: Option<String>,
    /// Cipher method (SS/VMess) or encryption (VLESS)
    pub method: Option<String>,
    pub flow: Option<String>,
    pub alter_id: Option<u16>,
    /// Hysteria2 obfs type ("salamander") & password
    pub obfs: Option<String>,
    pub obfs_password: Option<String>,
    pub up_mbps: Option<u32>,
    pub down_mbps: Option<u32>,
    #[serde(default)]
    pub tls: TlsSettings,
    #[serde(default)]
    pub transport: TransportSettings,
    pub subscription_id: Option<String>,
    pub latency_ms: Option<i64>,
    pub raw_uri: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Profile {
    pub fn new(remarks: String, protocol: Protocol, server: String, port: u16, secret: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            remarks,
            protocol,
            server,
            port,
            secret,
            username: None,
            method: None,
            flow: None,
            alter_id: None,
            obfs: None,
            obfs_password: None,
            up_mbps: None,
            down_mbps: None,
            tls: TlsSettings::default(),
            transport: TransportSettings::default(),
            subscription_id: None,
            latency_ms: None,
            raw_uri: None,
            created_at: Utc::now(),
        }
    }

    pub fn display_name(&self) -> &str {
        if self.remarks.trim().is_empty() {
            &self.server
        } else {
            &self.remarks
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub name: String,
    pub url: String,
    pub enabled: bool,
    #[serde(default)]
    pub user_agent: Option<String>,
    pub last_updated: Option<DateTime<Utc>>,
}

impl Subscription {
    pub fn new(name: String, url: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            url,
            enabled: true,
            user_agent: None,
            last_updated: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunMode {
    Tun,
    ProxyOnly,
}

impl RunMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunMode::Tun => "TUN (System)",
            RunMode::ProxyOnly => "Proxy-only",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            RunMode::Tun => RunMode::ProxyOnly,
            RunMode::ProxyOnly => RunMode::Tun,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoutingPreset {
    Global,
    BypassLan,
}

impl RoutingPreset {
    pub fn as_str(&self) -> &'static str {
        match self {
            RoutingPreset::Global => "Global",
            RoutingPreset::BypassLan => "Bypass LAN",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            RoutingPreset::Global => RoutingPreset::BypassLan,
            RoutingPreset::BypassLan => RoutingPreset::Global,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoreEngine {
    Auto,
    SingBox,
    Xray,
}

impl Default for CoreEngine {
    fn default() -> Self {
        CoreEngine::Auto
    }
}

impl CoreEngine {
    pub fn as_str(&self) -> &'static str {
        match self {
            CoreEngine::Auto => "Auto",
            CoreEngine::SingBox => "sing-box",
            CoreEngine::Xray => "xray",
        }
    }

    pub fn next(self) -> Self {
        match self {
            CoreEngine::Auto => CoreEngine::SingBox,
            CoreEngine::SingBox => CoreEngine::Xray,
            CoreEngine::Xray => CoreEngine::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub mode: RunMode,
    pub routing: RoutingPreset,
    #[serde(default)]
    pub engine: CoreEngine,
    pub tun_interface: String,
    pub tun_mtu: u32,
    pub mixed_port: u16,
    pub ipv6_enabled: bool,
    pub remote_dns: String,
    pub local_dns: String,
    pub log_level: String,
    pub selected_profile_id: Option<String>,
    #[serde(default)]
    pub direct_domains: Vec<String>,
    #[serde(default)]
    pub direct_cidrs: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            mode: RunMode::Tun,
            routing: RoutingPreset::BypassLan,
            engine: CoreEngine::Auto,
            tun_interface: crate::DEFAULT_TUN_INTERFACE.to_string(),
            tun_mtu: 1400,
            mixed_port: 10808,
            ipv6_enabled: false,
            remote_dns: "https://1.1.1.1/dns-query".to_string(),
            local_dns: "local".to_string(),
            log_level: "info".to_string(),
            selected_profile_id: None,
            direct_domains: Vec::new(),
            direct_cidrs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub running: bool,
    pub active_profile_id: Option<String>,
    pub active_profile_name: Option<String>,
    pub mode: Option<RunMode>,
    #[serde(default)]
    pub engine: Option<String>,
    pub mixed_port: Option<u16>,
    pub pid: Option<u32>,
    pub started_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

impl Default for DaemonStatus {
    fn default() -> Self {
        Self {
            running: false,
            active_profile_id: None,
            active_profile_name: None,
            mode: None,
            engine: None,
            mixed_port: None,
            pid: None,
            started_at: None,
            last_error: None,
        }
    }
}

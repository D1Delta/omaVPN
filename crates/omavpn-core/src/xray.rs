use anyhow::{bail, Result};
use serde_json::{json, Map, Value};

use crate::models::{AppSettings, CoreEngine, Profile, Protocol};
use crate::parser;

/// Returns true if this profile uses Xray-only features (XHTTP or VLESS Encryption).
pub fn profile_requires_xray(profile: &Profile) -> bool {
    let is_xhttp = profile
        .transport
        .network
        .as_deref()
        .map(|n| n.eq_ignore_ascii_case("xhttp") || n.eq_ignore_ascii_case("splithttp"))
        .unwrap_or(false);

    let has_vless_encryption = profile.protocol == Protocol::Vless
        && profile
            .method
            .as_deref()
            .map(|m| {
                let t = m.trim();
                !t.is_empty() && !t.eq_ignore_ascii_case("none")
            })
            .unwrap_or(false);

    is_xhttp || has_vless_encryption
}

/// Decides whether to run Xray for the given profile and engine preference.
pub fn should_use_xray(profile: &Profile, engine: CoreEngine) -> bool {
    match engine {
        CoreEngine::Xray => true,
        CoreEngine::SingBox => false,
        CoreEngine::Auto => profile_requires_xray(profile),
    }
}

/// Re-hydrate newly added transport/tls fields (`mode`, `xhttp_extra`, `spider_x`)
/// from `raw_uri` if the profile was persisted before those fields existed.
fn hydrate_from_raw_uri(profile: &Profile) -> Profile {
    if profile.transport.mode.is_none()
        && profile.transport.xhttp_extra.is_none()
        && profile.tls.spider_x.is_none()
    {
        if let Some(raw) = &profile.raw_uri {
            if let Ok(reparsed) = parser::parse_uri(raw) {
                let mut merged = profile.clone();
                merged.transport.mode = reparsed.transport.mode;
                merged.transport.xhttp_extra = reparsed.transport.xhttp_extra;
                merged.tls.spider_x = reparsed.tls.spider_x;
                return merged;
            }
        }
    }
    profile.clone()
}

/// Builds an Xray-core JSON configuration exposing a local SOCKS5 inbound on `socks_port`.
pub fn build_config(profile: &Profile, settings: &AppSettings, socks_port: u16) -> Result<Value> {
    let profile = hydrate_from_raw_uri(profile);
    let outbound = build_outbound(&profile)?;

    let log_level = match settings.log_level.as_str() {
        "debug" | "info" | "warning" | "error" | "none" => settings.log_level.as_str(),
        _ => "warning",
    };

    Ok(json!({
        "log": {
            "loglevel": log_level
        },
        "inbounds": [
            {
                "tag": "socks-in",
                "listen": "127.0.0.1",
                "port": socks_port,
                "protocol": "socks",
                "settings": {
                    "auth": "noauth",
                    "udp": true
                },
                "sniffing": {
                    "enabled": true,
                    "destOverride": ["http", "tls", "quic"]
                }
            }
        ],
        "outbounds": [
            outbound,
            {
                "tag": "direct",
                "protocol": "freedom"
            },
            {
                "tag": "block",
                "protocol": "blackhole"
            }
        ]
    }))
}

pub fn build_config_pretty(
    profile: &Profile,
    settings: &AppSettings,
    socks_port: u16,
) -> Result<String> {
    let val = build_config(profile, settings, socks_port)?;
    Ok(serde_json::to_string_pretty(&val)?)
}

fn build_outbound(profile: &Profile) -> Result<Value> {
    if profile.server.trim().is_empty() {
        bail!("profile server address is empty");
    }

    let mut out = Map::new();
    out.insert("tag".into(), Value::String("proxy".into()));

    match profile.protocol {
        Protocol::Vless => {
            out.insert("protocol".into(), Value::String("vless".into()));
            let encryption = profile
                .method
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("none");

            let mut user = Map::new();
            user.insert("id".into(), Value::String(profile.secret.clone()));
            user.insert("encryption".into(), Value::String(encryption.to_string()));
            if let Some(flow) = &profile.flow {
                if !flow.is_empty() {
                    user.insert("flow".into(), Value::String(flow.clone()));
                }
            }

            out.insert(
                "settings".into(),
                json!({
                    "vnext": [{
                        "address": profile.server,
                        "port": profile.port,
                        "users": [Value::Object(user)]
                    }]
                }),
            );
            out.insert("streamSettings".into(), build_stream_settings(profile));
        }
        Protocol::Vmess => {
            out.insert("protocol".into(), Value::String("vmess".into()));
            let security = profile.method.as_deref().unwrap_or("auto");
            out.insert(
                "settings".into(),
                json!({
                    "vnext": [{
                        "address": profile.server,
                        "port": profile.port,
                        "users": [{
                            "id": profile.secret,
                            "alterId": profile.alter_id.unwrap_or(0),
                            "security": security
                        }]
                    }]
                }),
            );
            out.insert("streamSettings".into(), build_stream_settings(profile));
        }
        Protocol::Trojan => {
            out.insert("protocol".into(), Value::String("trojan".into()));
            out.insert(
                "settings".into(),
                json!({
                    "servers": [{
                        "address": profile.server,
                        "port": profile.port,
                        "password": profile.secret
                    }]
                }),
            );
            out.insert("streamSettings".into(), build_stream_settings(profile));
        }
        Protocol::Shadowsocks => {
            out.insert("protocol".into(), Value::String("shadowsocks".into()));
            let method = profile
                .method
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("aes-256-gcm");
            out.insert(
                "settings".into(),
                json!({
                    "servers": [{
                        "address": profile.server,
                        "port": profile.port,
                        "method": method,
                        "password": profile.secret
                    }]
                }),
            );
        }
        Protocol::Socks => {
            out.insert("protocol".into(), Value::String("socks".into()));
            let mut srv = Map::new();
            srv.insert("address".into(), Value::String(profile.server.clone()));
            srv.insert("port".into(), Value::Number(profile.port.into()));
            if let Some(u) = &profile.username {
                srv.insert(
                    "users".into(),
                    json!([{ "user": u, "pass": profile.secret }]),
                );
            }
            out.insert("settings".into(), json!({ "servers": [Value::Object(srv)] }));
        }
        Protocol::Http => {
            out.insert("protocol".into(), Value::String("http".into()));
            let mut srv = Map::new();
            srv.insert("address".into(), Value::String(profile.server.clone()));
            srv.insert("port".into(), Value::Number(profile.port.into()));
            if let Some(u) = &profile.username {
                srv.insert(
                    "users".into(),
                    json!([{ "user": u, "pass": profile.secret }]),
                );
            }
            out.insert("settings".into(), json!({ "servers": [Value::Object(srv)] }));
        }
        Protocol::Hysteria2 => {
            bail!("Hysteria2 is handled by sing-box — switch Core Engine to Auto or sing-box");
        }
    }

    Ok(Value::Object(out))
}

fn build_stream_settings(profile: &Profile) -> Value {
    let mut ss = Map::new();
    let net = profile
        .transport
        .network
        .as_deref()
        .unwrap_or("tcp")
        .to_ascii_lowercase();

    let xray_net = match net.as_str() {
        "splithttp" => "xhttp",
        "h2" | "http" => "http",
        other => other,
    };
    ss.insert("network".into(), Value::String(xray_net.to_string()));

    match xray_net {
        "xhttp" => {
            let mut xh = Map::new();
            if let Some(host) = &profile.transport.host {
                xh.insert("host".into(), Value::String(host.clone()));
            }
            xh.insert(
                "path".into(),
                Value::String(
                    profile
                        .transport
                        .path
                        .clone()
                        .unwrap_or_else(|| "/".to_string()),
                ),
            );
            if let Some(mode) = &profile.transport.mode {
                xh.insert("mode".into(), Value::String(mode.clone()));
            }
            if let Some(extra_raw) = &profile.transport.xhttp_extra {
                if let Ok(parsed) = serde_json::from_str::<Value>(extra_raw) {
                    xh.insert("extra".into(), parsed);
                }
            }
            ss.insert("xhttpSettings".into(), Value::Object(xh));
        }
        "ws" => {
            let mut ws = Map::new();
            ws.insert(
                "path".into(),
                Value::String(
                    profile
                        .transport
                        .path
                        .clone()
                        .unwrap_or_else(|| "/".to_string()),
                ),
            );
            if let Some(host) = &profile.transport.host {
                ws.insert("host".into(), Value::String(host.clone()));
            }
            ss.insert("wsSettings".into(), Value::Object(ws));
        }
        "httpupgrade" => {
            let mut hu = Map::new();
            hu.insert(
                "path".into(),
                Value::String(
                    profile
                        .transport
                        .path
                        .clone()
                        .unwrap_or_else(|| "/".to_string()),
                ),
            );
            if let Some(host) = &profile.transport.host {
                hu.insert("host".into(), Value::String(host.clone()));
            }
            ss.insert("httpupgradeSettings".into(), Value::Object(hu));
        }
        "grpc" => {
            let mut grpc = Map::new();
            if let Some(svc) = &profile.transport.service_name {
                grpc.insert("serviceName".into(), Value::String(svc.clone()));
            }
            if let Some(auth) = &profile.transport.host {
                grpc.insert("authority".into(), Value::String(auth.clone()));
            }
            if profile
                .transport
                .mode
                .as_deref()
                .map(|m| m.eq_ignore_ascii_case("multi"))
                .unwrap_or(false)
            {
                grpc.insert("multiMode".into(), Value::Bool(true));
            }
            ss.insert("grpcSettings".into(), Value::Object(grpc));
        }
        _ => {}
    }

    if profile.tls.enabled {
        let is_reality = profile.tls.reality_public_key.is_some();
        if is_reality {
            ss.insert("security".into(), Value::String("reality".into()));
            let mut r = Map::new();
            if let Some(sni) = &profile.tls.server_name {
                r.insert("serverName".into(), Value::String(sni.clone()));
            }
            if let Some(fp) = &profile.tls.fingerprint {
                r.insert("fingerprint".into(), Value::String(fp.clone()));
            }
            if let Some(pbk) = &profile.tls.reality_public_key {
                r.insert("publicKey".into(), Value::String(pbk.clone()));
            }
            if let Some(sid) = &profile.tls.reality_short_id {
                r.insert("shortId".into(), Value::String(sid.clone()));
            }
            if let Some(spx) = &profile.tls.spider_x {
                r.insert("spiderX".into(), Value::String(spx.clone()));
            }
            ss.insert("realitySettings".into(), Value::Object(r));
        } else {
            ss.insert("security".into(), Value::String("tls".into()));
            let mut t = Map::new();
            if let Some(sni) = &profile.tls.server_name {
                t.insert("serverName".into(), Value::String(sni.clone()));
            }
            if profile.tls.insecure {
                t.insert("allowInsecure".into(), Value::Bool(true));
            }
            if let Some(fp) = &profile.tls.fingerprint {
                t.insert("fingerprint".into(), Value::String(fp.clone()));
            }
            if !profile.tls.alpn.is_empty() {
                t.insert(
                    "alpn".into(),
                    Value::Array(
                        profile
                            .tls
                            .alpn
                            .iter()
                            .cloned()
                            .map(Value::String)
                            .collect(),
                    ),
                );
            }
            ss.insert("tlsSettings".into(), Value::Object(t));
        }
    } else {
        ss.insert("security".into(), Value::String("none".into()));
    }

    let mut sockopt = Map::new();
    if let Some(iface) = detect_default_interface() {
        sockopt.insert("interface".into(), Value::String(iface));
    }
    if !sockopt.is_empty() {
        ss.insert("sockopt".into(), Value::Object(sockopt));
    }

    Value::Object(ss)
}

/// Detects the default non-tunnel network interface name (e.g. "eth0", "wlan0")
/// from /proc/net/route on Linux systems.
pub fn detect_default_interface() -> Option<String> {
    let content = std::fs::read_to_string("/proc/net/route").ok()?;
    for line in content.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() >= 2 && cols[1] == "00000000" {
            let iface = cols[0];
            if !iface.starts_with("omavpn") && !iface.starts_with("tun") {
                return Some(iface.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_xhttp_vless_encryption_config() {
        let uri = "vless://11111111-2222-3333-4444-555555555555@example.com:8008?encryption=mlkem768x25519plus.native.1rtt.TESTKEY&type=xhttp&path=%2Fupload&host=cdn.example.com&mode=stream-up&security=none#NL";
        let profile = parser::parse_uri(uri).unwrap();
        assert!(profile_requires_xray(&profile));

        let settings = AppSettings::default();
        let cfg = build_config(&profile, &settings, 10809).unwrap();
        let ob = &cfg["outbounds"][0];
        assert_eq!(ob["protocol"], "vless");
        assert_eq!(
            ob["settings"]["vnext"][0]["users"][0]["encryption"],
            "mlkem768x25519plus.native.1rtt.TESTKEY"
        );
        assert_eq!(ob["streamSettings"]["network"], "xhttp");
        assert_eq!(ob["streamSettings"]["xhttpSettings"]["mode"], "stream-up");
        assert_eq!(ob["streamSettings"]["xhttpSettings"]["path"], "/upload");
        assert_eq!(
            ob["streamSettings"]["xhttpSettings"]["host"],
            "cdn.example.com"
        );
    }
}

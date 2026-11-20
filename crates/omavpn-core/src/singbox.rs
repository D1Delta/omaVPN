use anyhow::{bail, Result};
use serde_json::{json, Map, Value};
use url::Url;

use crate::models::{AppSettings, Profile, Protocol, RoutingPreset, RunMode};

/// Generate a complete sing-box 1.12–1.14+ JSON configuration for the given profile and settings.
pub fn build_config(profile: &Profile, settings: &AppSettings) -> Result<Value> {
    let proxy_outbound = build_proxy_outbound(profile)?;

    let mut inbounds = Vec::new();

    if settings.mode == RunMode::Tun {
        let mut tun_addrs = vec!["172.19.0.1/30"];
        if settings.ipv6_enabled {
            tun_addrs.push("fdfe:dcba:9876::1/126");
        }
        inbounds.push(json!({
            "type": "tun",
            "tag": "tun-in",
            "interface_name": settings.tun_interface,
            "address": tun_addrs,
            "mtu": settings.tun_mtu,
            "auto_route": true,
            "strict_route": false,
            "stack": "gvisor"
        }));
    }

    inbounds.push(json!({
        "type": "mixed",
        "tag": "mixed-in",
        "listen": "127.0.0.1",
        "listen_port": settings.mixed_port
    }));

    let mut route_rules = vec![
        json!({ "action": "sniff" }),
        json!({
            "protocol": "dns",
            "action": "hijack-dns"
        }),
    ];

    if settings.routing == RoutingPreset::BypassLan {
        route_rules.push(json!({
            "ip_is_private": true,
            "outbound": "direct"
        }));
    }

    if !settings.direct_domains.is_empty() {
        route_rules.push(json!({
            "domain_suffix": settings.direct_domains,
            "outbound": "direct"
        }));
    }

    if !settings.direct_cidrs.is_empty() {
        route_rules.push(json!({
            "ip_cidr": settings.direct_cidrs,
            "outbound": "direct"
        }));
    }

    let dns_strategy = if settings.ipv6_enabled {
        "prefer_ipv4"
    } else {
        "ipv4_only"
    };

    let remote_dns_server = build_dns_server("remote", &settings.remote_dns, Some("proxy"));
    let local_dns_server = build_dns_server("local", &settings.local_dns, None);

    let config = json!({
        "log": {
            "level": settings.log_level,
            "timestamp": true
        },
        "dns": {
            "servers": [
                remote_dns_server,
                local_dns_server
            ],
            "final": "remote",
            "strategy": dns_strategy
        },
        "inbounds": inbounds,
        "outbounds": [
            proxy_outbound,
            {
                "type": "direct",
                "tag": "direct"
            }
        ],
        "route": {
            "auto_detect_interface": true,
            "default_domain_resolver": "local",
            "rules": route_rules,
            "final": "proxy"
        }
    });

    Ok(config)
}

pub fn build_config_pretty(profile: &Profile, settings: &AppSettings) -> Result<String> {
    let val = build_config(profile, settings)?;
    Ok(serde_json::to_string_pretty(&val)?)
}

/// Builds a sing-box 1.14+ TUN + Mixed bridge config that forwards all traffic into a local
/// Xray SOCKS5 inbound (`127.0.0.1:upstream_socks_port`) while bypassing `bypass_server_host`
/// to prevent routing/DNS loops.
pub fn build_tun_bridge_config_pretty(
    settings: &AppSettings,
    upstream_socks_port: u16,
    bypass_server_host: &str,
) -> Result<String> {
    let mut tun_addrs = vec!["172.19.0.1/30"];
    if settings.ipv6_enabled {
        tun_addrs.push("fdfe:dcba:9876::1/126");
    }

    let inbounds = vec![
        json!({
            "type": "tun",
            "tag": "tun-in",
            "interface_name": settings.tun_interface,
            "address": tun_addrs,
            "mtu": settings.tun_mtu,
            "auto_route": true,
            "strict_route": false,
            "stack": "gvisor"
        }),
        json!({
            "type": "mixed",
            "tag": "mixed-in",
            "listen": "127.0.0.1",
            "listen_port": settings.mixed_port
        }),
    ];

    let mut route_rules = vec![
        json!({ "action": "sniff" }),
        json!({
            "protocol": "dns",
            "action": "hijack-dns"
        }),
    ];

    let mut dns_rules = Vec::new();
    let host = bypass_server_host.trim();
    if !host.is_empty() {
        let mut cidrs = Vec::new();
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            match ip {
                std::net::IpAddr::V4(_) => cidrs.push(format!("{ip}/32")),
                std::net::IpAddr::V6(_) => cidrs.push(format!("{ip}/128")),
            };
        } else {
            route_rules.push(json!({
                "domain": [host],
                "outbound": "direct"
            }));
            dns_rules.push(json!({
                "domain": [host],
                "server": "local"
            }));

            // Resolve host to IP addresses and add them to direct route rules
            // to prevent Xray's raw outbound TCP connections from looping into TUN
            use std::net::ToSocketAddrs;
            if let Ok(iter) = format!("{host}:80").to_socket_addrs() {
                for sa in iter {
                    match sa.ip() {
                        std::net::IpAddr::V4(v4) => {
                            cidrs.push(format!("{v4}/32"));
                            let oct = v4.octets();
                            cidrs.push(format!("{}.{}.{}.0/24", oct[0], oct[1], oct[2]));
                        }
                        std::net::IpAddr::V6(v6) => {
                            cidrs.push(format!("{v6}/128"));
                        }
                    }
                }
            }
        }

        if !cidrs.is_empty() {
            route_rules.push(json!({
                "ip_cidr": cidrs,
                "outbound": "direct"
            }));
        }
    }

    // Always keep private/loopback traffic direct so 127.0.0.1:upstream_socks_port never loops
    route_rules.push(json!({
        "ip_is_private": true,
        "outbound": "direct"
    }));

    if !settings.direct_domains.is_empty() {
        route_rules.push(json!({
            "domain_suffix": settings.direct_domains,
            "outbound": "direct"
        }));
    }

    if !settings.direct_cidrs.is_empty() {
        route_rules.push(json!({
            "ip_cidr": settings.direct_cidrs,
            "outbound": "direct"
        }));
    }

    let dns_strategy = if settings.ipv6_enabled {
        "prefer_ipv4"
    } else {
        "ipv4_only"
    };

    let remote_dns_server = build_dns_server("remote", &settings.remote_dns, Some("proxy"));
    let local_dns_server = build_dns_server("local", &settings.local_dns, None);

    let config = json!({
        "log": {
            "level": settings.log_level,
            "timestamp": true
        },
        "dns": {
            "servers": [
                remote_dns_server,
                local_dns_server
            ],
            "rules": dns_rules,
            "final": "remote",
            "strategy": dns_strategy
        },
        "inbounds": inbounds,
        "outbounds": [
            {
                "type": "socks",
                "tag": "proxy",
                "server": "127.0.0.1",
                "server_port": upstream_socks_port
            },
            {
                "type": "direct",
                "tag": "direct"
            }
        ],
        "route": {
            "auto_detect_interface": true,
            "default_domain_resolver": "local",
            "rules": route_rules,
            "final": "proxy"
        }
    });

    Ok(serde_json::to_string_pretty(&config)?)
}

/// Converts a user-facing DNS address (`local`, `1.1.1.1`, `https://1.1.1.1/dns-query`, `tls://1.1.1.1`)
/// into the typed sing-box 1.12+ / 1.14+ DNS server object.
fn build_dns_server(tag: &str, address: &str, detour: Option<&str>) -> Value {
    let trimmed = address.trim();
    let lower = trimmed.to_ascii_lowercase();
    let mut obj = Map::new();
    obj.insert("tag".into(), Value::String(tag.into()));

    if lower.is_empty() || lower == "local" || lower == "system" {
        obj.insert("type".into(), Value::String("local".into()));
        return Value::Object(obj);
    }

    if let Ok(url) = Url::parse(trimmed) {
        match url.scheme() {
            "https" => {
                obj.insert("type".into(), Value::String("https".into()));
                obj.insert(
                    "server".into(),
                    Value::String(url.host_str().unwrap_or("1.1.1.1").into()),
                );
                obj.insert(
                    "server_port".into(),
                    Value::Number(url.port().unwrap_or(443).into()),
                );
                let path = if url.path().is_empty() || url.path() == "/" {
                    "/dns-query"
                } else {
                    url.path()
                };
                obj.insert("path".into(), Value::String(path.into()));
            }
            "tls" => {
                obj.insert("type".into(), Value::String("tls".into()));
                obj.insert(
                    "server".into(),
                    Value::String(url.host_str().unwrap_or("1.1.1.1").into()),
                );
                obj.insert(
                    "server_port".into(),
                    Value::Number(url.port().unwrap_or(853).into()),
                );
            }
            "tcp" => {
                obj.insert("type".into(), Value::String("tcp".into()));
                obj.insert(
                    "server".into(),
                    Value::String(url.host_str().unwrap_or("1.1.1.1").into()),
                );
                obj.insert(
                    "server_port".into(),
                    Value::Number(url.port().unwrap_or(53).into()),
                );
            }
            _ => {
                obj.insert("type".into(), Value::String("udp".into()));
                obj.insert(
                    "server".into(),
                    Value::String(url.host_str().unwrap_or("1.1.1.1").into()),
                );
                obj.insert(
                    "server_port".into(),
                    Value::Number(url.port().unwrap_or(53).into()),
                );
            }
        }
    } else {
        obj.insert("type".into(), Value::String("udp".into()));
        obj.insert("server".into(), Value::String(trimmed.into()));
        obj.insert("server_port".into(), Value::Number(53u16.into()));
    }

    if let Some(d) = detour {
        obj.insert("detour".into(), Value::String(d.into()));
    }

    Value::Object(obj)
}

fn build_proxy_outbound(profile: &Profile) -> Result<Value> {
    let mut obj = Map::new();
    obj.insert("tag".into(), Value::String("proxy".into()));
    obj.insert("server".into(), Value::String(profile.server.clone()));
    obj.insert("server_port".into(), Value::Number(profile.port.into()));

    match profile.protocol {
        Protocol::Vless => {
            obj.insert("type".into(), Value::String("vless".into()));
            obj.insert("uuid".into(), Value::String(profile.secret.clone()));
            if let Some(flow) = &profile.flow {
                if !flow.is_empty() {
                    obj.insert("flow".into(), Value::String(flow.clone()));
                }
            }
            obj.insert("packet_encoding".into(), Value::String("xudp".into()));
            attach_tls(&mut obj, profile);
            attach_transport(&mut obj, profile);
        }
        Protocol::Vmess => {
            obj.insert("type".into(), Value::String("vmess".into()));
            obj.insert("uuid".into(), Value::String(profile.secret.clone()));
            obj.insert(
                "security".into(),
                Value::String(
                    profile
                        .method
                        .clone()
                        .unwrap_or_else(|| "auto".to_string()),
                ),
            );
            obj.insert(
                "alter_id".into(),
                Value::Number(profile.alter_id.unwrap_or(0).into()),
            );
            attach_tls(&mut obj, profile);
            attach_transport(&mut obj, profile);
        }
        Protocol::Trojan => {
            obj.insert("type".into(), Value::String("trojan".into()));
            obj.insert("password".into(), Value::String(profile.secret.clone()));
            attach_tls(&mut obj, profile);
            attach_transport(&mut obj, profile);
        }
        Protocol::Shadowsocks => {
            obj.insert("type".into(), Value::String("shadowsocks".into()));
            let method = profile
                .method
                .clone()
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| "aes-256-gcm".to_string());
            obj.insert("method".into(), Value::String(method));
            obj.insert("password".into(), Value::String(profile.secret.clone()));
        }
        Protocol::Hysteria2 => {
            obj.insert("type".into(), Value::String("hysteria2".into()));
            obj.insert("password".into(), Value::String(profile.secret.clone()));
            if let Some(up) = profile.up_mbps {
                obj.insert("up_mbps".into(), Value::Number(up.into()));
            }
            if let Some(down) = profile.down_mbps {
                obj.insert("down_mbps".into(), Value::Number(down.into()));
            }
            if let Some(obfs) = &profile.obfs {
                let mut obfs_obj = Map::new();
                obfs_obj.insert("type".into(), Value::String(obfs.clone()));
                if let Some(pw) = &profile.obfs_password {
                    obfs_obj.insert("password".into(), Value::String(pw.clone()));
                }
                obj.insert("obfs".into(), Value::Object(obfs_obj));
            }
            attach_tls(&mut obj, profile);
        }
        Protocol::Socks => {
            obj.insert("type".into(), Value::String("socks".into()));
            if let Some(user) = &profile.username {
                obj.insert("username".into(), Value::String(user.clone()));
                obj.insert("password".into(), Value::String(profile.secret.clone()));
            }
        }
        Protocol::Http => {
            obj.insert("type".into(), Value::String("http".into()));
            if let Some(user) = &profile.username {
                obj.insert("username".into(), Value::String(user.clone()));
                obj.insert("password".into(), Value::String(profile.secret.clone()));
            }
            attach_tls(&mut obj, profile);
        }
    }

    if profile.server.trim().is_empty() {
        bail!("profile server address is empty");
    }

    Ok(Value::Object(obj))
}

fn attach_tls(obj: &mut Map<String, Value>, profile: &Profile) {
    if !profile.tls.enabled {
        return;
    }

    let mut tls = Map::new();
    tls.insert("enabled".into(), Value::Bool(true));

    if let Some(sni) = &profile.tls.server_name {
        tls.insert("server_name".into(), Value::String(sni.clone()));
    }
    if profile.tls.insecure {
        tls.insert("insecure".into(), Value::Bool(true));
    }
    if !profile.tls.alpn.is_empty() {
        tls.insert(
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
    if let Some(fp) = &profile.tls.fingerprint {
        tls.insert(
            "utls".into(),
            json!({
                "enabled": true,
                "fingerprint": fp
            }),
        );
    }
    if let Some(pbk) = &profile.tls.reality_public_key {
        let mut reality = Map::new();
        reality.insert("enabled".into(), Value::Bool(true));
        reality.insert("public_key".into(), Value::String(pbk.clone()));
        if let Some(sid) = &profile.tls.reality_short_id {
            reality.insert("short_id".into(), Value::String(sid.clone()));
        }
        tls.insert("reality".into(), Value::Object(reality));
    }

    obj.insert("tls".into(), Value::Object(tls));
}

fn attach_transport(obj: &mut Map<String, Value>, profile: &Profile) {
    let Some(net) = profile.transport.network.as_deref() else {
        return;
    };

    match net.to_ascii_lowercase().as_str() {
        "ws" => {
            let mut tr = Map::new();
            tr.insert("type".into(), Value::String("ws".into()));
            tr.insert(
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
                tr.insert("headers".into(), json!({ "Host": host }));
            }
            obj.insert("transport".into(), Value::Object(tr));
        }
        "grpc" => {
            let mut tr = Map::new();
            tr.insert("type".into(), Value::String("grpc".into()));
            if let Some(svc) = &profile.transport.service_name {
                tr.insert("service_name".into(), Value::String(svc.clone()));
            }
            obj.insert("transport".into(), Value::Object(tr));
        }
        "httpupgrade" => {
            let mut tr = Map::new();
            tr.insert("type".into(), Value::String("httpupgrade".into()));
            if let Some(host) = &profile.transport.host {
                tr.insert("host".into(), Value::String(host.clone()));
            }
            if let Some(path) = &profile.transport.path {
                tr.insert("path".into(), Value::String(path.clone()));
            }
            obj.insert("transport".into(), Value::Object(tr));
        }
        "h2" | "http" => {
            let mut tr = Map::new();
            tr.insert("type".into(), Value::String("http".into()));
            if let Some(host) = &profile.transport.host {
                tr.insert("host".into(), json!([host]));
            }
            if let Some(path) = &profile.transport.path {
                tr.insert("path".into(), Value::String(path.clone()));
            }
            obj.insert("transport".into(), Value::Object(tr));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_uri;

    #[test]
    fn builds_singbox_1_14_tun_and_dns() {
        let profile = parse_uri(
            "vless://11111111-2222-3333-4444-555555555555@example.com:443?security=reality&sni=example.com&fp=chrome&pbk=KEY&sid=01#Test",
        )
        .unwrap();
        let settings = AppSettings::default();
        let cfg = build_config(&profile, &settings).unwrap();

        let inbounds = cfg["inbounds"].as_array().unwrap();
        assert_eq!(inbounds.len(), 2);
        assert_eq!(inbounds[0]["type"], "tun");
        assert_eq!(inbounds[1]["type"], "mixed");
        assert_eq!(cfg["dns"]["servers"][0]["type"], "https");
        assert_eq!(cfg["dns"]["servers"][0]["server"], "1.1.1.1");
        assert_eq!(cfg["dns"]["servers"][1]["type"], "local");
        assert_eq!(cfg["route"]["auto_detect_interface"], true);
        assert_eq!(cfg["route"]["default_domain_resolver"], "local");
        assert_eq!(cfg["route"]["rules"][0]["action"], "sniff");
        assert_eq!(cfg["route"]["rules"][1]["action"], "hijack-dns");
    }
}

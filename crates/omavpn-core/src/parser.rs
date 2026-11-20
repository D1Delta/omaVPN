use std::collections::HashMap;

use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine as _;
use serde::Deserialize;
use url::Url;

use crate::models::{Profile, Protocol, TlsSettings, TransportSettings};

/// Parse a raw block of text (plain URI lines or base64-encoded subscription payload).
pub fn parse_batch(input: &str) -> Vec<Profile> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let direct = parse_lines(trimmed);
    if !direct.is_empty() {
        return direct;
    }

    if let Some(decoded) = decode_base64_flexible(trimmed) {
        return parse_lines(&decoded);
    }

    Vec::new()
}

fn parse_lines(text: &str) -> Vec<Profile> {
    let mut profiles = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Ok(profile) = parse_uri(line) {
            profiles.push(profile);
        }
    }
    profiles
}

pub fn parse_uri(uri: &str) -> Result<Profile> {
    let trimmed = uri.trim();
    let lower = trimmed.to_ascii_lowercase();

    let mut profile = if lower.starts_with("vless://") {
        parse_vless(trimmed)?
    } else if lower.starts_with("vmess://") {
        parse_vmess(trimmed)?
    } else if lower.starts_with("trojan://") {
        parse_trojan(trimmed)?
    } else if lower.starts_with("ss://") {
        parse_shadowsocks(trimmed)?
    } else if lower.starts_with("hy2://") || lower.starts_with("hysteria2://") {
        parse_hysteria2(trimmed)?
    } else if lower.starts_with("socks://")
        || lower.starts_with("socks5://")
        || lower.starts_with("socks4://")
    {
        parse_socks_or_http(trimmed, Protocol::Socks)?
    } else if lower.starts_with("http://") || lower.starts_with("https://") {
        parse_socks_or_http(trimmed, Protocol::Http)?
    } else {
        bail!("unsupported URI scheme");
    };

    profile.raw_uri = Some(trimmed.to_string());
    Ok(profile)
}

fn query_map(url: &Url) -> HashMap<String, String> {
    url.query_pairs()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.into_owned()))
        .collect()
}

fn decode_fragment(url: &Url) -> String {
    url.fragment()
        .map(percent_decode_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn percent_decode_str(input: &str) -> String {
    url::form_urlencoded::parse(format!("x={input}").as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| input.to_string())
}

fn clean_host(url: &Url) -> Result<String> {
    let host = url.host_str().ok_or_else(|| anyhow!("missing host"))?;
    Ok(host.trim_start_matches('[').trim_end_matches(']').to_string())
}

fn parse_alpn(raw: Option<&String>) -> Vec<String> {
    raw.map(|s| {
        s.split(',')
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

fn parse_bool_flag(raw: Option<&String>) -> bool {
    matches!(
        raw.map(|s| s.to_ascii_lowercase()).as_deref(),
        Some("1") | Some("true") | Some("yes")
    )
}

fn non_empty(raw: Option<&String>) -> Option<String> {
    raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn fill_common_transport_and_tls(
    params: &HashMap<String, String>,
    default_tls: bool,
) -> (TlsSettings, TransportSettings) {
    let security = params
        .get("security")
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    let tls_enabled = match security.as_str() {
        "tls" | "reality" | "xtls" => true,
        "none" => false,
        _ => default_tls,
    };

    let tls = TlsSettings {
        enabled: tls_enabled,
        server_name: non_empty(params.get("sni").or_else(|| params.get("peer"))),
        insecure: parse_bool_flag(
            params
                .get("allowinsecure")
                .or_else(|| params.get("insecure")),
        ),
        alpn: parse_alpn(params.get("alpn")),
        fingerprint: non_empty(params.get("fp")),
        reality_public_key: non_empty(params.get("pbk")),
        reality_short_id: non_empty(params.get("sid")),
        spider_x: non_empty(params.get("spx")),
    };

    let transport = TransportSettings {
        network: non_empty(params.get("type")),
        host: non_empty(params.get("host").or_else(|| params.get("authority"))),
        path: non_empty(params.get("path")),
        service_name: non_empty(params.get("servicename")),
        mode: non_empty(params.get("mode")),
        xhttp_extra: non_empty(params.get("extra")),
    };

    (tls, transport)
}

fn parse_vless(uri: &str) -> Result<Profile> {
    let url = Url::parse(uri).context("invalid vless URI")?;
    let host = clean_host(&url)?;
    let port = url.port().unwrap_or(443);
    let uuid = percent_decode_str(url.username());
    if uuid.is_empty() {
        bail!("vless URI missing UUID");
    }
    let params = query_map(&url);
    let remarks = decode_fragment(&url);
    let (tls, transport) = fill_common_transport_and_tls(&params, false);

    let mut profile = Profile::new(
        if remarks.is_empty() {
            format!("{host}:{port}")
        } else {
            remarks
        },
        Protocol::Vless,
        host,
        port,
        uuid,
    );
    profile.method = non_empty(params.get("encryption"));
    profile.flow = non_empty(params.get("flow"));
    profile.tls = tls;
    profile.transport = transport;
    Ok(profile)
}

fn parse_trojan(uri: &str) -> Result<Profile> {
    let url = Url::parse(uri).context("invalid trojan URI")?;
    let host = clean_host(&url)?;
    let port = url.port().unwrap_or(443);
    let password = percent_decode_str(url.username());
    if password.is_empty() {
        bail!("trojan URI missing password");
    }
    let params = query_map(&url);
    let remarks = decode_fragment(&url);
    let (tls, transport) = fill_common_transport_and_tls(&params, true);

    let mut profile = Profile::new(
        if remarks.is_empty() {
            format!("{host}:{port}")
        } else {
            remarks
        },
        Protocol::Trojan,
        host,
        port,
        password,
    );
    profile.tls = tls;
    profile.transport = transport;
    Ok(profile)
}

fn parse_hysteria2(uri: &str) -> Result<Profile> {
    let normalized = if uri.to_ascii_lowercase().starts_with("hy2://") {
        format!("hysteria2://{}", &uri[6..])
    } else {
        uri.to_string()
    };
    let url = Url::parse(&normalized).context("invalid hysteria2 URI")?;
    let host = clean_host(&url)?;
    let port = url.port().unwrap_or(443);
    let password = if let Some(pass) = url.password() {
        format!("{}:{}", percent_decode_str(url.username()), percent_decode_str(pass))
    } else {
        percent_decode_str(url.username())
    };
    if password.is_empty() {
        bail!("hysteria2 URI missing auth password");
    }
    let params = query_map(&url);
    let remarks = decode_fragment(&url);

    let mut profile = Profile::new(
        if remarks.is_empty() {
            format!("{host}:{port}")
        } else {
            remarks
        },
        Protocol::Hysteria2,
        host,
        port,
        password,
    );
    profile.obfs = non_empty(params.get("obfs")).filter(|v| !v.eq_ignore_ascii_case("none"));
    profile.obfs_password = non_empty(params.get("obfs-password"));
    profile.up_mbps = params.get("upmbps").and_then(|v| v.parse::<u32>().ok());
    profile.down_mbps = params.get("downmbps").and_then(|v| v.parse::<u32>().ok());
    profile.tls = TlsSettings {
        enabled: true,
        server_name: non_empty(params.get("sni")),
        insecure: parse_bool_flag(params.get("insecure")),
        alpn: parse_alpn(params.get("alpn")),
        fingerprint: None,
        reality_public_key: None,
        reality_short_id: None,
        spider_x: None,
    };
    Ok(profile)
}

#[derive(Debug, Deserialize)]
struct VmessJson {
    #[serde(default)]
    ps: String,
    #[serde(default)]
    add: String,
    #[serde(default)]
    port: serde_json::Value,
    #[serde(default)]
    id: String,
    #[serde(default)]
    aid: serde_json::Value,
    #[serde(default)]
    scy: Option<String>,
    #[serde(default)]
    net: Option<String>,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    tls: Option<String>,
    #[serde(default)]
    sni: Option<String>,
    #[serde(default)]
    alpn: Option<String>,
    #[serde(default)]
    fp: Option<String>,
}

fn value_to_u16(val: &serde_json::Value) -> Option<u16> {
    match val {
        serde_json::Value::Number(n) => n.as_u64().and_then(|v| u16::try_from(v).ok()),
        serde_json::Value::String(s) => s.trim().parse::<u16>().ok(),
        _ => None,
    }
}

fn parse_vmess(uri: &str) -> Result<Profile> {
    let encoded = uri
        .get(8..)
        .ok_or_else(|| anyhow!("empty vmess payload"))?
        .trim();
    let decoded = decode_base64_flexible(encoded).ok_or_else(|| anyhow!("invalid vmess base64"))?;
    let v: VmessJson = serde_json::from_str(&decoded).context("invalid vmess JSON")?;
    let port = value_to_u16(&v.port).ok_or_else(|| anyhow!("invalid vmess port"))?;
    if v.add.trim().is_empty() || v.id.trim().is_empty() {
        bail!("vmess missing host or id");
    }

    let remarks = if v.ps.trim().is_empty() {
        format!("{}:{port}", v.add.trim())
    } else {
        v.ps.trim().to_string()
    };

    let mut profile = Profile::new(
        remarks,
        Protocol::Vmess,
        v.add.trim().to_string(),
        port,
        v.id.trim().to_string(),
    );
    profile.method = non_empty(v.scy.as_ref()).or_else(|| Some("auto".to_string()));
    profile.alter_id = value_to_u16(&v.aid);
    let tls_enabled = v
        .tls
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("tls"))
        .unwrap_or(false);
    profile.tls = TlsSettings {
        enabled: tls_enabled,
        server_name: non_empty(v.sni.as_ref()),
        insecure: false,
        alpn: parse_alpn(v.alpn.as_ref()),
        fingerprint: non_empty(v.fp.as_ref()),
        reality_public_key: None,
        reality_short_id: None,
        spider_x: None,
    };
    profile.transport = TransportSettings {
        network: non_empty(v.net.as_ref()),
        host: non_empty(v.host.as_ref()),
        path: non_empty(v.path.as_ref()),
        service_name: None,
        mode: None,
        xhttp_extra: None,
    };
    Ok(profile)
}

fn parse_shadowsocks(uri: &str) -> Result<Profile> {
    let rest = uri.get(5..).ok_or_else(|| anyhow!("empty ss URI"))?;
    let (without_frag, frag) = match rest.split_once('#') {
        Some((a, b)) => (a, percent_decode_str(b)),
        None => (rest, String::new()),
    };

    // SIP002: ss://BASE64(method:pass)@host:port
    // Legacy: ss://BASE64(method:pass@host:port)
    let (method, password, host, port) = if let Some((userinfo_part, host_part)) =
        without_frag.split_once('@')
    {
        let host_only = host_part.split('?').next().unwrap_or(host_part);
        let (host, port) = split_host_port(host_only)?;
        let (method, password) = if let Some((m, p)) = userinfo_part.split_once(':') {
            (percent_decode_str(m), percent_decode_str(p))
        } else {
            let decoded = decode_base64_flexible(userinfo_part)
                .ok_or_else(|| anyhow!("invalid ss userinfo base64"))?;
            let (m, p) = decoded
                .split_once(':')
                .ok_or_else(|| anyhow!("invalid ss method:password"))?;
            (m.to_string(), p.to_string())
        };
        (method, password, host, port)
    } else {
        let decoded = decode_base64_flexible(without_frag)
            .ok_or_else(|| anyhow!("invalid legacy ss base64"))?;
        let (creds, host_port) = decoded
            .rsplit_once('@')
            .ok_or_else(|| anyhow!("invalid legacy ss format"))?;
        let (method, password) = creds
            .split_once(':')
            .ok_or_else(|| anyhow!("invalid legacy ss method:password"))?;
        let (host, port) = split_host_port(host_port)?;
        (method.to_string(), password.to_string(), host, port)
    };

    let remarks = if frag.trim().is_empty() {
        format!("{host}:{port}")
    } else {
        frag.trim().to_string()
    };

    let mut profile = Profile::new(remarks, Protocol::Shadowsocks, host, port, password);
    profile.method = Some(method);
    Ok(profile)
}

fn parse_socks_or_http(uri: &str, protocol: Protocol) -> Result<Profile> {
    let url = Url::parse(uri).context("invalid proxy URI")?;
    let host = clean_host(&url)?;
    let default_port = match protocol {
        Protocol::Socks => 1080,
        Protocol::Http => 8080,
        _ => 443,
    };
    let port = url.port().unwrap_or(default_port);
    let remarks = decode_fragment(&url);
    let username = percent_decode_str(url.username());
    let password = url.password().map(percent_decode_str).unwrap_or_default();

    let mut profile = Profile::new(
        if remarks.is_empty() {
            format!("{host}:{port}")
        } else {
            remarks
        },
        protocol,
        host,
        port,
        password,
    );
    if !username.is_empty() {
        profile.username = Some(username);
    }
    Ok(profile)
}

fn split_host_port(raw: &str) -> Result<(String, u16)> {
    let s = raw.trim().trim_end_matches('/');
    if let Some(rest) = s.strip_prefix('[') {
        let (host, port_part) = rest
            .split_once("]:")
            .ok_or_else(|| anyhow!("invalid IPv6 host:port"))?;
        let port = port_part.parse::<u16>().context("invalid port")?;
        Ok((host.to_string(), port))
    } else {
        let (host, port_part) = s
            .rsplit_once(':')
            .ok_or_else(|| anyhow!("missing port in host:port"))?;
        let port = port_part.parse::<u16>().context("invalid port")?;
        Ok((host.to_string(), port))
    }
}

pub fn decode_base64_flexible(input: &str) -> Option<String> {
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.is_empty() {
        return None;
    }
    let bytes = STANDARD
        .decode(&cleaned)
        .or_else(|_| STANDARD_NO_PAD.decode(&cleaned))
        .or_else(|_| URL_SAFE.decode(&cleaned))
        .or_else(|_| URL_SAFE_NO_PAD.decode(&cleaned))
        .ok()?;
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vless_reality() {
        let uri = "vless://11111111-2222-3333-4444-555555555555@example.com:443?encryption=none&flow=xtls-rprx-vision&security=reality&sni=www.microsoft.com&fp=chrome&pbk=PUBKEY123&sid=abcd&type=tcp#Finland%2001";
        let p = parse_uri(uri).expect("should parse vless");
        assert_eq!(p.protocol, Protocol::Vless);
        assert_eq!(p.server, "example.com");
        assert_eq!(p.port, 443);
        assert_eq!(p.remarks, "Finland 01");
        assert_eq!(p.flow.as_deref(), Some("xtls-rprx-vision"));
        assert!(p.tls.enabled);
        assert_eq!(p.tls.reality_public_key.as_deref(), Some("PUBKEY123"));
        assert_eq!(p.tls.reality_short_id.as_deref(), Some("abcd"));
    }

    #[test]
    fn parses_hysteria2() {
        let uri = "hy2://secretpass@1.2.3.4:8443/?sni=cdn.example.com&obfs=salamander&obfs-password=obfspw&insecure=1#FastHy2";
        let p = parse_uri(uri).expect("should parse hy2");
        assert_eq!(p.protocol, Protocol::Hysteria2);
        assert_eq!(p.port, 8443);
        assert_eq!(p.secret, "secretpass");
        assert_eq!(p.obfs.as_deref(), Some("salamander"));
        assert_eq!(p.obfs_password.as_deref(), Some("obfspw"));
        assert!(p.tls.insecure);
    }

    #[test]
    fn parses_base64_subscription() {
        let raw = "trojan://pass123@t.example.com:443?sni=t.example.com#NodeA\n";
        let encoded = STANDARD.encode(raw);
        let list = parse_batch(&encoded);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].remarks, "NodeA");
    }
}

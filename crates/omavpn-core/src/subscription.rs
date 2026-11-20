use std::fs;
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client;
use url::Url;

use crate::models::{Profile, Subscription};
use crate::parser::parse_batch;

/// Unwraps common VPN client deep links (`happ://add/...`, `v2raytun://import/...`,
/// `hiddify://import/...`, `v2rayng://install-sub?url=...`, etc.) into a plain `http(s)://` URL.
pub fn extract_subscription_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.contains('\n') {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some(trimmed.to_string());
    }

    // Prefix deep links like: happ://add/https://..., v2raytun://import/https://..., hiddify://import/https://...
    for prefix in [
        "happ://add/",
        "v2raytun://import/",
        "hiddify://import/",
        "streisand://import/",
        "sing-box://import-remote-profile?url=",
    ] {
        if lower.starts_with(prefix) {
            let rest = trimmed[prefix.len()..].trim();
            let decoded = percent_decode(rest);
            let candidate = if decoded.to_ascii_lowercase().starts_with("http://")
                || decoded.to_ascii_lowercase().starts_with("https://")
            {
                decoded
            } else {
                rest.to_string()
            };
            let cand_lower = candidate.to_ascii_lowercase();
            if cand_lower.starts_with("http://") || cand_lower.starts_with("https://") {
                return Some(candidate);
            }
        }
    }

    // Query-parameter deep links like: v2rayng://install-sub?url=https%3A%2F%2F...
    if let Ok(parsed) = Url::parse(trimmed) {
        for (k, v) in parsed.query_pairs() {
            if k.eq_ignore_ascii_case("url") {
                let val = v.trim();
                let val_lower = val.to_ascii_lowercase();
                if val_lower.starts_with("http://") || val_lower.starts_with("https://") {
                    return Some(val.to_string());
                }
            }
        }
    }

    None
}

fn percent_decode(input: &str) -> String {
    url::form_urlencoded::parse(format!("x={input}").as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| input.to_string())
}

/// Collects genuine Linux device metadata for panels that enforce HWID device limits.
pub fn build_device_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();

    if let Some(hwid) = read_machine_id() {
        if let Ok(val) = HeaderValue::from_str(&hwid) {
            headers.insert("x-hwid", val);
        }
    }

    headers.insert("x-device-os", HeaderValue::from_static("Linux"));

    if let Some(ver) = read_os_version() {
        if let Ok(val) = HeaderValue::from_str(&ver) {
            headers.insert("x-ver-os", val);
        }
    }

    if let Some(model) = read_device_model() {
        if let Ok(val) = HeaderValue::from_str(&model) {
            headers.insert("x-device-model", val);
        }
    }

    if let Some(locale) = read_locale() {
        if let Ok(val) = HeaderValue::from_str(&locale) {
            headers.insert("x-device-locale", val);
        }
    }

    headers
}

fn read_machine_id() -> Option<String> {
    for path in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(raw) = fs::read_to_string(path) {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn read_os_version() -> Option<String> {
    if let Ok(raw) = fs::read_to_string("/etc/os-release") {
        for line in raw.lines() {
            if let Some(rest) = line.strip_prefix("PRETTY_NAME=") {
                let cleaned = rest.trim().trim_matches('"').trim();
                if !cleaned.is_empty() {
                    return Some(cleaned.to_string());
                }
            }
        }
    }
    Some(std::env::consts::ARCH.to_string())
}

fn read_device_model() -> Option<String> {
    if let Ok(raw) = fs::read_to_string("/etc/hostname") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    Some("Linux Desktop".to_string())
}

fn read_locale() -> Option<String> {
    let raw = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LANG"))
        .ok()?;
    let trimmed = raw.split('.').next()?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub async fn fetch_subscription(sub: &Subscription) -> Result<Vec<Profile>> {
    let ua = sub
        .user_agent
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("omavpn/0.1.0");

    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(ua)
        .default_headers(build_device_headers())
        .build()
        .context("failed to build HTTP client")?;

    let body = client
        .get(&sub.url)
        .send()
        .await
        .with_context(|| format!("failed to fetch subscription '{}'", sub.name))?
        .error_for_status()
        .with_context(|| format!("subscription '{}' returned HTTP error", sub.name))?
        .text()
        .await
        .context("failed to read subscription body")?;

    let mut profiles = parse_batch(&body);
    for p in &mut profiles {
        p.subscription_id = Some(sub.id.clone());
    }
    Ok(profiles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwraps_happ_deep_link() {
        let got = extract_subscription_url("happ://add/https://example.com/sub/abc123");
        assert_eq!(got.as_deref(), Some("https://example.com/sub/abc123"));
    }

    #[test]
    fn unwraps_v2rayng_install_sub() {
        let got = extract_subscription_url(
            "v2rayng://install-sub?url=https%3A%2F%2Fexample.com%2Fsub%3Fid%3D1&name=Test",
        );
        assert_eq!(got.as_deref(), Some("https://example.com/sub?id=1"));
    }

    #[test]
    fn accepts_plain_https() {
        let got = extract_subscription_url("https://example.com/sub");
        assert_eq!(got.as_deref(), Some("https://example.com/sub"));
    }

    #[test]
    fn builds_device_headers_with_os() {
        let headers = build_device_headers();
        assert_eq!(headers.get("x-device-os").and_then(|v| v.to_str().ok()), Some("Linux"));
    }
}

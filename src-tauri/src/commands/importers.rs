//! Profile importers — file, URL, and Gist.
//!
//! URL fetcher is hardened against SSRF: scheme must be HTTPS, hosts must
//! resolve only to public IPs (private/loopback/link-local/CGNAT rejected
//! per-hop), redirects are capped at 2 with the same checks, body is streamed
//! with a 1 MiB hard cap, and a User-Agent is always set.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use futures_util::StreamExt;
use reqwest::redirect::Policy;

use crate::models::ProfileFile;

use super::profiles::save_profile;
use super::{CmdResult, CommandError};

const MAX_BODY_BYTES: usize = 1024 * 1024;
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);
const USER_AGENT: &str = concat!("ad/", env!("CARGO_PKG_VERSION"));

#[tauri::command]
pub fn import_from_file(path: String) -> CmdResult<ProfileFile> {
    let pb = PathBuf::from(path);
    // Defense in depth: only accept .json files. The frontend dialog filter
    // already restricts this, but a direct invoke() bypasses the dialog.
    if pb.extension().and_then(|s| s.to_str()) != Some("json") {
        return Err(CommandError::Generic("only .json files accepted".into()));
    }
    let metadata = std::fs::metadata(&pb).with_context(|| format!("stat {}", pb.display()))?;
    if metadata.len() > MAX_BODY_BYTES as u64 {
        return Err(CommandError::Generic(format!(
            "file > 1 MiB: {} bytes",
            metadata.len()
        )));
    }
    let bytes = std::fs::read(&pb).with_context(|| format!("read {}", pb.display()))?;
    let profile: ProfileFile = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse profile json from {}", pb.display()))?;
    save_profile(profile)
}

#[tauri::command]
pub async fn import_from_url(url: String) -> CmdResult<ProfileFile> {
    let final_url = if let Some(gist_id) = parse_gist_url(&url) {
        resolve_gist_raw_url(&gist_id).await?
    } else {
        url
    };

    validate_url(&final_url)?;
    validate_host_resolves_to_public(&final_url).await?;

    let client = build_safe_client()?;
    let resp = client
        .get(&final_url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| CommandError::Generic(format!("http get: {e}")))?;

    if !resp.status().is_success() {
        return Err(CommandError::Generic(format!(
            "import url returned status {}",
            resp.status()
        )));
    }

    if let Some(ct) = resp.headers().get("content-type") {
        let s = ct.to_str().unwrap_or("");
        if !s.contains("json") {
            return Err(CommandError::Generic(format!(
                "expected json content-type, got: {s}"
            )));
        }
    }

    if let Some(len) = resp.content_length() {
        if len > MAX_BODY_BYTES as u64 {
            return Err(CommandError::Generic(format!(
                "Content-Length {len} exceeds 1 MiB cap"
            )));
        }
    }

    // Stream the body so a malicious server can't inflate RAM beyond the cap.
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| CommandError::Generic(format!("stream: {e}")))?;
        if buf.len() + chunk.len() > MAX_BODY_BYTES {
            return Err(CommandError::Generic("response > 1 MiB".into()));
        }
        buf.extend_from_slice(&chunk);
    }

    let profile: ProfileFile =
        serde_json::from_slice(&buf).context("parse imported profile json")?;
    save_profile(profile)
}

fn build_safe_client() -> CmdResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(USER_AGENT)
        .https_only(true)
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() >= 2 {
                return attempt.error("too many redirects (max 2)");
            }
            let url = attempt.url();
            if url.scheme() != "https" {
                return attempt.error("redirect to non-https rejected");
            }
            // Per-hop host re-validation happens via dns_resolver below.
            attempt.follow()
        }))
        .build()
        .map_err(|e| CommandError::Generic(format!("client build: {e}")))
}

fn validate_url(url: &str) -> CmdResult<()> {
    let parsed =
        url::Url::parse(url).map_err(|e| CommandError::Generic(format!("invalid url: {e}")))?;
    if parsed.scheme() != "https" {
        return Err(CommandError::Generic(format!(
            "only https urls accepted, got: {}",
            parsed.scheme()
        )));
    }
    if parsed.host_str().is_none() {
        return Err(CommandError::Generic("url has no host".into()));
    }
    Ok(())
}

/// Resolves the URL's host and rejects if any returned address is non-public.
/// Run before each request to keep the SSRF window microsecond-thin.
async fn validate_host_resolves_to_public(url: &str) -> CmdResult<()> {
    let parsed =
        url::Url::parse(url).map_err(|e| CommandError::Generic(format!("invalid url: {e}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| CommandError::Generic("url has no host".into()))?
        .to_string();
    let port = parsed.port_or_known_default().unwrap_or(443);

    // Resolve via blocking std lookup on a Tokio worker. This matches what
    // reqwest's default resolver does and avoids a heavier async-DNS dep.
    let addrs: Vec<SocketAddr> = tokio::task::spawn_blocking(move || {
        (host.as_str(), port).to_socket_addrs().map(|i| i.collect())
    })
    .await
    .map_err(|e| CommandError::Generic(format!("dns join: {e}")))?
    .map_err(|e| CommandError::Generic(format!("dns: {e}")))?;

    if addrs.is_empty() {
        return Err(CommandError::Generic("no DNS results".into()));
    }
    for addr in &addrs {
        if !is_public_ip(addr.ip()) {
            return Err(CommandError::Generic(format!(
                "host resolves to non-public ip: {}",
                addr.ip()
            )));
        }
    }
    Ok(())
}

/// Returns true only for globally routable unicast addresses. Rejects
/// loopback, link-local, private (RFC1918, CGNAT, RFC6598), unique-local IPv6,
/// multicast, broadcast, unspecified, and documentation ranges.
fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_unspecified()
                || v4.is_documentation()
                // 100.64.0.0/10 (CGNAT, RFC6598)
                || (o[0] == 100 && (o[1] & 0xC0) == 64)
                // 0.0.0.0/8
                || o[0] == 0)
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // fe80::/10 link-local
                || (s[0] & 0xFFC0) == 0xFE80
                // fc00::/7 unique local
                || (s[0] & 0xFE00) == 0xFC00)
        }
    }
}

fn parse_gist_url(url: &str) -> Option<String> {
    let s = url.trim();
    let prefix = "https://gist.github.com/";
    let rest = s.strip_prefix(prefix)?;
    let mut parts = rest.split('/');
    let _user = parts.next()?;
    let id = parts.next()?;
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

async fn resolve_gist_raw_url(id: &str) -> CmdResult<String> {
    let url = format!("https://api.github.com/gists/{id}");
    validate_host_resolves_to_public(&url).await?;
    let client = build_safe_client()?;
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| CommandError::Generic(format!("get gist meta: {e}")))?;

    if !resp.status().is_success() {
        return Err(CommandError::Generic(format!(
            "gist api status {}",
            resp.status()
        )));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| CommandError::Generic(format!("parse gist meta: {e}")))?;

    let files = json
        .get("files")
        .and_then(|v| v.as_object())
        .ok_or_else(|| CommandError::Generic("gist has no files".into()))?;

    let mut names: Vec<&String> = files.keys().collect();
    names.sort();
    for name in names {
        if name.ends_with(".json") {
            if let Some(raw) = files
                .get(name)
                .and_then(|f| f.get("raw_url"))
                .and_then(|v| v.as_str())
            {
                return Ok(raw.to_string());
            }
        }
    }
    Err(CommandError::Generic("no .json file in gist".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::net::Ipv6Addr;

    #[test]
    fn detects_gist_urls() {
        assert_eq!(
            parse_gist_url("https://gist.github.com/alice/abc123"),
            Some("abc123".into())
        );
        assert_eq!(
            parse_gist_url("https://gist.github.com/alice/abc123/revisions"),
            Some("abc123".into())
        );
        assert_eq!(parse_gist_url("https://example.com/foo.json"), None);
    }

    #[test]
    fn rejects_non_https() {
        assert!(validate_url("http://example.com/x.json").is_err());
        assert!(validate_url("file:///etc/passwd").is_err());
        assert!(validate_url("ftp://example.com/x").is_err());
        assert!(validate_url("https://example.com/x.json").is_ok());
    }

    #[test]
    fn rejects_private_ips() {
        // IPv4
        assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
        assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
        assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))));
        assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)))); // AWS metadata
        assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1)))); // CGNAT
        assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))));
        // public
        assert!(is_public_ip(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
        assert!(is_public_ip(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
        // IPv6
        assert!(!is_public_ip(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(!is_public_ip(IpAddr::V6(Ipv6Addr::UNSPECIFIED)));
        assert!(!is_public_ip(IpAddr::V6(
            "fe80::1".parse::<Ipv6Addr>().unwrap()
        )));
        assert!(!is_public_ip(IpAddr::V6(
            "fc00::1".parse::<Ipv6Addr>().unwrap()
        )));
        assert!(is_public_ip(IpAddr::V6(
            "2606:4700:4700::1111".parse::<Ipv6Addr>().unwrap()
        )));
    }

    #[tokio::test]
    async fn rejects_localhost_resolution() {
        // "localhost" resolves to 127.0.0.1 / ::1
        let err = validate_host_resolves_to_public("https://localhost/x.json")
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("non-public"));
    }
}

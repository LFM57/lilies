#![deny(unsafe_code)]

use crate::error::NetError;
use std::fs;
use std::path::Path;
use std::time::Duration;

pub const USER_AGENT: &str = "lilies/0.1.0";
pub const MAX_REDIRECTS: usize = 5;

/// Fetches the resource specified by `url`.
///
/// Supported schemes:
/// - `http://` and `https://`: Fetched over network using `ureq` with pure-Rust TLS,
///   handling up to 5 HTTP redirects and setting User-Agent `"lilies/0.1.0"`.
/// - `file://` or local file paths: Read directly from local filesystem.
pub fn fetch(url: &str) -> Result<String, NetError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(NetError::InvalidUrl("URL cannot be empty".to_string()));
    }

    if let Some((scheme, rest)) = trimmed.split_once("://") {
        match scheme.to_ascii_lowercase().as_str() {
            "http" | "https" => fetch_http(trimmed),
            "file" => fetch_file(rest),
            unsupported => Err(NetError::UnsupportedScheme(unsupported.to_string())),
        }
    } else {
        // No scheme specified: treat as local filesystem path
        fetch_local_path(trimmed)
    }
}

fn fetch_http(url: &str) -> Result<String, NetError> {
    let agent = ureq::AgentBuilder::new()
        .redirects(MAX_REDIRECTS as u32)
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(30))
        .build();

    let response = agent.get(url).call()?;
    let body = response.into_string()?;
    Ok(body)
}

fn fetch_file(path_str: &str) -> Result<String, NetError> {
    // Strip optional localhost if present (e.g. file://localhost/path)
    let path_str = if let Some(stripped) = path_str.strip_prefix("localhost/") {
        format!("/{}", stripped)
    } else if let Some(stripped) = path_str.strip_prefix("localhost") {
        stripped.to_string()
    } else {
        path_str.to_string()
    };

    let decoded = percent_decode(&path_str);
    fetch_local_path(&decoded)
}

fn fetch_local_path(path_str: &str) -> Result<String, NetError> {
    let path = Path::new(path_str);
    fs::read_to_string(path).map_err(NetError::Io)
}

/// Decodes standard URL percent-encoded sequences (%20, etc.).
fn percent_decode(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let mut chars = input.bytes();

    while let Some(b) = chars.next() {
        if b == b'%' {
            let h1 = chars.next();
            let h2 = chars.next();
            if let (Some(c1), Some(c2)) = (h1, h2) {
                if let Ok(val) = u8::from_str_radix(
                    std::str::from_utf8(&[c1, c2]).unwrap_or_default(),
                    16,
                ) {
                    bytes.push(val);
                    continue;
                }
            }
        }
        bytes.push(b);
    }

    String::from_utf8_lossy(&bytes).into_owned()
}

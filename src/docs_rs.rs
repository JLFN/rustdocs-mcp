//! docs.rs client: rustdoc JSON download and crate source fetching.
//!
//! docs.rs serves the full rustdoc JSON for a crate build as zstd-compressed
//! JSON at /crate/{name}/{version}/json.zst (optionally with a target triple
//! segment: /crate/{name}/{version}/{target}/json.zst). We decompress and
//! parse it with `rustdoc-types` and cache the parsed crate in memory.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use rustdoc_types::Crate;
use tokio::sync::Mutex;

const DOCS_RS: &str = "https://docs.rs";
const USER_AGENT: &str = "rustdocs-mcp/0.1 (open-grok MCP server)";

/// Load (and cache) the parsed rustdoc JSON for `krate@version`.
///
/// `version` must already be a concrete version (resolve it via crates.io
/// first). Returns a shared Arc so repeated lookups within one process are
/// free after the first fetch.
pub async fn load_rustdoc(
    client: &reqwest::Client,
    cache: &Mutex<HashMap<String, Arc<Crate>>>,
    krate: &str,
    version: &str,
) -> Result<Arc<Crate>> {
    let key = format!("{krate}@{version}");
    {
        let cache = cache.lock().await;
        if let Some(docs) = cache.get(&key) {
            return Ok(Arc::clone(docs));
        }
    }

    let url = format!("{DOCS_RS}/crate/{krate}/{version}/json.zst");
    let bytes = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .with_context(|| format!("failed to download rustdoc JSON: {url}"))?;
    if !bytes.status().is_success() {
        bail!("docs.rs returned HTTP {} for {url}", bytes.status());
    }
    let bytes = bytes.bytes().await?;

    let json = zstd::stream::decode_all(bytes.as_ref())
        .with_context(|| format!("failed to decompress rustdoc JSON for {krate}@{version}"))?;

    let docs: Crate = match serde_json::from_slice(&json) {
        Ok(c) => c,
        Err(e) => {
            // Give a precise error for format mismatches instead of a raw
            // serde error: read just the format_version field.
            let format_version = serde_json::from_slice::<serde_json::Value>(&json)
                .ok()
                .and_then(|v| v.get("format_version").and_then(|f| f.as_u64()))
                .unwrap_or(0);
            bail!(
                "cannot parse rustdoc JSON for {krate}@{version} (format_version {format_version}, expected {}): {e}",
                rustdoc_types::FORMAT_VERSION
            );
        }
    };

    if docs.format_version != rustdoc_types::FORMAT_VERSION {
        bail!(
            "rustdoc JSON for {krate}@{version} has format_version {}, but this build of rustdocs-mcp supports {}",
            docs.format_version,
            rustdoc_types::FORMAT_VERSION
        );
    }

    let docs = Arc::new(docs);
    let mut cache = cache.lock().await;
    cache.insert(key, Arc::clone(&docs));
    Ok(docs)
}

/// Fetch a raw source file of a crate version from the crates.io `.crate`
/// archive (static.crates.io). `path` is relative to the crate root, e.g.
/// "src/lib.rs", "Cargo.toml", "examples/hello.rs".
pub async fn get_source(
    client: &reqwest::Client,
    krate: &str,
    version: &str,
    path: &str,
) -> Result<String> {
    let url = format!("https://static.crates.io/crates/{krate}/{krate}-{version}.crate");
    let resp = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .with_context(|| format!("failed to fetch crate archive: {url}"))?;
    if !resp.status().is_success() {
        bail!("static.crates.io returned HTTP {} for {url}", resp.status());
    }
    let bytes = resp.bytes().await?;

    let gz = flate2::read::GzDecoder::new(bytes.as_ref());
    let mut archive = tar::Archive::new(gz);
    let wanted = format!("{krate}-{version}/{path}");
    let mut found: Option<Vec<u8>> = None;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let entry_path = entry.path()?.into_owned();
        let entry_path = entry_path.to_string_lossy().replace('\\', "/");
        if entry_path == wanted {
            let mut buf = Vec::new();
            use std::io::Read as _;
            entry.read_to_end(&mut buf)?;
            found = Some(buf);
            break;
        }
    }
    let Some(bytes) = found else {
        bail!("file '{path}' not found in {krate}-{version}.crate");
    };
    String::from_utf8(bytes).with_context(|| format!("source file '{path}' is not valid UTF-8"))
}

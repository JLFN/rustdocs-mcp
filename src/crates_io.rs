//! crates.io HTTP API client (api.crates.io/v1).
//!
//! All endpoints are public JSON, no authentication. A User-Agent header is
//! set (crates.io policy) on the shared client, created in `main.rs`.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const API: &str = "https://crates.io/api/v1";

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResponse {
    pub crates: Vec<CrateSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrateSummary {
    pub name: String,
    pub description: Option<String>,
    pub max_version: String,
    pub downloads: u64,
    pub recent_downloads: u64,
    pub documentation: Option<String>,
    pub repository: Option<String>,
    pub homepage: Option<String>,
    #[serde(default)]
    pub keywords: Option<Vec<String>>,
    #[serde(default)]
    pub categories: Option<Vec<String>>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CrateDetailsResponse {
    #[serde(rename = "crate")]
    pub crate_: CrateDetails,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CrateDetails {
    pub name: String,
    pub description: Option<String>,
    pub max_version: String,
    pub newest_version: String,
    pub downloads: u64,
    pub recent_downloads: u64,
    pub documentation: Option<String>,
    pub repository: Option<String>,
    pub homepage: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub default_version: String,
    pub num_versions: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VersionsResponse {
    pub versions: Vec<VersionInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VersionInfo {
    pub num: String,
    pub yanked: bool,
    pub downloads: u64,
    pub created_at: String,
    pub rust_version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DependenciesResponse {
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Dependency {
    #[serde(rename = "crate_id")]
    pub name: String,
    pub req: String,
    pub optional: bool,
    pub default_features: bool,
    #[serde(default)]
    pub features: Vec<String>,
    pub kind: String,
    pub target: Option<String>,
}

/// Search crates.io by query string.
pub async fn search(client: &reqwest::Client, q: &str, limit: u32) -> Result<SearchResponse> {
    let url = format!("{API}/crates?q={}&per_page={limit}", urlencode(q));
    get_json(client, &url).await
}

/// Fetch full metadata for one crate.
pub async fn crate_details(client: &reqwest::Client, name: &str) -> Result<CrateDetails> {
    let url = format!("{API}/crates/{}", urlencode(name));
    let resp: CrateDetailsResponse = get_json(client, &url).await?;
    Ok(resp.crate_)
}

/// Fetch all versions of a crate (the API returns them newest first).
pub async fn versions(client: &reqwest::Client, name: &str) -> Result<Vec<VersionInfo>> {
    let url = format!("{API}/crates/{}/versions", urlencode(name));
    let resp: VersionsResponse = get_json(client, &url).await?;
    Ok(resp.versions)
}

/// Fetch dependencies of one crate version.
pub async fn dependencies(
    client: &reqwest::Client,
    name: &str,
    version: &str,
) -> Result<Vec<Dependency>> {
    let url = format!(
        "{API}/crates/{}/{}/dependencies",
        urlencode(name),
        urlencode(version)
    );
    let resp: DependenciesResponse = get_json(client, &url).await?;
    Ok(resp.dependencies)
}

/// Resolve a semver requirement to a concrete non-yanked version.
///
/// Accepted forms:
///   - empty / "*"       -> newest non-yanked
///   - "1.2.3"           -> exact version (docs.rs semantics)
///   - "1.2", "^1.2",
///     ">=1, <2", ...    -> cargo-style VersionReq, newest match
pub fn resolve(versions: &[VersionInfo], req: &str) -> Result<String> {
    let non_yanked: Vec<&VersionInfo> = versions.iter().filter(|v| !v.yanked).collect();
    if non_yanked.is_empty() {
        bail!("no non-yanked versions found");
    }
    let req = req.trim();
    if req.is_empty() || req == "*" || req == "latest" || req == "newest" {
        // The API returns versions newest-first; the first non-yanked is the
        // latest release.
        return Ok(non_yanked[0].num.clone());
    }
    // Exact version wins (matches docs.rs URL semantics).
    if let Some(v) = non_yanked.iter().find(|v| v.num == req) {
        return Ok(v.num.clone());
    }
    let parsed = semver::VersionReq::parse(req)
        .with_context(|| format!("invalid semver requirement '{req}'"))?;
    let mut best: Option<&VersionInfo> = None;
    for v in non_yanked {
        let Ok(ver) = semver::Version::parse(&v.num) else { continue };
        if parsed.matches(&ver) && best.is_none() {
            best = Some(v);
        }
    }
    match best {
        Some(v) => Ok(v.num.clone()),
        None => bail!("no non-yanked version of the crate matches requirement '{req}'"),
    }
}

async fn get_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
) -> Result<T> {
    let resp = client
        .get(url)
        .header("User-Agent", "rustdocs-mcp/0.1 (open-grok MCP server)")
        .send()
        .await
        .with_context(|| format!("request failed: {url}"))?;
    if !resp.status().is_success() {
        bail!("HTTP {} from {url}", resp.status());
    }
    resp.json::<T>()
        .await
        .with_context(|| format!("bad JSON from {url}"))
}

fn urlencode(s: &str) -> String {
    // Percent-encode everything except unreserved chars.
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

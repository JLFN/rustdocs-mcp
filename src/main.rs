//! rustdocs-mcp — MCP server exposing Rust crate documentation.
//!
//! Tools:
//!   - rustdocs_search_crates      search crates.io by query
//!   - rustdocs_crate_metadata     metadata for one crate (latest version, downloads, links)
//!   - rustdocs_list_versions      all versions of a crate
//!   - rustdocs_resolve_version    resolve a semver requirement to a concrete version
//!   - rustdocs_dependencies       dependencies of a crate version
//!   - rustdocs_get_item           item at a path (signature, docs, span), re-exports followed
//!   - rustdocs_list_module        items inside a module
//!   - rustdocs_list_methods       inherent methods of a type
//!   - rustdocs_list_impls         trait implementations of a type
//!   - rustdocs_search_items       substring search over item names in a crate
//!   - rustdocs_get_source         raw source file from docs.rs
//!
//! Data sources: crates.io HTTP API (search/metadata/versions/deps) and
//! docs.rs rustdoc JSON (structured API surface, zstd-compressed).

mod crates_io;
mod docs_rs;
mod rustdoc;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
    ServiceExt,
};
use rustdoc_types::Crate;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
struct RustdocsServer {
    tool_router: ToolRouter<Self>,
    client: Arc<reqwest::Client>,
    cache: Arc<Mutex<HashMap<String, Arc<Crate>>>>,
}

impl RustdocsServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            client: Arc::new(reqwest::Client::new()),
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Resolve the requested version to a concrete one: use it as given, or
    /// fall back to the latest non-yanked version when omitted / "latest".
    async fn version_or_latest(&self, krate: &str, version: &Option<String>) -> Result<String> {
        match version.as_deref() {
            Some(v) if !v.is_empty() && v != "latest" => Ok(v.to_string()),
            _ => {
                let vs = crates_io::versions(&self.client, krate).await?;
                crates_io::resolve(&vs, "")
            }
        }
    }

    /// Load rustdoc JSON for krate@version (resolving "latest" first).
    async fn load(&self, krate: &str, version: &Option<String>) -> Result<(String, Arc<Crate>)> {
        let version = self.version_or_latest(krate, version).await?;
        let docs = docs_rs::load_rustdoc(&self.client, &self.cache, krate, &version).await?;
        Ok((version, docs))
    }

    /// Resolve a path to an item, following re-export (import) chains.
    async fn item_at(&self, krate: &str, version: &Option<String>, path: &str) -> String {
        let (version, docs) = match self.load(krate, version).await {
            Ok(v) => v,
            Err(e) => return err_json(&e),
        };
        let (target, fuzzy) = match resolve_id(&docs, path) {
            Ok((id, fuzzy)) => (id, fuzzy),
            Err(e) => {
                return serde_json::json!({
                    "error": e,
                    "candidates": fuzzy_candidates(&docs, path),
                })
                .to_string()
            }
        };
        let Some(item) = docs.index.get(&target) else {
            return err_json(&anyhow::anyhow!("item id missing from index"));
        };
        let is_module = matches!(item.inner, rustdoc_types::ItemEnum::Module(_));
        serde_json::json!({
            "crate": krate,
            "version": version,
            "resolved_path": rustdoc::id_path(&docs, target),
            "requested_path": path,
            "fuzzy_match": fuzzy,
            "name": rustdoc::display_name(item),
            "kind": rustdoc::item_kind_name(&item.inner.item_kind()),
            "signature": rustdoc::signature(&docs, item),
            "docs": item.docs.clone().unwrap_or_default(),
            "span": rustdoc::span_str(item),
            "module_items": if is_module { Some(rustdoc::module_items(&docs, target)) } else { None },
        })
        .to_string()
    }
}

/// Resolve a path to an item id, following re-export chains. Returns the id
/// plus whether the match was fuzzy (by name rather than exact path).
fn resolve_id(
    docs: &Crate,
    path: &str,
) -> std::result::Result<(rustdoc_types::Id, bool), String> {
    if let Some((id, _)) = rustdoc::resolve_path(docs, path) {
        return Ok((follow_import(docs, id), false));
    }
    let last = path.rsplit("::").next().unwrap_or(path);
    let exact: Vec<rustdoc_types::Id> = rustdoc::find_by_name(docs, last)
        .into_iter()
        .filter(|id| {
            docs.index
                .get(id)
                .and_then(|it| it.name.clone())
                .map(|n| n == last)
                .unwrap_or(false)
        })
        .collect();
    match exact.len() {
        0 => Err(format!("item '{path}' not found")),
        1 => Ok((follow_import(docs, exact[0]), true)),
        _ => Err(format!(
            "ambiguous path '{path}'; use a full path like crate::mod::Name"
        )),
    }
}

/// Candidate rows for an unresolved path, to help the model disambiguate.
fn fuzzy_candidates(docs: &Crate, path: &str) -> Vec<serde_json::Value> {
    let last = path.rsplit("::").next().unwrap_or(path);
    let ids = rustdoc::find_by_name(docs, last);
    if ids.is_empty() {
        return Vec::new();
    }
    rustdoc::candidate_rows(docs, &ids)
}

/// Follow `use` re-export chains to the canonical item.
fn follow_import(docs: &Crate, mut id: rustdoc_types::Id) -> rustdoc_types::Id {
    for _ in 0..8 {
        let Some(item) = docs.index.get(&id) else { break };
        let rustdoc_types::ItemEnum::Use(u) = &item.inner else { break };
        let Some(next) = u.id else { break };
        if next == id {
            break;
        }
        id = next;
    }
    id
}

// ---------------------------------------------------------------------------
// Tool input types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct SearchCratesRequest {
    /// Search query, e.g. "web framework" or "async postgres".
    q: String,
    /// Maximum number of results. Default 10, max 25.
    limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct CrateRequest {
    /// Crate name on crates.io, e.g. "serde".
    name: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct VersionRequest {
    /// Crate name on crates.io, e.g. "axum".
    krate: String,
    /// Semver requirement: empty / "*" for latest, "1.2.3" for exact,
    /// or cargo-style like "^1.5" / ">=1, <2". Default: latest.
    req: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct DepsRequest {
    /// Crate name on crates.io.
    krate: String,
    /// Concrete version. Default: latest non-yanked.
    version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct ItemPathRequest {
    /// Crate name on crates.io.
    krate: String,
    /// Concrete version. Default: latest non-yanked.
    version: Option<String>,
    /// Item path, e.g. "axum::Router" or "serde::Deserialize". Re-exports are followed.
    path: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct SearchItemsRequest {
    /// Crate name on crates.io.
    krate: String,
    /// Concrete version. Default: latest non-yanked.
    version: Option<String>,
    /// Substring to search for in item names, e.g. "IntoIterator" or "builder".
    query: String,
    /// Maximum number of results. Default 30, max 100.
    limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct SourceRequest {
    /// Crate name on crates.io.
    krate: String,
    /// Concrete version. Default: latest non-yanked.
    version: Option<String>,
    /// Path inside the crate source, e.g. "src/lib.rs" or "examples/hello.rs".
    path: String,
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

#[tool_router]
impl RustdocsServer {
    /// Search crates.io by query string.
    #[tool(description = "Search the crates.io registry for crates matching a query string (name, description, keywords). Returns JSON with up to 'limit' crates: name, latest version, total downloads, recent downloads, description, docs/repository/homepage links. Use to discover which crate to use or to check a crate exists.")]
    async fn rustdocs_search_crates(
        &self,
        Parameters(req): Parameters<SearchCratesRequest>,
    ) -> String {
        match crates_io::search(&self.client, &req.q, req.limit.unwrap_or(10).min(25)).await {
            Ok(resp) => serde_json::to_string_pretty(&resp.crates).unwrap_or_else(|_| "[]".into()),
            Err(e) => err_json(&e),
        }
    }

    /// Fetch full metadata for one crate.
    #[tool(description = "Fetch metadata for one crate from crates.io: latest version, total and recent downloads, description, keywords, categories, documentation/repository/homepage links, creation and update dates. Pass the exact crate name, e.g. 'serde'.")]
    async fn rustdocs_crate_metadata(
        &self,
        Parameters(req): Parameters<CrateRequest>,
    ) -> String {
        match crates_io::crate_details(&self.client, &req.name).await {
            Ok(d) => serde_json::to_string_pretty(&d).unwrap_or_else(|_| "{}".into()),
            Err(e) => err_json(&e),
        }
    }

    /// List all versions of a crate.
    #[tool(description = "List all published versions of a crate from crates.io, newest first, each with yanked status, download count, release date and MSRV (rust_version). Useful before pinning a dependency or resolving a version requirement.")]
    async fn rustdocs_list_versions(
        &self,
        Parameters(req): Parameters<CrateRequest>,
    ) -> String {
        match crates_io::versions(&self.client, &req.name).await {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|_| "[]".into()),
            Err(e) => err_json(&e),
        }
    }

    /// Resolve a semver requirement to a concrete version.
    #[tool(description = "Resolve a semver requirement to a concrete non-yanked version of a crate. Empty/'*'/'latest' gives the newest release; '1.2.3' is an exact match; '^1.5', '~1.2', '>=1, <2' use cargo semantics and pick the newest match. Returns the concrete version string to pass to the other tools.")]
    async fn rustdocs_resolve_version(
        &self,
        Parameters(req): Parameters<VersionRequest>,
    ) -> String {
        let krate = req.krate.clone();
        let req = req.req.as_deref().unwrap_or("*");
        match crates_io::versions(&self.client, &krate).await {
            Ok(vs) => match crates_io::resolve(&vs, req) {
                Ok(v) => serde_json::json!({ "crate": krate, "requirement": req, "version": v }).to_string(),
                Err(e) => err_json(&e),
            },
            Err(e) => err_json(&e),
        }
    }

    /// Dependencies of a crate version.
    #[tool(description = "Fetch the dependency list of a crate version from crates.io: each dependency with version requirement, optional flag, default-features flag, enabled features, kind (normal/build/dev) and target. Defaults to the latest non-yanked version when 'version' is omitted.")]
    async fn rustdocs_dependencies(
        &self,
        Parameters(req): Parameters<DepsRequest>,
    ) -> String {
        let version = match self.version_or_latest(&req.krate, &req.version).await {
            Ok(v) => v,
            Err(e) => return err_json(&e),
        };
        match crates_io::dependencies(&self.client, &req.krate, &version).await {
            Ok(d) => serde_json::to_string_pretty(&d).unwrap_or_else(|_| "[]".into()),
            Err(e) => err_json(&e),
        }
    }

    /// Get one item (type, function, trait, module) at a path.
    #[tool(description = "Fetch a single item from a crate's rustdoc JSON by path, e.g. 'axum::Router', 'serde::Serialize', 'tokio::time::sleep'. Re-exports are followed to the canonical item. Returns JSON: resolved path, kind, one-line signature, full doc text, source location (file:line), and module contents when the item is a module. If the path is ambiguous, returns candidate paths to disambiguate.")]
    async fn rustdocs_get_item(
        &self,
        Parameters(req): Parameters<ItemPathRequest>,
    ) -> String {
        self.item_at(&req.krate, &req.version, &req.path).await
    }

    /// List items inside a module.
    #[tool(description = "List the items (structs, functions, traits, submodules, ...) directly inside a module path, e.g. 'axum::routing'. Returns each item's name, kind, canonical path and a short doc snippet. Combine with rustdocs_get_item for full details.")]
    async fn rustdocs_list_module(
        &self,
        Parameters(req): Parameters<ItemPathRequest>,
    ) -> String {
        let (version, docs) = match self.load(&req.krate, &req.version).await {
            Ok(v) => v,
            Err(e) => return err_json(&e),
        };
        let id = match resolve_id(&docs, &req.path) {
            Ok((id, _)) => id,
            Err(e) => return err_json(&anyhow::anyhow!("{e} in {}@{version}", req.krate)),
        };
        serde_json::json!({
            "crate": req.krate,
            "version": version,
            "module": rustdoc::id_path(&docs, id),
            "items": rustdoc::module_items(&docs, id),
        })
        .to_string()
    }

    /// Inherent methods of a type.
    #[tool(description = "List the inherent (no-trait) methods of a type at a path, e.g. 'axum::Router'. Returns each method's name, one-line signature and short doc snippet. Use rustdocs_list_impls for trait implementations.")]
    async fn rustdocs_list_methods(
        &self,
        Parameters(req): Parameters<ItemPathRequest>,
    ) -> String {
        let (version, docs) = match self.load(&req.krate, &req.version).await {
            Ok(v) => v,
            Err(e) => return err_json(&e),
        };
        let id = match resolve_id(&docs, &req.path) {
            Ok((id, _)) => id,
            Err(e) => return err_json(&anyhow::anyhow!("{e} in {}@{version}", req.krate)),
        };
        serde_json::json!({
            "crate": req.krate,
            "version": version,
            "type": rustdoc::id_path(&docs, id),
            "methods": rustdoc::inherent_methods(&docs, id),
        })
        .to_string()
    }

    /// Trait implementations of a type.
    #[tool(description = "List the trait implementations of a type at a path, e.g. 'serde_json::Value' or 'axum::Router'. Returns each trait name, impl generics, negative/synthetic flags and the provided method names. Answers questions like 'is X Send/Sync?' and 'what traits does X implement?'.")]
    async fn rustdocs_list_impls(
        &self,
        Parameters(req): Parameters<ItemPathRequest>,
    ) -> String {
        let (version, docs) = match self.load(&req.krate, &req.version).await {
            Ok(v) => v,
            Err(e) => return err_json(&e),
        };
        let id = match resolve_id(&docs, &req.path) {
            Ok((id, _)) => id,
            Err(e) => return err_json(&anyhow::anyhow!("{e} in {}@{version}", req.krate)),
        };
        serde_json::json!({
            "crate": req.krate,
            "version": version,
            "type": rustdoc::id_path(&docs, id),
            "impls": rustdoc::trait_impls(&docs, id),
        })
        .to_string()
    }

    /// Substring search over item names in a crate.
    #[tool(description = "Search a crate's rustdoc JSON for items whose name contains the query (case-insensitive substring), e.g. query 'builder' or 'IntoIter'. Returns up to 'limit' items (default 30, max 100) with name, kind, canonical path and doc snippet. Use to find where something lives before calling rustdocs_get_item.")]
    async fn rustdocs_search_items(
        &self,
        Parameters(req): Parameters<SearchItemsRequest>,
    ) -> String {
        let (version, docs) = match self.load(&req.krate, &req.version).await {
            Ok(v) => v,
            Err(e) => return err_json(&e),
        };
        let limit = req.limit.unwrap_or(30).min(100) as usize;
        serde_json::json!({
            "crate": req.krate,
            "version": version,
            "query": req.query,
            "matches": rustdoc::search_items(&docs, &req.query, limit),
        })
        .to_string()
    }

    /// Fetch a raw source file from docs.rs.
    #[tool(description = "Fetch a raw source file of a crate version from the crates.io .crate archive (static.crates.io). Path is relative to the crate root: 'src/lib.rs', 'src/main.rs', 'Cargo.toml', 'examples/hello.rs', 'tests/x.rs', etc. Returns the file contents (truncated to 20000 chars). Use when doc strings are not enough and you need the actual implementation.")]
    async fn rustdocs_get_source(
        &self,
        Parameters(req): Parameters<SourceRequest>,
    ) -> String {
        let version = match self.version_or_latest(&req.krate, &req.version).await {
            Ok(v) => v,
            Err(e) => return err_json(&e),
        };
        match docs_rs::get_source(&self.client, &req.krate, &version, &req.path).await {
            Ok(text) => {
                let mut text = text;
                let truncated = text.chars().count() > 20000;
                if truncated {
                    text = text.chars().take(20000).collect();
                }
                serde_json::json!({
                    "crate": req.krate,
                    "version": version,
                    "path": req.path,
                    "truncated": truncated,
                    "source": text,
                })
                .to_string()
            }
            Err(e) => err_json(&e),
        }
    }
}

#[tool_handler]
impl ServerHandler for RustdocsServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Rust crate documentation server backed by crates.io and docs.rs rustdoc JSON. \
             Typical flow: rustdocs_resolve_version to pin a version, then rustdocs_get_item \
             for a path like 'axum::Router', rustdocs_list_methods / rustdocs_list_impls for a \
             type, rustdocs_search_items to find where something lives, rustdocs_search_crates \
             to discover crates, rustdocs_get_source for the implementation, \
             rustdocs_dependencies for the dependency tree.",
        )
    }
}

fn err_json(e: &anyhow::Error) -> String {
    serde_json::json!({ "error": e.to_string() }).to_string()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let service = RustdocsServer::new();
    let running = service.serve(stdio()).await?;
    running.waiting().await?;
    Ok(())
}

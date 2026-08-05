rustdocs-mcp

MCP server exposing Rust crate documentation to AI assistants, built on the
public crates.io HTTP API and the docs.rs rustdoc JSON download endpoint.

Public repository: https://github.com/JLFN/rustdocs-mcp

Data sources

- crates.io HTTP API (api.crates.io/v1): search, crate metadata, versions,
  dependencies. Public JSON, no authentication, User-Agent header required.
- docs.rs rustdoc JSON: https://docs.rs/crate/{name}/{version}/json.zst is the
  full structured API surface of a crate (every item, signature, doc string,
  source span), zstd-compressed. Parsed with rustdoc-types and cached in
  memory per crate@version for the lifetime of the server process.

Tools

- rustdocs_search_crates   search crates.io by query
- rustdocs_crate_metadata  metadata for one crate
- rustdocs_list_versions   all versions of a crate
- rustdocs_resolve_version resolve a semver requirement to a concrete version
- rustdocs_dependencies    dependencies of a crate version
- rustdocs_get_item        item at a path (signature, docs, span); re-exports followed
- rustdocs_list_module     items inside a module
- rustdocs_list_methods    inherent methods of a type
- rustdocs_list_impls      trait implementations of a type
- rustdocs_search_items    substring search over item names in a crate
- rustdocs_get_source      raw source file from docs.rs

Build

Standard builder: bash /data/build/linux/build.sh -p /data/rustdocs-mcp-rs
Installs the binary to ~/.local/bin/rustdocs-mcp. For iteration, cargo check
and cargo build directly so the incremental cache survives.

From the repository:

    cargo install --git https://github.com/JLFN/rustdocs-mcp

This installs the rustdocs-mcp binary into ~/.cargo/bin.

Install into Open Grok

Add to ~/.opengrok/config.toml:

    [mcp_servers.rustdocs]
    command = "rustdocs-mcp"
    enabled = true

Then refresh with /mcps (press r) or start a new session.

Known limitations

- docs.rs only builds rustdoc JSON from recent nightly rustdoc output; crates
  whose docs.rs build is too old will 404 on the json.zst endpoint and the
  tool reports the HTTP error.
- rustdoc JSON can be large (megabytes per crate); first fetch per
  crate@version takes a moment, subsequent calls are served from memory.

License

MIT OR Apache-2.0, at your option.

# rustdocs-mcp — Rust crate documentation for AI assistants

[![crates.io](https://img.shields.io/crates/v/rustdocs-mcp.svg?style=for-the-badge&color=fc8d62&logo=rust)](https://crates.io/crates/rustdocs-mcp)
[![docs.rs](https://img.shields.io/badge/docs.rs-rustdocs_mcp-66c2a5?style=for-the-badge&labelColor=555555&logo=docs.rs)](https://docs.rs/rustdocs-mcp)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-97ca00?style=for-the-badge)](LICENSE-MIT)
[![github](https://img.shields.io/badge/github-JLFN/rustdocs-mcp-8da0cb?style=for-the-badge&labelColor=555555&logo=github)](https://github.com/JLFN/rustdocs-mcp)

An MCP server that answers questions about any published Rust crate from
structured data — no HTML scraping, no cloning the repository. It reads two
public sources:

- **crates.io HTTP API** for search, metadata, versions, and dependencies.
- **docs.rs rustdoc JSON** for the full structured API surface of a crate —
  every item, signature, doc string, and source span — plus the raw source
  from the crate's published archive.

The tools run over MCP stdio and return JSON, so any MCP-capable client
(Open Grok, Claude, Codex, or your own tooling) can ask questions like "what
is the latest axum", "how do I use `TorClient`", "does `Value` implement
`Send`", or "show me the source of `serde::Deserialize`".

- **Crate discovery** — search crates.io by query, fetch metadata, list every
  published version with release dates and MSRV.
- **Version resolution** — resolve a semver requirement (`*`, `1.2.3`, `^1.5`,
  `>=1, <2`) to a concrete non-yanked version.
- **Item lookup** — one item at a path (`axum::Router`) with signature, full
  doc text, and source location; re-exports are followed automatically.
- **API surface** — list a type's inherent methods, its trait
  implementations, or the contents of a module; substring-search item names.
- **Raw source** — read any file from a version's `.crate` archive
  (`src/lib.rs`, `Cargo.toml`, examples) without downloading the repository.
- **Caching** — rustdoc JSON is fetched once per crate@version and served
  from memory afterwards.

## Installation

```console
cargo install rustdocs-mcp
```

This installs the `rustdocs-mcp` binary into `~/.cargo/bin`. Alternatively,
install directly from the repository:

```console
cargo install --git https://github.com/JLFN/rustdocs-mcp
```

## Configure with Open Grok

Add the server to `~/.opengrok/config.toml`:

```toml
[mcp_servers.rustdocs]
command = "rustdocs-mcp"
enabled = true
```

Refresh the MCP list with `/mcps` (press `r`) or restart, and the tools are
available. The repository also ships an Open Grok skill at
`skills/rustdocs/SKILL.md` that teaches the agent when and how to use each
tool; install it with:

```console
cp -r skills/rustdocs ~/.opengrok/skills/rustdocs
```


## Tools

| Tool | Answers |
| --- | --- |
| `rustdocs_search_crates` | Which crates match a query? |
| `rustdocs_crate_metadata` | What is this crate (links, downloads, description)? |
| `rustdocs_list_versions` | What versions exist, and when / with which MSRV? |
| `rustdocs_resolve_version` | Which concrete version satisfies a semver requirement? |
| `rustdocs_dependencies` | What does this version depend on? |
| `rustdocs_get_item` | Show me this item's signature, docs, and location. |
| `rustdocs_list_module` | What is inside this module? |
| `rustdocs_list_methods` | What methods does this type have? |
| `rustdocs_list_impls` | Which traits does this type implement? |
| `rustdocs_search_items` | Find items by name inside a crate. |
| `rustdocs_get_source` | Show me the raw source of a file. |

## Example

Resolving the latest axum, inspecting `Router`, and listing its methods:

1. `rustdocs_resolve_version(krate: "axum")` — returns `0.8.9`.
2. `rustdocs_get_item(krate: "axum", path: "axum::Router")` — returns the
   struct record: docs, resolved path `axum::routing::Router`, signature, and
   span `src/routing/mod.rs:68`.
3. `rustdocs_list_methods(krate: "axum", path: "axum::Router")` — returns
   `route`, `nest`, `merge`, `layer`, `fallback`, `with_state`, and more, each
   with its real signature.

## Development

```console
cargo build --release
```

The test flow is to run the server and probe it over stdio:

```console
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"probe","version":"0.1"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  | target/release/rustdocs-mcp
```

## Known limitations

- docs.rs builds rustdoc JSON from recent nightly rustdoc only. Crates whose
  docs.rs build is too old 404 on the `json.zst` endpoint, or carry an older
  JSON format that this server rejects with a clear error — it never returns
  garbage.
- rustdoc JSON can be several megabytes per crate. The first fetch per
  crate@version takes a moment; subsequent calls are served from memory.

## License

Licensed under either of [Apache License, Version
2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

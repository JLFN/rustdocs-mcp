---
name: rustdocs
description: >
  Use the rustdocs MCP server (crates.io + docs.rs rustdoc JSON) for any
  question about published Rust crates: latest versions, item signatures,
  methods, trait impls, module contents, dependencies, and source files.
  Use when the user asks "what is the latest version of X", "how do I use X",
  "what methods does X have", "does X implement Send", "show me the source
  of X", or runs /rustdocs.
argument-hint: "[crate] [path] [version]"
when-to-use: rust docs, crate docs, latest version of crate, what methods does X have, how do i use X, does X implement, crate source, rustdocs, /rustdocs, axum, serde, tokio
user-invocable: true
---

# Rustdocs — Rust Crate Documentation

Answer questions about any published Rust crate with the rustdocs MCP server. It is the in-house server (project rustdocs-mcp, binary rustdocs-mcp, installable via cargo install --git https://github.com/JLFN/rustdocs-mcp) and reads two public data sources: the crates.io HTTP API for search, metadata, versions, and dependencies, and docs.rs rustdoc JSON for the full structured API surface of a crate. Tools are invoked as rustdocs__rustdocs_<name>.

## When to Use

- "What is the latest version of X?" / "does version Y exist?" / "is X ready?"
- "How do I use X?" / "what methods does X have?" / "show me the signature of X::Y"
- "Which traits does X implement?" / "is X Send/Sync?"
- "What is in module X?" / "find the item Y in crate X"
- "What does crate X depend on?" / "show me the source of X"
- Any build or integration question that needs a crate's real API, not guesses.

## The Tools

1. rustdocs_search_crates — search crates.io by query (name, description, keywords). Use to discover crates or check one exists.
2. rustdocs_crate_metadata — one crate's metadata: latest version, downloads, description, docs/repository/homepage links.
3. rustdocs_list_versions — every published version with yanked status, downloads, release date, MSRV.
4. rustdocs_resolve_version — resolve a semver requirement ("*", "1.2.3", "^1.5", ">=1, <2") to a concrete non-yanked version.
5. rustdocs_dependencies — dependency list of a version: req, optional, features, kind, target.
6. rustdocs_get_item — one item at a path ("axum::Router"): resolved path, kind, one-line signature, full doc text, source location, module contents if it is a module. Re-exports are followed.
7. rustdocs_list_module — items directly inside a module path, with kinds and doc snippets.
8. rustdocs_list_methods — inherent methods of a type, with signatures and doc snippets.
9. rustdocs_list_impls — trait implementations of a type, with impl generics, negative/synthetic flags, and provided methods.
10. rustdocs_search_items — case-insensitive substring search over item names inside a crate.
11. rustdocs_get_source — a raw source file from the crate archive (path relative to crate root, e.g. "src/lib.rs", "Cargo.toml").

## Workflow

1. Pin the version. Omit the version argument to use the latest non-yanked release; pass a concrete "MAJOR.MINOR.PATCH" to pin. For a semver requirement, call rustdocs_resolve_version first and pass the returned version.
2. Orient. For an unfamiliar crate: rustdocs_search_crates or rustdocs_crate_metadata, then rustdocs_get_item on the crate root or a known path.
3. Drill in. rustdocs_list_module to explore a module, rustdocs_list_methods and rustdocs_list_impls for a type, rustdocs_search_items to locate something by name, rustdocs_get_source for the implementation, rustdocs_dependencies for the dependency tree.

## Rules

- Paths take the form "crate::mod::Item" ("axum::Router"). Re-export paths are accepted and resolved to the canonical item.
- The first call for a crate@version downloads the rustdoc JSON (a few megabytes) and takes a moment; later calls for the same version are served from an in-memory cache.
- Crates whose docs.rs build predates rustdoc JSON will 404 or fail to parse with a clear error; report that to the user rather than guessing.
- For crates checked out locally on this machine (under /data), read the local source directly instead of the MCP; rustdocs is for published crates.
- If the server is not listed, check it with: open-grok mcp doctor rustdocs.

## Related Rust Skills

Several Rust skills exist on this machine; route to the right one:

- rust-build: build any Rust project with the standard /data/build builder (release binary into bin/).
- crates-io: publish, yank, and manage this machine's crates on crates.io (cargo publish).
- repo-docs: standard repo layout, README template, and badges for any Rust repository.
- rust-env: install, update, or verify the Rust toolchain (rustup, rustc, cargo) and system build dependencies.
- rust-dev-standards: quality baseline, CI pipeline, and linting rules when writing, reviewing, or editing Rust code.
- develop-open-grok: building or changing the open-grok checkout specifically (repo-level skill).

rustdocs is only for questions about published crates (versions, APIs, docs, source). For building, publishing, toolchain, or code-quality work, use the matching skill above instead.

## Example

User: "What is the latest axum version and how do I add a route?"
Flow: rustdocs_resolve_version(axum) for the version, rustdocs_get_item(axum::Router) for the type, rustdocs_list_methods(axum::Router) for route/nest/layer/with_state signatures.

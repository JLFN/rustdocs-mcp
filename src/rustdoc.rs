//! Helpers over the parsed rustdoc JSON (`rustdoc_types::Crate`).
//!
//! The rustdoc JSON is a graph: `index` maps item id -> item, `paths` maps
//! item id -> a summary with the canonical path ("axum::Router"). Tools in
//! `main.rs` resolve a path string to an id, then extract signatures, methods,
//! impls, or module contents.

use rustdoc_types::{
    Crate, DynTrait, Enum, Function, GenericArgs, Id, Impl, Item, ItemEnum, ItemKind, Struct,
    StructKind, Type,
};

/// Human-readable kind name for an item.
pub fn item_kind_name(kind: &ItemKind) -> String {
    let name = match kind {
        ItemKind::Module => "module",
        ItemKind::ExternCrate => "extern crate",
        ItemKind::Use => "import",
        ItemKind::Struct => "struct",
        ItemKind::StructField => "field",
        ItemKind::Union => "union",
        ItemKind::Enum => "enum",
        ItemKind::Variant => "variant",
        ItemKind::Function => "function",
        ItemKind::TypeAlias => "type alias",
        ItemKind::Constant => "constant",
        ItemKind::Trait => "trait",
        ItemKind::TraitAlias => "trait alias",
        ItemKind::Impl => "impl",
        ItemKind::Static => "static",
        ItemKind::ExternType => "extern type",
        ItemKind::Macro => "macro",
        ItemKind::ProcAttribute => "proc macro (attribute)",
        ItemKind::ProcDerive => "proc macro (derive)",
        ItemKind::AssocConst => "associated constant",
        ItemKind::AssocType => "associated type",
        ItemKind::Primitive => "primitive",
        ItemKind::Keyword => "keyword",
        ItemKind::Attribute => "attribute",
        _ => "item",
    };
    name.to_string()
}

/// Best-effort one-line signature for an item, depending on its kind.
pub fn signature(docs: &Crate, item: &Item) -> String {
    let name = item.name.as_deref().unwrap_or("");
    match &item.inner {
        ItemEnum::Function(Function { sig, generics, .. }) => {
            let inputs: Vec<String> = sig
                .inputs
                .iter()
                .map(|(n, t)| format!("{n}: {}", render_type(t)))
                .collect();
            let mut out = format!("fn {name}{}({})", render_generics(generics), inputs.join(", "));
            if sig.is_c_variadic {
                out.push_str(", ...");
            }
            if let Some(t) = &sig.output {
                out.push_str(&format!(" -> {}", render_type(t)));
            }
            out
        }
        ItemEnum::Struct(s) => format!("struct {name}{} {}", render_generics(&s.generics), render_struct(s, docs)),
        ItemEnum::Union(u) => {
            let fields: Vec<String> = u
                .fields
                .iter()
                .filter_map(|id| docs.index.get(id))
                .map(|f| field_repr(f))
                .collect();
            format!("union {name}{} {{ {} }}", render_generics(&u.generics), fields.join(", "))
        }
        ItemEnum::Enum(e) => format!(
            "enum {name}{} {{ {} }}",
            render_generics(&e.generics),
            variant_names(docs, e).join(", ")
        ),
        ItemEnum::Trait(t) => {
            let items: Vec<String> = t
                .items
                .iter()
                .filter_map(|id| docs.index.get(id))
                .filter_map(|it| it.name.clone())
                .collect();
            format!("trait {name}{} {{ {} }}", render_generics(&t.generics), items.join(", "))
        }
        ItemEnum::TraitAlias(ta) => format!("trait alias {name} ({} bounds)", ta.params.len()),
        ItemEnum::TypeAlias(ta) => {
            format!("type {name}{} = {}", render_generics(&ta.generics), render_type(&ta.type_))
        }
        ItemEnum::Static(s) => format!("static {name}: {}", render_type(&s.type_)),
        ItemEnum::Constant { type_, const_ } => {
            format!("const {name}: {} = {}", render_type(type_), const_.expr)
        }
        ItemEnum::Module(m) => format!("module {name} ({} items)", m.items.len()),
        ItemEnum::Impl(imp) => render_impl_header(imp),
        ItemEnum::Macro(_) => format!("macro {name}"),
        ItemEnum::ProcMacro(_) => format!("proc macro {name}"),
        ItemEnum::Primitive(p) => format!("primitive {}", p.name),
        ItemEnum::Use(u) => format!("use {} as {name}", u.source),
        ItemEnum::AssocConst { type_, .. } => format!("associated const {name}: {}", render_type(type_)),
        ItemEnum::AssocType { type_, .. } => match type_ {
            Some(t) => format!("associated type {name} = {}", render_type(t)),
            None => format!("associated type {name}"),
        },
        ItemEnum::StructField(t) => format!("{name}: {}", render_type(t)),
        ItemEnum::Variant(_) => format!("variant {name}"),
        ItemEnum::ExternType => format!("extern type {name}"),
        other => format!("{name} ({})", item_kind_name(&other.item_kind())),
    }
}

/// Resolve a "::"-separated path string ("axum::Router") to an item id and
/// its canonical path. Exact, case-sensitive match over the paths map.
pub fn resolve_path(docs: &Crate, path: &str) -> Option<(Id, String)> {
    let wanted: Vec<&str> = path.split("::").filter(|s| !s.is_empty()).collect();
    docs.paths
        .iter()
        .find(|(_, s)| s.path.iter().map(|p| p.as_str()).eq(wanted.iter().copied()))
        .map(|(id, s)| (*id, s.path.join("::")))
}

/// Case-insensitive search by name; returns ids of every matching item.
pub fn find_by_name(docs: &Crate, name: &str) -> Vec<Id> {
    let needle = name.to_lowercase();
    docs.index
        .values()
        .filter(|it| {
            it.name
                .as_deref()
                .map(|n| n.to_lowercase().contains(&needle))
                .unwrap_or(false)
        })
        .map(|it| it.id)
        .collect()
}

/// Best display name for an item: its own name, or the inner name for
/// `use` items (which carry no `Item.name` in the JSON).
pub fn display_name(item: &Item) -> String {
    if let Some(n) = item.name.as_deref() {
        return n.to_string();
    }
    match &item.inner {
        ItemEnum::Use(u) => u.name.clone(),
        _ => String::new(),
    }
}

/// Canonical path string for an id (from the paths map), or a readable
/// fallback (use source / bare name) for items without a canonical path.
pub fn id_path(docs: &Crate, id: Id) -> String {
    if let Some(s) = docs.paths.get(&id) {
        return s.path.join("::");
    }
    match docs.index.get(&id) {
        Some(Item { inner: ItemEnum::Use(u), .. }) => format!("use {}", u.source),
        Some(it) => it.name.clone().unwrap_or_else(|| format!("#{:?}", id)),
        None => format!("#{:?}", id),
    }
}

/// Items directly inside a module (names, kinds, paths, docs snippets).
pub fn module_items(docs: &Crate, module_id: Id) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let Some(item) = docs.index.get(&module_id) else {
        return out;
    };
    let ItemEnum::Module(m) = &item.inner else {
        return out;
    };
    for id in &m.items {
        if let Some(it) = docs.index.get(id) {
            out.push(serde_json::json!({
                "name": display_name(it),
                "kind": item_kind_name(&it.inner.item_kind()),
                "path": id_path(docs, *id),
                "docs": snippet(it.docs.as_deref().unwrap_or(""), 200),
            }));
        }
    }
    out
}

/// Inherent methods of a type (impl blocks without a trait).
pub fn inherent_methods(docs: &Crate, type_id: Id) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for item in docs.index.values() {
        let ItemEnum::Impl(imp) = &item.inner else { continue };
        if imp.trait_.is_some() || imp.is_negative {
            continue;
        }
        if !type_matches(&imp.for_, type_id) {
            continue;
        }
        for id in &imp.items {
            let Some(m) = docs.index.get(id) else { continue };
            if let ItemEnum::Function(_) = &m.inner {
                out.push(serde_json::json!({
                    "name": m.name.as_deref().unwrap_or(""),
                    "signature": signature(docs, m),
                    "docs": snippet(m.docs.as_deref().unwrap_or(""), 200),
                }));
            }
        }
    }
    out
}

/// Trait implementations of a type (impl blocks with a trait).
pub fn trait_impls(docs: &Crate, type_id: Id) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    for item in docs.index.values() {
        let ItemEnum::Impl(imp) = &item.inner else { continue };
        let Some(trait_path) = &imp.trait_ else { continue };
        if !type_matches(&imp.for_, type_id) {
            continue;
        }
        let method_names: Vec<String> = imp
            .items
            .iter()
            .filter_map(|id| docs.index.get(id))
            .filter_map(|m| m.name.clone())
            .collect();
        out.push(serde_json::json!({
            "trait": trait_path.path,
            "generics": render_generics(&imp.generics),
            "negative": imp.is_negative,
            "synthetic": imp.is_synthetic,
            "provided_methods": method_names,
        }));
    }
    out
}

/// Substring search over item names.
pub fn search_items(docs: &Crate, query: &str, limit: usize) -> Vec<serde_json::Value> {
    let needle = query.to_lowercase();
    let mut out: Vec<serde_json::Value> = Vec::new();
    for item in docs.index.values() {
        let name = display_name(item);
        if name.is_empty() || !name.to_lowercase().contains(&needle) {
            continue;
        }
        out.push(serde_json::json!({
            "name": name,
            "kind": item_kind_name(&item.inner.item_kind()),
            "path": id_path(docs, item.id),
            "docs": snippet(item.docs.as_deref().unwrap_or(""), 160),
        }));
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// Where an item is defined, as "file:line" when the span is present.
pub fn span_str(item: &Item) -> Option<String> {
    item.span
        .as_ref()
        .map(|s| format!("{}:{}", s.filename.display(), s.begin.0))
}

/// Group candidate ids by their canonical path, for disambiguation output.
pub fn candidate_rows(docs: &Crate, ids: &[Id]) -> Vec<serde_json::Value> {
    ids.iter()
        .filter_map(|id| docs.index.get(id))
        .map(|it| {
            serde_json::json!({
                "name": display_name(it),
                "kind": item_kind_name(&it.inner.item_kind()),
                "path": id_path(docs, it.id),
                "docs": snippet(it.docs.as_deref().unwrap_or(""), 160),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Rendering helpers
// ---------------------------------------------------------------------------

fn render_struct(s: &Struct, docs: &Crate) -> String {
    match &s.kind {
        StructKind::Unit => "{ }".to_string(),
        StructKind::Tuple(ids) => {
            let inner: Vec<String> = ids
                .iter()
                .filter_map(|id| id.as_ref())
                .filter_map(|id| docs.index.get(id))
                .map(field_repr)
                .collect();
            format!("( {} )", inner.join(", "))
        }
        StructKind::Plain { fields, .. } => {
            let inner: Vec<String> = fields
                .iter()
                .filter_map(|id| docs.index.get(id))
                .map(field_repr)
                .collect();
            format!("{{ {} }}", inner.join(", "))
        }
    }
}

fn field_repr(f: &Item) -> String {
    let name = f.name.as_deref().unwrap_or("_");
    match &f.inner {
        ItemEnum::StructField(t) => format!("{name}: {}", render_type(t)),
        _ => format!("{name}: ?"),
    }
}

fn variant_names(docs: &Crate, e: &Enum) -> Vec<String> {
    e.variants
        .iter()
        .filter_map(|id| docs.index.get(id))
        .filter_map(|v| v.name.clone())
        .collect()
}

fn render_impl_header(imp: &Impl) -> String {
    let for_ = render_type(&imp.for_);
    match &imp.trait_ {
        Some(t) => format!("impl {} for {}", t.path, for_),
        None => format!("impl {for_}"),
    }
}

fn render_generics(g: &rustdoc_types::Generics) -> String {
    let names: Vec<&str> = g.params.iter().map(|p| p.name.as_str()).collect();
    if names.is_empty() {
        String::new()
    } else {
        format!("<{}>", names.join(", "))
    }
}

fn type_matches(t: &Type, id: Id) -> bool {
    match t {
        Type::ResolvedPath(p) => p.id == id,
        _ => false,
    }
}

/// Render a rustdoc Type to a compact one-line string.
pub fn render_type(t: &Type) -> String {
    match t {
        Type::ResolvedPath(p) => {
            let base = p.path.rsplit("::").next().unwrap_or(&p.path).to_string();
            match &p.args {
                Some(args) => format!("{base}{}", render_generic_args(args)),
                None => base,
            }
        }
        Type::Generic(s) => s.clone(),
        Type::Primitive(s) => s.clone(),
        Type::DynTrait(d) => render_dyn_trait(d),
        Type::FunctionPointer(fp) => {
            let inputs: Vec<String> = fp
                .sig
                .inputs
                .iter()
                .map(|(n, t)| format!("{n}: {}", render_type(t)))
                .collect();
            match &fp.sig.output {
                Some(o) => format!("fn({}) -> {}", inputs.join(", "), render_type(o)),
                None => format!("fn({})", inputs.join(", ")),
            }
        }
        Type::Tuple(ts) => {
            let inner: Vec<String> = ts.iter().map(render_type).collect();
            format!("({})", inner.join(", "))
        }
        Type::Slice(t) => format!("[{}]", render_type(t)),
        Type::Array { type_, len } => format!("[{}; {len}]", render_type(type_)),
        Type::Pat { type_, .. } => render_type(type_),
        Type::ImplTrait(bounds) => format!("impl ({} bounds)", bounds.len()),
        Type::Infer => "_".into(),
        Type::RawPointer { is_mutable, type_ } => {
            let m = if *is_mutable { "mut " } else { "" };
            format!("*{m}{}", render_type(type_))
        }
        Type::BorrowedRef { lifetime, is_mutable, type_ } => {
            let l = lifetime.as_deref().map(|l| format!("{l} ")).unwrap_or_default();
            let m = if *is_mutable { "mut " } else { "" };
            format!("&{l}{m}{}", render_type(type_))
        }
        Type::QualifiedPath { name, self_type, trait_, .. } => match trait_ {
            Some(t) => format!("<{} as {}>::{name}", render_type(self_type), t.path),
            None => format!("{}::{name}", render_type(self_type)),
        },
    }
}

fn render_generic_args(args: &GenericArgs) -> String {
    match args {
        GenericArgs::AngleBracketed { args, .. } => {
            let inner: Vec<String> = args
                .iter()
                .map(|a| match a {
                    rustdoc_types::GenericArg::Lifetime(s) => s.clone(),
                    rustdoc_types::GenericArg::Type(t) => render_type(t),
                    rustdoc_types::GenericArg::Const(c) => c.expr.clone(),
                    rustdoc_types::GenericArg::Infer => "_".into(),
                })
                .collect();
            format!("<{}>", inner.join(", "))
        }
        GenericArgs::Parenthesized { inputs, output } => {
            let ins: Vec<String> = inputs.iter().map(render_type).collect();
            match output {
                Some(o) => format!("({}) -> {}", ins.join(", "), render_type(o)),
                None => format!("({})", ins.join(", ")),
            }
        }
        GenericArgs::ReturnTypeNotation => "(..)".into(),
    }
}

fn render_dyn_trait(d: &DynTrait) -> String {
    let first = d.traits.first().map(|t| {
        t.trait_
            .path
            .rsplit("::")
            .next()
            .unwrap_or(&t.trait_.path)
            .to_string()
    });
    match first {
        Some(n) => format!("dyn {n}"),
        None => "dyn".into(),
    }
}

/// First `max` chars of a doc string, normalized to one line.
pub fn snippet(docs: &str, max: usize) -> String {
    let one_line: String = docs
        .chars()
        .map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c })
        .collect();
    let joined: String = one_line.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() > max {
        joined.chars().take(max).collect::<String>() + "..."
    } else {
        joined
    }
}

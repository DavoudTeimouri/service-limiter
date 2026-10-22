//! Docs must not reference code that no longer exists.
//!
//! Stale doc references are easy to miss and misleading to a reader: the
//! README and LOGIC.md quote function signatures, and a renamed function
//! leaves the docs describing an API that is gone. This test fails when a
//! doc names a `platform::` symbol that is not in the source.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {rel}: {e}"))
}

/// Every `platform::<fn>` symbol named in the docs.
///
/// Matches both call sites (`platform::foo(`) and backticked mentions
/// (`` `platform::foo` ``). Module names are excluded: `platform::linux` is a
/// module, and modules are not part of the exported surface.
fn referenced_symbols(doc: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = doc;
    while let Some(i) = rest.find("platform::") {
        rest = &rest[i + "platform::".len()..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        // A function is referenced either by calling it or by naming it in
        // backticks. A module is named in neither position.
        let after = &rest[name.len()..];
        if after.starts_with('(') || after.starts_with('`') {
            out.push(name);
        }
    }
    out
}

/// The `platform::` symbols the source actually exposes.
///
/// Two sources: the `pub use` re-exports of the per-OS modules, and the
/// `pub fn` items defined directly in `platform/mod.rs`.
fn real_symbols() -> Vec<String> {
    let src = read("src/platform/mod.rs");
    let mut out = Vec::new();

    for line in src.lines() {
        let line = line.trim();

        if let Some(rest) = line.strip_prefix("pub use ") {
            if let Some((_, names)) = rest.split_once('{') {
                if let Some((names, _)) = names.split_once('}') {
                    for name in names.split(',') {
                        let name = name.trim();
                        if !name.is_empty() {
                            out.push(name.to_string());
                        }
                    }
                }
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("pub fn ") {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                out.push(name);
            }
        }
    }
    out
}

#[test]
fn docs_only_reference_platform_symbols_that_exist() {
    let real = real_symbols();
    assert!(
        real.contains(&"discover_services".to_string()),
        "could not parse the platform exports: {real:?}"
    );

    for doc in ["README.md", "LOGIC.md"] {
        let text = read(doc);
        for symbol in referenced_symbols(&text) {
            assert!(
                real.contains(&symbol),
                "{doc} references platform::{symbol}, which does not exist. \
                 Real exports: {real:?}"
            );
        }
    }
}

#[test]
fn readme_documents_every_command_the_cli_exposes() {
    let readme = read("README.md");
    let main_rs = read("src/main.rs");

    // Each subcommand variant in the clap enum must be documented. The enum
    // uses Rust variant names, so match the capitalised form.
    for cmd in ["analyze", "generate", "apply", "revert", "status"] {
        let variant = format!("{} {{", to_camel(cmd));
        assert!(
            main_rs.contains(&variant),
            "expected a `{variant}` subcommand in the CLI enum"
        );
        assert!(
            readme.contains(&format!("service-limiter {cmd}")),
            "README does not document `service-limiter {cmd}`"
        );
    }
}

/// `analyze` -> `Analyze`
fn to_camel(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

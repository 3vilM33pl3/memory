//! Contract test: the OpenAPI specification (docs/api/openapi.yaml) and the
//! axum router (src/routes.rs) must describe the same path inventory, in both
//! directions. This is what makes the spec trustworthy without a codegen
//! framework: adding, removing, or renaming a route fails this test until the
//! spec is updated, and vice versa.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Route paths registered in routes.rs, via the `.route("...")` literals.
fn router_paths() -> BTreeSet<String> {
    let source = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("routes.rs"),
    )
    .expect("read routes.rs");
    let mut paths = BTreeSet::new();
    let mut rest = source.as_str();
    while let Some(index) = rest.find(".route(") {
        rest = &rest[index + ".route(".len()..];
        let Some(open) = rest.find('"') else { break };
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else { break };
        paths.insert(after[..close].to_string());
        rest = &after[close..];
    }
    paths
}

/// Path keys from the spec: two-space-indented lines ending in `:` that start
/// with `/`, inside the `paths:` section. The spec's formatting is under our
/// control (see docs/api/openapi.yaml), so this stays a plain scan.
fn spec_paths() -> BTreeSet<String> {
    let spec = std::fs::read_to_string(repo_root().join("docs").join("api").join("openapi.yaml"))
        .expect("read docs/api/openapi.yaml");
    let mut in_paths = false;
    let mut paths = BTreeSet::new();
    for line in spec.lines() {
        if line == "paths:" {
            in_paths = true;
            continue;
        }
        if in_paths && !line.starts_with(' ') && !line.is_empty() {
            break; // left the paths: section (e.g. components:)
        }
        if in_paths
            && let Some(stripped) = line.strip_prefix("  /")
            && let Some(path) = stripped.strip_suffix(':')
        {
            paths.insert(format!("/{path}"));
        }
    }
    paths
}

#[test]
fn openapi_spec_matches_router_inventory() {
    let router = router_paths();
    let spec = spec_paths();
    assert!(!router.is_empty() && !spec.is_empty());

    let undocumented: Vec<_> = router.difference(&spec).collect();
    assert!(
        undocumented.is_empty(),
        "routes missing from docs/api/openapi.yaml: {undocumented:?}"
    );

    let phantom: Vec<_> = spec.difference(&router).collect();
    assert!(
        phantom.is_empty(),
        "spec paths with no matching route in routes.rs: {phantom:?}"
    );
}

//! Step 8 wiring probe: every production `trope:` parse site must resolve
//! community groups. The parser itself is tested in the domain crate; this
//! test guards the easier-to-miss half — a surface still calling the seed-only
//! parser would silently disagree with every other surface.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates")
        .parent()
        .expect("workspace")
        .to_path_buf()
}

#[test]
fn every_production_parse_site_uses_the_resolver() {
    let sites: &[(&str, usize)] = &[
        ("crates/db/src/search/ast_search.rs", 1),
        ("crates/db/src/search/forum_ast_search.rs", 1),
        ("crates/db/src/search/user_ast_search.rs", 1),
        ("crates/app/src/routes/media.rs", 2),
        ("crates/app/src/exports.rs", 1),
        ("crates/app/src/bulk_export.rs", 1),
    ];
    for (site, expected) in sites {
        let text = std::fs::read_to_string(root().join(site))
            .unwrap_or_else(|e| panic!("read {site}: {e}"));
        let count = text.matches("parse_query_with_tropes").count();
        assert!(count >= *expected, "{site} has {count} resolver parses, expected {expected}");
        assert!(
            !text.contains("query::parse_query(") && !text.contains("= parse_query("),
            "{site} still calls the seed-only parser"
        );
    }
}

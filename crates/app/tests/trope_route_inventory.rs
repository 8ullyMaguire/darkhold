//! Step 9 probe: the trope proposal doors must be declared and mounted.
//! The generic route_inventory test checks handler audiences; this one checks
//! that the five doors exist at all before that test can judge them.

use darkhold_app::routes::inventory::{Audience, ROUTE_TABLE};
use std::path::Path;

#[test]
fn trope_proposal_routes_are_declared_and_mounted() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let route_file = crate_dir.join("src/routes/trope_proposals.rs");
    assert!(route_file.exists(), "trope_proposals.rs is missing");

    let expected: &[(&str, &str, Audience)] = &[
        ("POST", "/tropes/proposals", Audience::Authenticated),
        ("GET", "/tropes/proposals", Audience::Public),
        ("GET", "/tropes/proposals/{id}", Audience::Public),
        (
            "POST",
            "/tropes/proposals/{id}/vote",
            Audience::Authenticated,
        ),
        ("POST", "/operator/tropes/settle", Audience::Operator),
    ];
    for (method, path, audience) in expected {
        let found = ROUTE_TABLE.iter().any(|entry| {
            entry.method == *method && entry.path == *path && entry.audience == *audience
        });
        assert!(found, "missing route entry {method} {path}");
    }

    let server = std::fs::read_to_string(crate_dir.join("src/server.rs")).expect("server.rs");
    assert!(
        server.contains("routes::trope_proposals::router()"),
        "the trope router is not mounted"
    );
}

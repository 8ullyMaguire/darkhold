use darkhold_app::imports::body_policy_from;
use darkhold_app::imports::BodyPolicy;

#[test]
fn body_policy_defaults_to_inherit() {
    assert_eq!(body_policy_from(&serde_json::json!({})), BodyPolicy::Inherit);
    assert_eq!(
        body_policy_from(&serde_json::json!({"body_policy":"skip"})),
        BodyPolicy::Skip
    );
    assert_eq!(
        body_policy_from(&serde_json::json!({"body_policy":"store"})),
        BodyPolicy::Store
    );
}

#[test]
fn skip_policy_is_checked_before_retention() {
    let source = include_str!("../src/imports.rs");
    let start = source.find("if body_policy == BodyPolicy::Skip {").unwrap();
    let retention = source.find("resolve_for_source").unwrap();
    assert!(
        start < retention,
        "skip must be decided before retention is consulted"
    );
}

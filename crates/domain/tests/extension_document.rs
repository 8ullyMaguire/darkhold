//! M70-09 — a shared document means what it says, or it is refused.

use darkhold_domain::extension_document::{parse, Kind, Refusal, MAX_BYTES, VERSION};

fn json(kind: &str, body: &str) -> String {
    format!(r#"{{"version":{VERSION},"kind":"{kind}","name":"Winter reading","body":{body}}}"#)
}

#[test]
fn a_bundle_of_saved_views_round_trips_byte_identically() {
    let raw = json(
        "saved_views",
        r#"{"views":[{"name":"Slow burn","filters":{"tag":"slow-burn","words_min":40000}},
                     {"name":"Unfinished","filters":{"status":"wip","sort":"kudos"}}]}"#,
    );
    let document = parse(&raw).expect("valid bundle");
    assert_eq!(document.kind, Kind::SavedViews);
    assert_eq!(document.name, "Winter reading");

    let exported = document.to_json();
    let again = parse(&exported).expect("the export must re-import");
    assert_eq!(again, document, "export then import is identity");
    assert_eq!(
        again.to_json(),
        exported,
        "and the bytes are stable, so two instances can compare documents"
    );
}

#[test]
fn an_unknown_field_is_refused_rather_than_ignored() {
    // The field a newer instance might write. Dropping it silently would leave two instances holding "the same"
    // document that arrange the interface differently.
    let raw = r#"{"version":1,"kind":"layout","name":"Mine","body":{"slots":[{"widget":"inbox"}]},"publish_at":"2027-01-01"}"#;
    assert_eq!(
        parse(raw),
        Err(Refusal::Unknown {
            field: "publish_at".to_owned()
        })
    );
    // ...and the same rule one level down: a slot with a field this version does not define.
    let raw = json("layout", r#"{"slots":[{"widget":"inbox","pinned":true}]}"#);
    match parse(&raw) {
        Err(Refusal::BadBody { reason }) => assert!(reason.contains("pinned"), "{reason}"),
        other => panic!("expected a refusal naming the field, got {other:?}"),
    }
}

#[test]
fn a_version_from_the_future_is_refused_not_approximated() {
    let raw =
        r#"{"version":2,"kind":"layout","name":"Future","body":{"slots":[{"widget":"inbox"}]}}"#;
    assert_eq!(parse(raw), Err(Refusal::UnsupportedVersion { version: 2 }));
    assert_eq!(VERSION, 1, "this instance writes exactly one version");
}

#[test]
fn a_layout_may_only_name_widgets_this_instance_has() {
    let good = json(
        "layout",
        r#"{"slots":[{"widget":"continue_reading","span":8},{"widget":"inbox"}]}"#,
    );
    assert!(parse(&good).is_ok());
    let unknown = json("layout", r#"{"slots":[{"widget":"astrology"}]}"#);
    match parse(&unknown) {
        Err(Refusal::BadBody { reason }) => {
            assert!(
                reason.contains("astrology") && reason.contains("known:"),
                "{reason}"
            );
        }
        other => panic!("expected a refusal listing the known widgets, got {other:?}"),
    }
    // A span outside the grid is refused rather than clamped: clamping would silently rearrange someone's layout.
    assert!(matches!(
        parse(&json(
            "layout",
            r#"{"slots":[{"widget":"inbox","span":13}]}"#
        )),
        Err(Refusal::BadBody { .. })
    ));
}

#[test]
fn a_filter_value_is_a_scalar_because_a_bundle_is_not_a_query_language() {
    let nested = json(
        "saved_views",
        r#"{"views":[{"name":"Nested","filters":{"tag":{"any":["a","b"]}}}]}"#,
    );
    match parse(&nested) {
        Err(Refusal::BadBody { reason }) => assert!(reason.contains("tag"), "{reason}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_token_is_a_value_and_never_a_fragment() {
    let good = json(
        "theme_tokens",
        r##"{"tokens":{"--ink":"#101014","--paper":"#fdfdfb","--measure":"42rem"}}"##,
    );
    assert!(parse(&good).is_ok());

    for (value, culprit) in [
        ("red; } body { display: none", ";"),
        ("url(https://track.invalid/p.gif)", "url("),
        ("expression(alert(1))", "expression("),
        ("javascript:alert(1)", "javascript:"),
    ] {
        let raw = json(
            "theme_tokens",
            &format!(r##"{{"tokens":{{"--ink":"{value}"}}}}"##),
        );
        match parse(&raw) {
            Err(Refusal::BadBody { reason }) => assert!(reason.contains(culprit), "{reason}"),
            other => panic!("{value} should have been refused, got {other:?}"),
        }
    }
    // A token name that could not be a custom property is refused before the value is even considered.
    assert!(matches!(
        parse(&json("theme_tokens", r##"{"tokens":{"ink":"#fff"}}"##)),
        Err(Refusal::BadBody { .. })
    ));
}

#[test]
fn bounds_are_refusals_and_the_boundary_belongs_to_the_accepted_side() {
    let seventeen = (1..=21)
        .map(|i| format!(r#"{{"name":"v{i}","filters":{{}}}}"#))
        .collect::<Vec<_>>()
        .join(",");
    match parse(&json(
        "saved_views",
        &format!(r#"{{"views":[{seventeen}]}}"#),
    )) {
        Err(Refusal::BadBody { reason }) => assert!(reason.contains("1..=20"), "{reason}"),
        other => panic!("21 views should be refused, got {other:?}"),
    }
    let empty = json("saved_views", r#"{"views":[]}"#);
    assert!(
        matches!(parse(&empty), Err(Refusal::BadBody { .. })),
        "an empty bundle is not a document"
    );

    let big = " ".repeat(MAX_BYTES + 1);
    assert_eq!(
        parse(&big),
        Err(Refusal::TooLarge {
            bytes: MAX_BYTES + 1
        })
    );
}

#[test]
fn everything_that_is_not_a_document_is_named_as_such() {
    assert!(matches!(parse("not json"), Err(Refusal::BadJson { .. })));
    assert_eq!(parse("[1,2,3]"), Err(Refusal::NotAnObject));
    assert_eq!(
        parse(r#"{"kind":"layout","name":"x","body":{}}"#),
        Err(Refusal::Missing { field: "version" })
    );
    assert_eq!(
        parse(r#"{"version":1,"name":"x","body":{}}"#),
        Err(Refusal::Missing { field: "kind" })
    );
    assert_eq!(
        parse(r#"{"version":1,"kind":"garden","name":"x","body":{}}"#),
        Err(Refusal::UnknownKind {
            kind: "garden".to_owned()
        })
    );
    assert_eq!(
        parse(
            r#"{"version":1,"kind":"layout","name":"   ","body":{"slots":[{"widget":"inbox"}]}}"#
        ),
        Err(Refusal::BadName),
        "whitespace is not a name"
    );
}

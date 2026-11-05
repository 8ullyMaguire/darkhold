//! M70-06 — a skin may describe an interface and may not reach outside it.

use darkhold_domain::reader_skin::{check, Refusal, MAX_BYTES};

#[test]
fn ordinary_css_is_accepted_verbatim() {
    let css = ":root { --ink: #101014; }\nbody { color: var(--ink); }\n@media (prefers-reduced-motion: reduce) \
               { * { animation: none; } }";
    let skin = check(css).expect("ordinary CSS");
    assert_eq!(
        skin.css(),
        css,
        "a skin is served exactly as written; rewriting it would surprise its author"
    );
}

#[test]
fn an_import_is_refused_because_it_would_make_every_other_rule_advisory() {
    assert_eq!(
        check("@import url('https://evil.invalid/x.css');"),
        Err(Refusal::Import)
    );
    assert_eq!(
        check("@IMPORT 'x.css';"),
        Err(Refusal::Import),
        "CSS keywords are case-insensitive"
    );
}

#[test]
fn a_remote_url_is_refused_and_a_data_uri_is_not() {
    assert_eq!(
        check("body { background: url(https://track.invalid/p.gif); }"),
        Err(Refusal::RemoteUrl)
    );
    assert_eq!(
        check("body { background: URL( '/p.gif' ); }"),
        Err(Refusal::RemoteUrl)
    );
    assert!(check("body { background: url(data:image/gif;base64,R0lGOD); }").is_ok());
    assert!(check("body { background: url(\"data:image/png;base64,iVBORw0=\"); }").is_ok());
}

#[test]
fn every_stylesheet_code_path_is_refused_in_any_case() {
    for (css, construct) in [
        (
            "a { background: url(JAVASCRIPT:alert(1)); }",
            "javascript: URL",
        ),
        ("a { width: expression(alert(1)); }", "expression()"),
        ("a { behavior: url(#default#time2); }", "behavior: property"),
        ("a { -moz-binding: url(#x); }", "-moz-binding property"),
    ] {
        match check(css) {
            Err(Refusal::Scriptish { construct: found }) => assert_eq!(found, construct, "{css}"),
            other => panic!("{css} should have been refused, got {other:?}"),
        }
    }
}

#[test]
fn a_style_close_is_refused_because_any_future_inlining_would_end_the_element() {
    assert_eq!(
        check("body { color: red } </style><script>x</script>"),
        Err(Refusal::StyleClose)
    );
}

#[test]
fn size_is_a_refusal_not_a_truncation() {
    let big = "a { color: red; }\n".repeat(MAX_BYTES / 10);
    assert!(big.len() > MAX_BYTES);
    assert_eq!(check(&big), Err(Refusal::TooLarge { bytes: big.len() }));
    // One byte under the cap is fine: the boundary belongs to the accepted side.
    let mut edge = "a{color:red}".to_owned();
    edge.push_str(&" ".repeat(MAX_BYTES - edge.len()));
    assert_eq!(edge.len(), MAX_BYTES);
    assert!(check(&edge).is_ok());
}

#[test]
fn whitespace_is_not_a_skin() {
    assert_eq!(check("   \n\t "), Err(Refusal::Empty));
}

#[test]
fn each_refusal_tells_the_reader_what_to_remove() {
    for refusal in [
        Refusal::TooLarge { bytes: 70000 },
        Refusal::Empty,
        Refusal::Import,
        Refusal::RemoteUrl,
        Refusal::Scriptish {
            construct: "expression()",
        },
        Refusal::StyleClose,
    ] {
        let message = refusal.message();
        assert!(!message.is_empty());
        assert!(
            message.contains("remove") || message.contains("empty") || message.contains("limit"),
            "{message} should say what to do"
        );
    }
}

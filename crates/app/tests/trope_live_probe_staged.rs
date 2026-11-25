//! Step 12 staging probe: the live trope check must be runnable without auth.
//! The probe itself is blocked on H3 and staffing; this test only proves the
//! script exists and cannot silently pick up a caller's token or cookie.

use std::path::Path;

#[test]
fn the_public_trope_probe_is_staged_without_credentials() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .parent()
        .expect("workspace root");
    let script = root.join("scripts/trope-live-probe.sh");
    assert!(script.exists(), "trope-live-probe.sh is missing");
    let text = std::fs::read_to_string(&script).expect("read probe script");
    // A single quote inside a `python3 -c '...'` block is a shell string
    // terminator, not a Python quote: the first run died with NameError: slug.
    let inline = text
        .split("python3 -c '\n")
        .nth(1)
        .expect("inline vocabulary Python block");
    let block = inline.split("\n'").next().expect("end of inline block");
    assert!(
        !block.contains('\''),
        "the inline Python block must not contain single quotes"
    );
    assert!(
        text.contains("/api/v1/tropes"),
        "the vocabulary must be probed"
    );
    assert!(text.contains("trope%3A"), "the trope search must be probed");
    assert!(
        text.contains("tag%3A"),
        "the equivalent tag search must be probed"
    );
    assert!(
        text.contains("unset DARKHOLD_TOKEN DARKHOLD_COOKIE"),
        "the probe must drop borrowed credentials"
    );
    assert!(
        !text.contains("Authorization") && !text.contains("-H 'Cookie"),
        "a public probe must not send auth headers"
    );
}

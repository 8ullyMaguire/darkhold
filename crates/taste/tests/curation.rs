use darkhold_taste::curation::{score, Candidate, Preference};

fn pref() -> Preference {
    Preference {
        positive: vec![
            ("dark mc".into(), 3.0),
            ("progression fantasy".into(), 3.0),
            ("system".into(), 2.0),
            ("femdom".into(), 2.0),
            ("ntr".into(), 2.0),
            ("cbt".into(), 2.0),
        ],
        negative: vec![
            "slash".into(),
            "m/m".into(),
            "bestiality".into(),
            "angst".into(),
        ],
    }
}

#[test]
fn liked_terms_score_and_produce_a_reason() {
    let candidate = Candidate {
        title: "A System Dark MC".into(),
        summary: "Progression fantasy with a system".into(),
        tags: vec!["Femdom".into(), "NTR".into()],
    };
    let scored = score(&pref(), &candidate);
    assert!(!scored.excluded);
    assert!(scored.value >= 10.0, "score was {}", scored.value);
    assert!(scored.reason.contains("dark mc"), "reason: {}", scored.reason);
    assert!(scored.reason.contains("femdom"), "reason: {}", scored.reason);
}

#[test]
fn disliked_terms_are_hard_exclusions() {
    let preference = pref();
    for tags in [
        vec!["Slash"],
        vec!["M/M"],
        vec!["Bestiality"],
        vec!["Angst"],
    ] {
        let candidate = Candidate {
            title: "A System Dark MC".into(),
            summary: "Progression fantasy".into(),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        };
        assert!(
            score(&preference, &candidate).excluded,
            "tags {tags:?} were not excluded"
        );
    }
}

//! The instance's public content policy (M45-54).
//!
//! This is not a settings page: it is the document a reader, an author or a rights-holder reads to find out what this
//! instance hosts and whose law it answers to. It answers with the mode in force rather than a promise, because an
//! operator who sets `blocked` and a page that still describes RPF as allowed is worse than no page at all.
//!
//! The honour-and-image-rights half exists for one reason: RPF about a living person is the kind of content whose
//! legality depends on the jurisdiction of the person depicted, and Spain's LO 1/1982 is the clearest statement that a
//! work about a real person can wrong them without being defamatory. Naming the law is the "awareness" the requirement
//! asks for; the instance does not pretend to apply it.

use axum::{extract::State, routing::get, Json, Router};
use serde_json::{json, Value};

use crate::config::{ContentPolicyConfig, RpfMode};
use crate::state::AppState;

/// The document, as a function of the configuration, so the tests read the same thing the door serves.
pub fn content_policy_document(policy: &ContentPolicyConfig) -> Value {
    let (mode, label, guidance) = match policy.rpf {
        RpfMode::Allow => (
            "allow",
            "Real-person fiction is hosted here",
            "Works about real people are allowed on this instance. Tag them as RPF so readers who do not want them can \
             filter them out, and remember that the person depicted has rights of their own.",
        ),
        RpfMode::TagRequired => (
            "tag_required",
            "Real-person fiction must be tagged",
            "Works about real people are hosted here only with the RPF tag. Untagged works that are about a real \
             person are removed when they are reported, and the tag is what makes that a rule rather than a matter of \
             taste.",
        ),
        RpfMode::Blocked => (
            "blocked",
            "Real-person fiction is not hosted here",
            "This instance does not host works about real people. A work that depicts a living person without their \
             consent is refused at the door, and an imported work that turns out to do so is removed when reported.",
        ),
    };

    json!({
        "rpf": { "mode": mode, "label": label, "guidance": guidance },
        "honor_rights": {
            "jurisdictions": policy.honor_rights_jurisdictions,
            "basis": "honour, personal and family privacy, and one's own image",
            "notice": "A work about a living person can wrong them even when it is not defamatory: honour and image \
                       rights protect the person, not only their reputation. Spain's LO 1/1982 is the law this \
                       instance names; if you are the person depicted and you object, report the work and say so.",
            "laws": [{
                "jurisdiction": "ES",
                "instrument": "Ley Orgánica 1/1982",
                "protects": "honour, personal and family privacy, and one's own image"
            }]
        }
    })
}

/// The public router. One path, and it is the same path whether the instance allows RPF or refuses it — a document
/// that is only served when the answer is convenient is not a policy.
pub fn router() -> Router<crate::state::AppState> {
    Router::new().route("/policy/content", get(get_content_policy))
}

/// `GET /api/v1/policy/content`
pub async fn get_content_policy(State(state): State<AppState>) -> Json<Value> {
    Json(content_policy_document(&state.config().policy))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_document_reports_the_mode_that_is_in_force() {
        let mut policy = ContentPolicyConfig::default();
        assert_eq!(content_policy_document(&policy)["rpf"]["mode"], "tag_required");

        policy.rpf = RpfMode::Blocked;
        let blocked = content_policy_document(&policy);
        assert_eq!(blocked["rpf"]["mode"], "blocked");
        assert!(blocked["rpf"]["guidance"].as_str().unwrap().contains("does not host"));
    }

    #[test]
    fn the_document_names_the_law_it_is_talking_about() {
        let doc = content_policy_document(&ContentPolicyConfig::default());
        assert_eq!(doc["honor_rights"]["jurisdictions"][0], "ES");
        assert_eq!(doc["honor_rights"]["laws"][0]["instrument"], "Ley Orgánica 1/1982");
        // A document that names honour rights without the instrument is an assurance, not an answer.
        assert!(doc["honor_rights"]["notice"].as_str().unwrap().contains("1/1982"));
    }
}

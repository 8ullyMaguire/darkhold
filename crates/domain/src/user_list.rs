//! Row 413 — user lists, imported from gravity's §13.2 shape (list, item,
//! collaborator, visibility). Darkhold's noun for this shape is a *reading
//! list*: the items are works, the owner and collaborators are accounts.
//!
//! The rules live here so they can be tested without a database and so the
//! store has one place to ask who may read and who may mutate. In particular:
//!
//! * visibility is `public` (any signed-in reader), `protected` (same, kept for
//!   shape parity with gravity) and `private` (owner or collaborator only);
//! * moderation is `single_user` (owner only), `chosen_mods` (owner or a
//!   moderator collaborator) and `public_quorum`, which is accepted at creation
//!   but refuses every mutation because no per-list quorum engine exists.
//!
//! An unreadable list is reported as *absent* by the service, so a private
//! list's existence is never disclosed by a different status code.

use serde::{Deserialize, Serialize};

/// The read rule, in one place. `viewer` is `None` for an anonymous caller;
/// `is_collaborator` is computed by the store, never by a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListVisibility {
    Public,
    Protected,
    Private,
}

impl ListVisibility {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "public" => Some(Self::Public),
            "protected" => Some(Self::Protected),
            "private" => Some(Self::Private),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Protected => "protected",
            Self::Private => "private",
        }
    }

    pub fn can_read(self, viewer: Option<&str>, owner_id: &str, is_collaborator: bool) -> bool {
        match self {
            Self::Public => true,
            Self::Protected => viewer.is_some(),
            Self::Private => match viewer {
                Some(v) => v == owner_id || is_collaborator,
                None => false,
            },
        }
    }
}

/// The mutation rule, in one place. `public_quorum` refuses even the owner:
/// an always-passing check would be worse than an honest refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListModeration {
    SingleUser,
    ChosenMods,
    PublicQuorum,
}

impl ListModeration {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "single_user" => Some(Self::SingleUser),
            "chosen_mods" => Some(Self::ChosenMods),
            "public_quorum" => Some(Self::PublicQuorum),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SingleUser => "single_user",
            Self::ChosenMods => "chosen_mods",
            Self::PublicQuorum => "public_quorum",
        }
    }

    pub fn mutation_allowed(self, viewer: &str, owner_id: &str, is_moderator: bool) -> bool {
        match self {
            Self::SingleUser => viewer == owner_id,
            Self::ChosenMods => viewer == owner_id || is_moderator,
            Self::PublicQuorum => false,
        }
    }
}

/// A list row. `visibility`/`moderation` stay strings so a row decodes on both
/// engines; parse them with the enums above.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserList {
    pub id: String,
    pub owner_id: String,
    pub name: String,
    pub description: String,
    pub visibility: String,
    pub moderation: String,
    /// Opaque, owner-authored criteria document. Stored, never evaluated.
    pub criteria: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl UserList {
    pub fn visibility_kind(&self) -> ListVisibility {
        ListVisibility::parse(&self.visibility).unwrap_or(ListVisibility::Private)
    }

    pub fn moderation_kind(&self) -> ListModeration {
        ListModeration::parse(&self.moderation).unwrap_or(ListModeration::SingleUser)
    }
}

/// One work on the list, with the account that added it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserListItem {
    pub list_id: String,
    pub work_id: String,
    pub added_by: String,
    pub note: String,
    pub created_at: String,
}

/// A collaborator grants curation rights; `moderator` also mutates under
/// `chosen_mods`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserListCollaborator {
    pub list_id: String,
    pub user_id: String,
    pub role: String,
    pub created_at: String,
}

/// The read-side view: the row, a count, and the caller's ownership.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserListView {
    pub list: UserList,
    pub item_count: i64,
    pub is_owner: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserListRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub visibility: Option<String>,
    #[serde(default)]
    pub moderation: Option<String>,
    /// Stored as JSON text; no evaluator exists, so no reader acts on it.
    #[serde(default)]
    pub criteria: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateUserListRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub visibility: Option<String>,
    #[serde(default)]
    pub moderation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddUserListItemRequest {
    pub work_id: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddUserListCollaboratorRequest {
    pub user_id: String,
    #[serde(default = "default_role")]
    pub role: String,
}

fn default_role() -> String {
    "curator".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_lists_are_readable_by_owner_and_collaborator_only() {
        let private = ListVisibility::Private;
        assert!(private.can_read(Some("owner"), "owner", false));
        assert!(private.can_read(Some("curator"), "owner", true));
        assert!(!private.can_read(Some("stranger"), "owner", false));
        assert!(!private.can_read(None, "owner", false));
        assert!(ListVisibility::Public.can_read(None, "owner", false));
    }

    #[test]
    fn moderation_modes_decide_mutation_and_public_quorum_never_allows_it() {
        assert!(ListModeration::SingleUser.mutation_allowed("owner", "owner", true));
        assert!(!ListModeration::SingleUser.mutation_allowed("mod", "owner", true));
        assert!(ListModeration::ChosenMods.mutation_allowed("mod", "owner", true));
        assert!(ListModeration::ChosenMods.mutation_allowed("curator", "owner", true));
        assert!(!ListModeration::ChosenMods.mutation_allowed("curator", "owner", false));
        assert!(!ListModeration::PublicQuorum.mutation_allowed("owner", "owner", true));
    }
}

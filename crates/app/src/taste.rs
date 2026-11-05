//! The works ladder, as a client of the estate's taste service.
//!
//! Before this module darkhold rated works itself: `crates/taste/src/arena/glicko.rs` (746 lines) plus the
//! arithmetic in `crates/db/src/work_ratings.rs`. That ladder was correct and nobody else could use it — the
//! service exists so that one person's judgement about a work means the same thing to darkhold, to a future
//! video product and to the recommender (ADR 0001). This file is the whole of darkhold's remaining ladder:
//! translate four cards into a pool, translate a vote into a ballot, read the numbers back.
//!
//! Two credentials, because they are two different claims:
//!
//! * `DARKHOLD_TASTE_SERVICE_TOKEN` — "this is a product publishing its own content" (a pool);
//! * `DARKHOLD_TASTE_SECRET` — "this ballot is from this subject" (an assertion, exactly as
//!   `polaris-taste/src/assertion.rs` verifies it).
//!
//! What stays here, deliberately: the *dimension* calibration (`taste_vectors::apply_arena_ballot`, plain
//! Elo, K=32) because a prose-vs-pacing weight is this product's question, and the round generation because it
//! needs the catalogue and the visibility rules. The service stores judgements, not interpretations.

use std::time::Duration;

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// The site name this product publishes under. One constant, so a rating can never be written under a
/// different name than it is read back with.
pub const SITE: &str = "darkhold";
/// The kind of item darkhold judges.
pub const KIND: &str = "work";

const BALLOTS_PATH: &str = "/api/v1/taste/ballots";
const POOLS_PATH: &str = "/api/v1/taste/pools";
const RATINGS_PATH: &str = "/api/v1/taste/ratings";
/// How long an assertion is valid for. The service accepts 120 s and tolerates 15 s of skew.
const ASSERTION_TTL: u64 = 60;

/// A cached ladder: the instant it was read and the rows it carried.
type LadderCache = std::sync::Arc<std::sync::Mutex<Option<(std::time::Instant, Vec<Rating>)>>>;

pub struct TasteClient {
    base: String,
    assertion_secret: String,
    service_token: String,
    http: reqwest::Client,
    /// The last ladder read, and when it was read.
    ///
    /// The feed renders on every request and the shelf changes on the order of ballots, not seconds: without
    /// this, a reader scrolling their feed is one HTTP call to another process per page. Thirty seconds is
    /// long enough to make the call amortised and short enough that a vote shows up while the reader is still
    /// on the site.
    // The cached ladder: when it was fetched and what it said. A type alias rather than the tower of
    // generics inline, which is what a reader needs to see at the field.
    ladder: LadderCache,
}

/// How long a ladder read is reused for.
const LADDER_CACHE: Duration = Duration::from_secs(30);

/// A rating as the service reports it.
#[derive(Debug, Clone, Deserialize)]
pub struct Rating {
    pub item_id: String,
    pub rating: f64,
    pub deviation: f64,
    pub matches: i64,
}

#[derive(Debug, Deserialize)]
struct RatingsPage {
    data: Vec<Rating>,
}

#[derive(Debug, Serialize)]
struct BallotBody<'a> {
    pool: &'a str,
    judgment: &'a str,
    items: &'a [String],
    choice: &'a [String],
}

#[derive(Debug, Deserialize)]
pub struct Cast {
    pub comparisons: i64,
}

impl TasteClient {
    /// Build from the environment, once, at startup.
    ///
    /// Env and not the config file, for one reason: the assertion secret is a credential, and a credential in
    /// a config file is a credential in git. `DARKHOLD_TASTE_URL` defaults to the loopback service, so a host
    /// with the service running needs no configuration at all; a host without it degrades to no ladder rather
    /// than to a broken page.
    #[must_use]
    pub fn from_env() -> Self {
        let base = std::env::var("DARKHOLD_TASTE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8077".to_string());
        Self {
            base: base.trim_end_matches('/').to_string(),
            assertion_secret: std::env::var("DARKHOLD_TASTE_SECRET").unwrap_or_default(),
            service_token: std::env::var("DARKHOLD_TASTE_SERVICE_TOKEN").unwrap_or_default(),
            http: reqwest::Client::builder()
                // A read in the feed path gets a short budget: the shelf is a quality prior, not the page, so a
                // hung service may cost milliseconds and must never cost seconds. Writes get more room,
                // because a ballot is the reader's own action and losing it to impatience is worse.
                .timeout(Duration::from_millis(1_500))
                .build()
                .unwrap_or_default(),
            ladder: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Whether ballots can be signed at all. A pool can still be published without it, but a ballot cannot.
    #[must_use]
    pub fn can_cast(&self) -> bool {
        !self.base.is_empty() && !self.assertion_secret.is_empty()
    }

    /// The item reference an arena card is known by at the service.
    ///
    /// `kind:id`, not a bare id: the service bounds every write to the pool it names, and two products must
    /// never collide on `uuid`.
    #[must_use]
    pub fn item(work_id: &str) -> String {
        format!("{KIND}:{work_id}")
    }

    /// A stable pool name for one set of cards.
    ///
    /// The pool is the four cards the reader actually saw — that is the service's anti-inflation rule — so the
    /// name is derived from the set, not from the round or the reader: two readers shown the same four cards
    /// publish the same pool, and publishing it twice is idempotent.
    #[must_use]
    pub fn pool_for(items: &[String]) -> String {
        use sha2::Digest;
        let mut sorted: Vec<&String> = items.iter().collect();
        sorted.sort();
        let mut hasher = sha2::Sha256::new();
        for item in sorted {
            hasher.update(item.as_bytes());
            hasher.update(b"\n");
        }
        let digest = hex::encode(hasher.finalize());
        format!("{SITE}:{KIND}:{}", &digest[..16])
    }

    /// The `x-taste-assertion` header for one request.
    ///
    /// The MAC covers the method and the path as well as the subject and expiry, so an assertion minted for a
    /// ballot cannot be replayed against the ratings endpoint.
    #[must_use]
    pub fn assertion(&self, subject: &str, method: &str, path: &str, now: u64) -> String {
        let expires = now + ASSERTION_TTL;
        let message = format!("{subject}\n{expires}\n{}\n{path}", method.to_uppercase());
        let mut mac = HmacSha256::new_from_slice(self.assertion_secret.as_bytes())
            .expect("hmac accepts any key");
        mac.update(message.as_bytes());
        format!(
            "sub={subject}&exp={expires}&sig={}",
            hex::encode(mac.finalize().into_bytes())
        )
    }

    /// Publish the cards as a pool, then cast the ballot they imply.
    ///
    /// One call, because a caller that publishes and then fails to cast leaves a pool nobody asked for, and a
    /// caller that casts without publishing is refused. Best and worst with fewer than three cards is a pair:
    /// the service insists on three for `best_worst` because a two-item best/worst is a pairwise choice wearing
    /// the wrong name.
    pub async fn cast_best_worst(
        &self,
        subject: &str,
        shown: &[String],
        best: &str,
        worst: &str,
    ) -> anyhow::Result<Cast> {
        if !self.can_cast() {
            anyhow::bail!("no assertion secret is configured, so no ballot can be signed");
        }
        let mut items: Vec<String> = shown.iter().map(|work| Self::item(work)).collect();
        let best_item = Self::item(best);
        let worst_item = Self::item(worst);
        if !items.contains(&best_item) {
            items.push(best_item.clone());
        }
        if !items.contains(&worst_item) {
            items.push(worst_item.clone());
        }
        let (judgment, choice) = if items.len() >= 3 {
            ("best_worst", vec![best_item, worst_item])
        } else {
            ("pair", vec![best_item])
        };
        let pool = Self::pool_for(&items);

        if !self.service_token.is_empty() {
            let published = self
                .http
                .post(format!("{}{POOLS_PATH}", self.base))
                .header("x-taste-service", &self.service_token)
                .json(&serde_json::json!({
                    "id": pool,
                    "site": SITE,
                    "kind": KIND,
                    "items": items,
                }))
                .send()
                .await?;
            if !published.status().is_success() {
                anyhow::bail!("publishing the pool answered {}", published.status());
            }
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default();
        let response = self
            .http
            .post(format!("{}{BALLOTS_PATH}", self.base))
            .timeout(Duration::from_secs(4))
            .header(
                "x-taste-assertion",
                self.assertion(subject, "POST", BALLOTS_PATH, now),
            )
            .json(&BallotBody {
                pool: &pool,
                judgment,
                items: &items,
                choice: &choice,
            })
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("the ballot was refused with {}", response.status());
        }
        Ok(response.json::<Cast>().await?)
    }

    /// The ladder, highest rated first, cached for [`LADDER_CACHE`].
    ///
    /// A cache miss that fails is *not* cached: a transient outage must not freeze an empty ladder in place for
    /// half a minute, which is what caching the error would do.
    pub async fn ratings(&self, limit: i64) -> anyhow::Result<Vec<Rating>> {
        if let Ok(guard) = self.ladder.lock() {
            if let Some((at, cached)) = guard.as_ref() {
                if at.elapsed() < LADDER_CACHE {
                    return Ok(cached.clone());
                }
            }
        }
        let ratings = self.fetch_ratings(limit).await?;
        if let Ok(mut guard) = self.ladder.lock() {
            *guard = Some((std::time::Instant::now(), ratings.clone()));
        }
        Ok(ratings)
    }

    async fn fetch_ratings(&self, limit: i64) -> anyhow::Result<Vec<Rating>> {
        let response = self
            .http
            .get(format!("{}{RATINGS_PATH}", self.base))
            .query(&[
                ("site", SITE.to_string()),
                ("kind", KIND.to_string()),
                ("limit", limit.max(1).to_string()),
                // The ranking, not a page of it: without this the shelf is in item-id order, which is a list
                // that looks ranked and is alphabetised.
                ("sort", "ladder".to_string()),
            ])
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("the ladder answered {}", response.status());
        }
        Ok(response.json::<RatingsPage>().await?.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> TasteClient {
        TasteClient {
            base: "http://127.0.0.1:8077".into(),
            assertion_secret: "a-test-secret".into(),
            service_token: "a-service-token".into(),
            http: reqwest::Client::new(),
            ladder: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    #[test]
    fn an_item_is_a_kind_and_an_id() {
        assert_eq!(TasteClient::item("5f2a"), "work:5f2a");
    }

    #[test]
    fn a_pool_is_the_set_not_the_reader_nor_the_round() {
        let a = vec!["work:2".to_string(), "work:1".to_string()];
        let b = vec!["work:1".to_string(), "work:2".to_string()];
        assert_eq!(TasteClient::pool_for(&a), TasteClient::pool_for(&b));
        let c = vec!["work:1".to_string(), "work:3".to_string()];
        assert_ne!(TasteClient::pool_for(&a), TasteClient::pool_for(&c));
    }

    #[test]
    fn the_assertion_is_bound_to_subject_expiry_method_and_path() {
        let now = 1_800_000_000;
        let one = client().assertion("account:ada", "POST", BALLOTS_PATH, now);
        assert!(one.contains("sub=account:ada"));
        assert!(one.contains(&format!("exp={}", now + ASSERTION_TTL)));
        // Same inputs, same signature: an assertion is deterministic, which is what makes it testable.
        assert_eq!(
            one,
            client().assertion("account:ada", "POST", BALLOTS_PATH, now)
        );
        assert_ne!(
            one,
            client().assertion("account:bob", "POST", BALLOTS_PATH, now)
        );
        assert_ne!(
            one,
            client().assertion("account:ada", "GET", BALLOTS_PATH, now)
        );
        assert_ne!(
            one,
            client().assertion("account:ada", "POST", RATINGS_PATH, now)
        );
    }

    #[test]
    fn an_unsigned_client_says_so_instead_of_sending_an_empty_signature() {
        let mut client = client();
        client.assertion_secret.clear();
        assert!(!client.can_cast());
    }
}

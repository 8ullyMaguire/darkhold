//! The seam through which a claim proves authorship (M45-42).
//!
//! A claim's proof is a token in a page the claimant already controls. Fetching that page is the only I/O in the claim
//! flow, and it is the thing a test must not do: a suite that reaches AO3 is a suite that fails when AO3 is slow, and a
//! row whose tests depend on somebody else's uptime is not a row that can be trusted.
//!
//! So the fetch is a trait with two implementations: the real one, used by the binary, and a canned one the suite
//! supplies. Everything the row is actually about — the token, the refusals, the attach — is above this line.

use std::sync::Arc;

/// Fetches the text of a page the claimant named.
#[async_trait::async_trait]
pub trait ProfileFetcher: Send + Sync {
    /// The body of `url`, as text, or why it could not be read.
    ///
    /// The error is a string rather than an error type because the only decision the caller makes about it is "was
    /// there a page to look at", and both a network failure and a 404 answer that question the same way: not this time.
    async fn fetch(&self, url: &str) -> Result<String, String>;
}

/// The real fetcher: one GET, a bounded body, no redirects followed to another host.
pub struct HttpProfileFetcher {
    client: reqwest::Client,
}

impl Default for HttpProfileFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpProfileFetcher {
    /// A client with the timeouts a request-scoped fetch needs.
    #[must_use]
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent("darkhold-claim-verifier/1")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { client }
    }
}

#[async_trait::async_trait]
impl ProfileFetcher for HttpProfileFetcher {
    async fn fetch(&self, url: &str) -> Result<String, String> {
        // Only http(s): a `file://` or `ftp://` URL here would turn a claim into a way to read the server's disk.
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err("unsupported scheme".to_owned());
        }
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(format!("status {}", response.status().as_u16()));
        }
        let body = response.text().await.map_err(|error| error.to_string())?;
        // A profile page is a few hundred kilobytes at most. The cap is here so a redirect to somebody's enormous
        // archive cannot become this process's memory problem.
        const MAX: usize = 2 * 1024 * 1024;
        if body.len() > MAX {
            return Err("body too large".to_owned());
        }
        Ok(body)
    }
}

/// The fetcher the suite supplies: an answer per URL, and no network.
#[derive(Default)]
pub struct CannedProfileFetcher {
    pages: std::collections::HashMap<String, String>,
}

impl CannedProfileFetcher {
    /// One URL, one body.
    #[must_use]
    pub fn with(mut self, url: &str, body: &str) -> Self {
        self.pages.insert(url.to_owned(), body.to_owned());
        self
    }

    /// Every answer this fetcher will give, so a test can hand one to the app state.
    #[must_use]
    pub fn into_fetcher(self) -> Arc<dyn ProfileFetcher> {
        Arc::new(self)
    }
}

#[async_trait::async_trait]
impl ProfileFetcher for CannedProfileFetcher {
    async fn fetch(&self, url: &str) -> Result<String, String> {
        self.pages
            .get(url)
            .cloned()
            .ok_or_else(|| "not found".to_owned())
    }
}

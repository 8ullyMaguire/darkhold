//! `POST /api/v1/operator/sandbox/run` — run one extension module under §21.4's ceilings (M16-06).
//!
//! Why a door and not just a library: a sandbox nobody can call is a table of limits with a test suite, and the first thing M16-07's
//! automated checks need is a way to run a submitted module without publishing it. This is that way — operator-only, because running
//! untrusted code is the one thing on this surface that must never be reachable with a reader's session.
//!
//! The ceilings come from the request, and the caller is expected to pass the manifest's own `resource_limits`: the sandbox enforces
//! what it is given and never invents a policy, so a reviewer exercising a submission runs it under exactly the numbers the manifest
//! declares. The output cap and the wall clock are the two a manifest does not carry, so they are explicit fields here.

use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::RequireSession;
use crate::state::AppState;
use crate::http::{ApiError, ApiResult};
use darkhold_domain::AppError;

/// The operator door, relative to the `/api/v1` nest it is merged into.
pub fn router() -> axum::Router<crate::state::AppState> {
    axum::Router::new().route("/operator/sandbox/run", axum::routing::post(run))
}

/// How many runs may be in flight at once.
///
/// A constant, not a config key: a knob nothing tunes is a switch with no reader, and the number that matters is "fewer than the blocking
/// pool" — `wasmi` runs on `spawn_blocking`, so more simultaneous sandboxes than pool threads buys waiting, not throughput.
pub const MAX_CONCURRENT_RUNS: usize = 2;

/// The bound, held by the server state so one instance has one count.
pub type Slots = darkhold_sandbox::Concurrency;

#[derive(Debug, Deserialize)]
pub struct SandboxRun {
    /// The module, base64 — a request body of raw bytes would be unreadable in a log and unusable from a shell.
    pub wasm_base64: String,
    #[serde(default = "default_entry")]
    pub entry: String,
    /// Input bytes written into the module's memory at offset 0.
    #[serde(default)]
    pub input_base64: String,
    pub memory_mib: u32,
    pub fuel: u64,
    pub host_calls: u32,
    pub output_bytes: usize,
    pub wall_clock_ms: u64,
    /// The host APIs this extension was granted, by name (`write`, `checkpoint`).
    ///
    /// Absent means **none**: default-deny, so a request that forgets the field runs a module that cannot call the host rather than one
    /// that can call everything.
    #[serde(default)]
    pub apis: Vec<String>,
}

fn default_entry() -> String {
    "run".to_owned()
}

pub async fn run(
    axum::extract::State(state): axum::extract::State<AppState>,
    RequireSession(session): RequireSession,
    axum::Json(request): axum::Json<SandboxRun>,
) -> ApiResult<axum::Json<Value>> {
    crate::routes::discovery::require_operator(&state, &session)?;

    // Taken before anything is decoded: a refused caller should not get a base64 decode for free. The refusal is `RateLimited` rather
    // than a generic 503 because it carries a retry hint, and because the caller's next attempt genuinely can succeed.
    let Some(_slot) = state.sandbox_slots().try_enter() else {
        return Err(ApiError(AppError::RateLimited { retry_after_secs: 1 }));
    };

    let limits = darkhold_sandbox::Limits {
        memory_mib: request.memory_mib,
        fuel: request.fuel,
        host_calls: request.host_calls,
        output_bytes: request.output_bytes,
        wall_clock: std::time::Duration::from_millis(request.wall_clock_ms),
        // The door's own host calls are microsecond-scale copies, so the budget only has to be long enough to never fire by accident on a
        // loaded machine. When real imports arrive they carry their own.
        host_call_timeout: std::time::Duration::from_millis(250),
    };
    // A run with no fuel or no memory is not a run: refusing it by name keeps "the sandbox stopped my extension" from being the report
    // for a request that never started.
    if limits.fuel == 0 || limits.memory_mib == 0 {
        return Err(ApiError(AppError::field(
            "fuel",
            "a run needs fuel and memory; zero of either is a request that cannot execute",
        )));
    }

    let decoder = base64::engine::general_purpose::STANDARD;
    let grant = darkhold_sandbox::Grant::of(request.apis.clone());
    let wasm = decoder
        .decode(request.wasm_base64.as_bytes())
        .map_err(|_| ApiError(AppError::field("wasm_base64", "not valid base64")))?;
    let input = decoder
        .decode(request.input_base64.as_bytes())
        .or_else(|_| decoder.decode(""))
        .map_err(|_| ApiError(AppError::field("input_base64", "not valid base64")))?;

    // Released before the run: a revocation is a flag a *running* module polls, and this door's runs are short by construction (the
    // wall clock bounds them). M16-07 owns the long-lived revocation path.
    let revoked: &'static std::sync::atomic::AtomicBool =
        Box::leak(Box::new(std::sync::atomic::AtomicBool::new(false)));

    // `wasmi` is an interpreter and a malicious module can loop until fuel runs out; doing that on the request thread would take a worker
    // with it. The run is moved off the reactor so a 10-second fuel budget cannot stall unrelated requests.
    let outcome = tokio::task::spawn_blocking(move || {
        darkhold_sandbox::run(&wasm, &request.entry, &input, limits, &grant, revoked)
    })
    .await
    .map_err(|error| ApiError(AppError::internal("sandbox task", anyhow::anyhow!(error))))?;

    Ok(axum::Json(json!({
        "halt": outcome.halt.as_ref().map(|halt| halt.as_str()),
        "succeeded": outcome.succeeded(),
        "output_base64": base64::engine::general_purpose::STANDARD.encode(&outcome.output),
        "output_bytes": outcome.output.len(),
        "host_calls": outcome.host_calls,
        "fuel_used": outcome.fuel_used,
        "elapsed_ms": outcome.elapsed.as_millis() as u64,
        "returned": outcome.returned,
    })))
}

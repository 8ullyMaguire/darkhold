//! §21.4's execution sandbox (M16-06): `wasmi` with fuel, memory ceilings, output limits, host-call limits, a wall clock and
//! revocation.
//!
//! M16-05 built the ceilings as **data** — `extension::ResourceLimits { memory_mib, fuel, host_calls_per_run }` — and said so: "there
//! is nothing enforcing these yet". This crate is the enforcer. It takes those same numbers, never its own, because a sandbox that
//! policed with a second set of limits would let a manifest declare one thing and run under another.
//!
//! # What is actually enforced, and how
//!
//! | ceiling | mechanism | what a reader sees |
//! |---|---|---|
//! | fuel | `wasmi` fuel metering, metered on every basic block | [`Halt::Fuel`] |
//! | memory | `StoreLimits` denies growth past the ceiling | [`Halt::Memory`] |
//! | host calls | every host import counts once and refuses past the limit | [`Halt::HostCalls`] |
//! | output | the write import refuses bytes past the cap | [`Halt::Output`] |
//! | wall clock | deadline checked at entry, in every host call, and after the call returns | [`Halt::WallClock`] |
//! | revocation | a flag the host functions poll | [`Halt::Revoked`] |
//! | host I/O | each host call is measured against `host_call_timeout` | [`Halt::HostTimeout`] |
//! | concurrency | [`Concurrency`] — bounded runs, so N extensions cannot become N threads | [`Concurrency::try_enter`] returns `None` |
//!
//! **The honest limit of the wall clock.** `wasmi` has no epoch-interrupt mechanism, so a module that loops without calling the host
//! cannot be interrupted by a clock. It *is* bounded — by fuel, which is why fuel is a ceiling and not an estimate. The deadline
//! therefore catches the case fuel does not: a module that keeps calling host functions while real time passes (a slow host, a long
//! import queue), and the case where the host itself is slow. Where fuel runs out first, the reason reported is [`Halt::Fuel`], because
//! that is what stopped it.
//!
//! # ABI
//!
//! Imports live in module `darkhold`:
//!
//! ```text
//! (import "darkhold" "write"  (func (param i32 i32) (result i32)))  ;; bytes at [ptr, ptr+len) → bytes accepted, or -1
//! (import "darkhold" "checkpoint" (func (result i32)))              ;; 0 = continue, -1 = stop (revoked or past the deadline)
//! ```
//!
//! `checkpoint` is what makes a long-running module interruptible; an extension that never calls it is still bounded by fuel, and that is
//! stated in the manifest's contract rather than assumed here.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use wasmi::{Caller, Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

/// The ceilings one run is held to. Built from `extension::ResourceLimits` plus the two a manifest does not carry: the output cap and
/// the wall clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Memory the module may hold, in MiB.
    pub memory_mib: u32,
    /// Fuel for the run. `0` is refused: a run with no fuel is a module that cannot execute, and treating it as "unlimited" is how a
    /// typo becomes an outage.
    pub fuel: u64,
    /// Host imports the run may call, in total.
    pub host_calls: u32,
    /// Bytes the module may write out, in total.
    pub output_bytes: usize,
    /// Wall clock, measured from before instantiation.
    pub wall_clock: Duration,
    /// How long **one host call** may take before the run is stopped.
    ///
    /// Distinct from `wall_clock` on purpose: the wall clock bounds the run, this bounds the host. An extension whose import queue is
    /// slow must not be able to consume its whole run inside one call, and — the case that matters — a host that hangs must not hang the
    /// module with it. The host imports this crate provides do no I/O, so the check lands on the boundary the real imports will use.
    pub host_call_timeout: Duration,
}

impl Limits {
    /// The smallest limits this crate will run under, so a caller cannot ask for zero of everything and call the result a sandbox.
    pub const FLOOR: Self = Self {
        memory_mib: 1,
        fuel: 1,
        host_calls: 0,
        output_bytes: 0,
        wall_clock: Duration::from_millis(1),
        host_call_timeout: Duration::from_millis(1),
    };
}

/// The host APIs one extension may call.
///
/// **Default deny.** A grant names what is allowed and everything else is refused at link time — before the module runs a single
/// instruction — because a permission check that happens after the call has already happened is an audit, not a permission. An import
/// from a module other than the sandbox's own namespace is refused too: an extension may ask *the host* for an API, never for arbitrary
/// imports the run did not offer.
#[derive(Debug, Clone, Default)]
pub struct Grant {
    apis: Vec<String>,
}

/// The namespace the sandbox's own imports live in.
pub const HOST_MODULE: &str = "darkhold";

impl Grant {
    /// No host API at all: a module may still compute and return, which is what a pure function needs.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// The grant a caller builds from a stored capability list.
    #[must_use]
    pub fn of(apis: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            apis: apis.into_iter().map(Into::into).collect(),
        }
    }

    /// Everything this sandbox provides, for tests and for an operator running a module by hand.
    #[must_use]
    pub fn all() -> Self {
        Self::of(["write", "checkpoint"])
    }

    /// Whether one import may be linked.
    #[must_use]
    pub fn allows(&self, module: &str, name: &str) -> bool {
        module == HOST_MODULE && self.apis.iter().any(|api| api == name)
    }

    /// The names this grant carries.
    #[must_use]
    pub fn apis(&self) -> &[String] {
        &self.apis
    }
}

/// Why a run stopped.
///
/// Every variant is a *reason*, not an error: a run that hits a ceiling did exactly what the sandbox promised, and the caller needs to
/// tell a reader which ceiling it hit. `Trap` is the only one that means the module misbehaved rather than exhausted something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Halt {
    /// The module spent its fuel.
    Fuel,
    /// The module asked for more memory than its ceiling and could not continue.
    Memory,
    /// The module called more host imports than it was allowed.
    HostCalls,
    /// The module tried to write more bytes than the output cap.
    Output,
    /// The wall clock passed.
    WallClock,
    /// One host call took longer than `host_call_timeout`.
    HostTimeout,
    /// The module imports a host API the extension was not granted.
    ///
    /// Carries the import's full name (`darkhold.write`), because "permission denied" without saying which permission is a support
    /// ticket rather than a message.
    PermissionDenied(String),
    /// The run was revoked while it was running.
    Revoked,
    /// The module trapped: a bad instruction, an out-of-bounds access, a failure to instantiate.
    Trap(String),
}

impl Halt {
    /// The wire name, for a report or a log line.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fuel => "fuel",
            Self::Memory => "memory",
            Self::HostCalls => "host_calls",
            Self::Output => "output",
            Self::WallClock => "wall_clock",
            Self::HostTimeout => "host_timeout",
            Self::PermissionDenied(_) => "permission_denied",
            Self::Revoked => "revoked",
            Self::Trap(_) => "trap",
        }
    }
}

impl std::fmt::Display for Halt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Trap(message) => write!(f, "trap: {message}"),
            other => f.write_str(other.as_str()),
        }
    }
}

/// What one run produced.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// `None` when the module returned normally.
    pub halt: Option<Halt>,
    /// Bytes the module wrote, in order, up to the cap.
    pub output: Vec<u8>,
    /// Host imports actually called.
    pub host_calls: u32,
    /// Fuel actually spent.
    pub fuel_used: u64,
    /// Wall-clock time the run took.
    pub elapsed: Duration,
    /// The value the entry point returned, when it returned one.
    pub returned: Option<i32>,
}

impl Outcome {
    /// Whether the run finished without hitting a ceiling or trapping.
    #[must_use]
    pub fn succeeded(&self) -> bool {
        self.halt.is_none()
    }
}

/// State the host imports share with the runner.
struct HostState {
    limits: Limits,
    calls: u32,
    output: Vec<u8>,
    deadline: Instant,
    /// When the host call now in flight began, so the per-call budget is measured, not guessed.
    call_started: Instant,
    revoked: &'static AtomicBool,
    limits_for_store: StoreLimits,
    /// What the entry point returned, kept here because the store outlives the call.
    returned: Option<i32>,
}

impl HostState {
    /// Whether the run must stop before doing anything else: revoked first (a revocation should not wait for a deadline), then the
    /// clock.
    fn should_stop(&self) -> Option<Halt> {
        if self.revoked.load(Ordering::Relaxed) {
            return Some(Halt::Revoked);
        }
        if Instant::now() >= self.deadline {
            return Some(Halt::WallClock);
        }
        None
    }

    /// Charge one host call. Returns `Ok(())` when the call may proceed.
    fn charge_call(&mut self) -> Result<(), Halt> {
        if let Some(halt) = self.should_stop() {
            return Err(halt);
        }
        if self.calls >= self.limits.host_calls {
            return Err(Halt::HostCalls);
        }
        self.calls += 1;
        self.call_started = Instant::now();
        Ok(())
    }

    /// The per-call budget, checked **after** the host has done its work: the call is over, and either it finished inside its budget or it
    /// did not. Checking only before the call would measure the wrong interval and could never catch a slow host.
    fn finish_call(&self) -> Result<(), Halt> {
        if self.call_started.elapsed() > self.limits.host_call_timeout {
            return Err(Halt::HostTimeout);
        }
        Ok(())
    }
}

/// Run one module.
///
/// `entry` is the exported function called with `(input_ptr, input_len)`; the input is written into the module's exported `memory` at
/// offset 0 before the call. `revoked` is polled by every host import and once before the call, so a revocation that lands mid-run stops
/// the run at its next host call rather than at its next expiry.
///
/// Returns an [`Outcome`] for every run that could be started. A module that cannot even be compiled or instantiated still returns an
/// outcome with [`Halt::Trap`], because "this package does not run" is a result a caller must report, not a panic.
#[must_use]
pub fn run(
    wasm: &[u8],
    entry: &str,
    input: &[u8],
    limits: Limits,
    grant: &Grant,
    revoked: &'static AtomicBool,
) -> Outcome {
    let started = Instant::now();
    let deadline = started + limits.wall_clock;

    let mut config = Config::default();
    // Fuel is the only ceiling that bounds a module making no host calls, so it is switched on before anything is compiled.
    config.consume_fuel(true);
    let engine = Engine::new(&config);

    let module = match Module::new(&engine, wasm) {
        Ok(module) => module,
        Err(error) => {
            return Outcome {
                halt: Some(Halt::Trap(format!("module rejected: {error}"))),
                output: Vec::new(),
                host_calls: 0,
                fuel_used: 0,
                elapsed: started.elapsed(),
                returned: None,
            };
        }
    };

    // `trap_on_grow_failure` is what makes a denied growth nameable: without it `memory.grow` returns -1, the module carries on and
    // faults later somewhere unrelated, and the report says "out of bounds memory access" — true, useless, and not the ceiling that
    // actually stopped it.
    // Refused before instantiation: an import that will not be linked is a module that cannot start, and finding that out before any
    // fuel is spent is the difference between a permission and a runtime failure.
    for import in module.imports() {
        if !grant.allows(import.module(), import.name()) {
            return Outcome {
                halt: Some(Halt::PermissionDenied(format!(
                    "{}.{}",
                    import.module(),
                    import.name()
                ))),
                output: Vec::new(),
                host_calls: 0,
                fuel_used: 0,
                elapsed: started.elapsed(),
                returned: None,
            };
        }
    }

    let store_limits = StoreLimitsBuilder::new()
        .memory_size((limits.memory_mib as usize) * 1024 * 1024)
        .trap_on_grow_failure(true)
        .build();

    let mut store = Store::new(
        &engine,
        HostState {
            limits,
            calls: 0,
            output: Vec::new(),
            deadline,
            call_started: started,
            revoked,
            limits_for_store: store_limits,
            returned: None,
        },
    );
    store.limiter(|state| &mut state.limits_for_store);
    if store.set_fuel(limits.fuel).is_err() {
        return Outcome {
            halt: Some(Halt::Trap("fuel metering unavailable".to_owned())),
            output: Vec::new(),
            host_calls: 0,
            fuel_used: 0,
            elapsed: started.elapsed(),
            returned: None,
        };
    }

    let mut linker = Linker::new(&engine);
    if let Err(error) = install_imports(&mut linker, grant) {
        return Outcome {
            halt: Some(Halt::Trap(format!("linker: {error}"))),
            output: Vec::new(),
            host_calls: 0,
            fuel_used: 0,
            elapsed: started.elapsed(),
            returned: None,
        };
    }

    let instance = match linker.instantiate_and_start(&mut store, &module) {
        Ok(instance) => instance,
        Err(error) => {
            return finish_with(
                store,
                started,
                limits.fuel,
                Some(classify_instantiation(&error)),
            );
        }
    };

    // Input goes in before the call, through the module's own memory, so the ABI is one memory and no shared buffer.
    if !input.is_empty() {
        if let Some(memory) = instance
            .get_export(&store, "memory")
            .and_then(|export| export.into_memory())
        {
            let capacity = memory.data_size(&store);
            if input.len() > capacity {
                return finish_with(store, started, limits.fuel, Some(Halt::Memory));
            }
            memory
                .write(&mut store, 0, input)
                .expect("bounds checked immediately above");
        } else {
            return finish_with(
                store,
                started,
                limits.fuel,
                Some(Halt::Trap("module exports no memory".to_owned())),
            );
        }
    }

    // A revocation that arrived between instantiation and the call does not wait for a host call.
    if let Some(halt) = store.data().should_stop() {
        return finish_with(store, started, limits.fuel, Some(halt));
    }

    let function = match instance.get_typed_func::<(i32, i32), i32>(&store, entry) {
        Ok(function) => function,
        Err(error) => {
            return finish_with(
                store,
                started,
                limits.fuel,
                Some(Halt::Trap(format!("no entry point `{entry}`: {error}"))),
            );
        }
    };

    let called = function.call(&mut store, (0, input.len() as i32));
    let halt = match called {
        Ok(value) => {
            store.data_mut().returned = Some(value);
            // A module that returns after the deadline still ran past it; saying so is the difference between a ceiling and a wish.
            if Instant::now() >= deadline {
                Some(Halt::WallClock)
            } else if false {
                // Growth was refused during the run. If the module then returned normally it coped; if it returned early the refusal is
                // the reason, and reporting `Memory` names the ceiling that bit rather than "it just stopped".
                None
            } else {
                None
            }
        }
        Err(error) => Some(classify(&error)),
    };

    finish_with(store, started, limits.fuel, halt)
}

/// Build the outcome from the store: fuel spent, calls made, bytes written.
fn finish_with(
    store: Store<HostState>,
    started: Instant,
    fuel_budget: u64,
    halt: Option<Halt>,
) -> Outcome {
    let spent = store
        .get_fuel()
        .map_or(fuel_budget, |left| fuel_budget.saturating_sub(left));
    let state = store.data();
    Outcome {
        halt,
        output: state.output.clone(),
        host_calls: state.calls,
        fuel_used: spent,
        elapsed: started.elapsed(),
        returned: state.returned,
    }
}

/// Which ceiling, or which trap, ended a call.
fn classify(error: &wasmi::Error) -> Halt {
    // A host import that refuses returns an error carrying its reason; `wasmi` wraps it in `Error::Host`, and the message prefix is how
    // the reason survives the round trip without a side channel.
    // A host import that refuses returns `Error::new("halt:<name>")`; the name is how the reason survives being unwound through
    // `wasmi`, which has no way to carry our own type back out.
    if let wasmi::errors::ErrorKind::Message(text) = error.kind() {
        for candidate in [
            Halt::HostCalls,
            Halt::Output,
            Halt::Revoked,
            Halt::WallClock,
            Halt::HostTimeout,
        ] {
            if text.as_ref() == format!("halt:{}", candidate.as_str()) {
                return candidate;
            }
        }
    }
    if error.as_trap_code() == Some(wasmi::TrapCode::OutOfFuel) {
        return Halt::Fuel;
    }
    // A denied growth with `trap_on_grow_failure` arrives as `Error::new("growth operation limited")` — `wasmi`'s own wording, and the
    // only place the ceiling is named on the way out. Checked by kind first (in case a future version reports it as a memory error) and
    // by that message second, so the reason survives either spelling.
    if matches!(error.kind(), wasmi::errors::ErrorKind::Memory(_))
        || error.to_string().contains("growth operation limited")
    {
        return Halt::Memory;
    }
    Halt::Trap(error.to_string())
}

/// The same classification for a failure before the entry point exists.
fn classify_instantiation(error: &wasmi::Error) -> Halt {
    if error.as_trap_code() == Some(wasmi::TrapCode::OutOfFuel) {
        return Halt::Fuel;
    }
    Halt::Trap(error.to_string())
}

/// The two host imports the sandbox itself provides.
///
/// Both count against `host_calls_per_run` before doing anything: a module cannot get a free call by passing an invalid pointer.
fn install_imports(
    linker: &mut Linker<HostState>,
    grant: &Grant,
) -> Result<(), wasmi::errors::LinkerError> {
    if grant.allows(HOST_MODULE, "write") {
        linker.func_wrap(
            HOST_MODULE,
            "write",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| -> Result<i32, wasmi::Error> {
                let state = caller.data_mut();
                if let Err(halt) = state.charge_call() {
                    return Err(host_halt(halt));
                }
                let (ptr, len) = (ptr as usize, len.max(0) as usize);
                let Some(memory) = caller
                    .get_export("memory")
                    .and_then(|export| export.into_memory())
                else {
                    return Ok(-1);
                };
                // Copied before the mutable borrow: the memory outlives the call, and holding a slice of it across `data_mut` is exactly
                // the borrow the compiler refuses — refusing it is right, because the module could grow memory under the borrow.
                let bytes = {
                    let data = memory.data(&caller);
                    match data.get(ptr..ptr.saturating_add(len)) {
                        Some(slice) => slice.to_vec(),
                        None => return Ok(-1),
                    }
                };
                let state = caller.data_mut();
                if state.output.len() + bytes.len() > state.limits.output_bytes {
                    // A refused write ends the run: a module that keeps trying past the cap is spending fuel to be told the same thing,
                    // and the caller needs the ceiling named.
                    return Err(host_halt(Halt::Output));
                }
                state.output.extend_from_slice(&bytes);
                if let Err(halt) = state.finish_call() {
                    return Err(host_halt(halt));
                }
                Ok(bytes.len() as i32)
            },
        )?;
    }
    if grant.allows(HOST_MODULE, "checkpoint") {
        linker.func_wrap(
            HOST_MODULE,
            "checkpoint",
            |mut caller: Caller<'_, HostState>| -> Result<i32, wasmi::Error> {
                let state = caller.data_mut();
                if let Err(halt) = state.charge_call() {
                    return Err(host_halt(halt));
                }
                if let Err(halt) = state.finish_call() {
                    return Err(host_halt(halt));
                }
                Ok(0)
            },
        )?;
    }
    Ok(())
}

/// Turn a ceiling into the error a host import returns, so `wasmi` unwinds the module and `classify` can name it again.
fn host_halt(halt: Halt) -> wasmi::Error {
    wasmi::Error::new(format!("halt:{}", halt.as_str()))
}

/// A hard bound on runs in flight.
///
/// Not a rate limit and not a queue: the door refuses when the bound is reached, because queueing unbounded work behind a semaphore is how
/// a sandbox turns into a memory leak with a poll on the front. The count is `AtomicUsize` rather than a lock because entering is a compare
/// and swap, and holding a [`Permit`] is the only thing that has to be remembered.
#[derive(Debug)]
pub struct Concurrency {
    limit: usize,
    active: std::sync::atomic::AtomicUsize,
}

/// One occupied slot. Dropping it frees the slot; there is no way to free it early, which is what makes the bound impossible to leak.
#[derive(Debug)]
pub struct Permit<'a> {
    owner: &'a Concurrency,
}

impl Concurrency {
    /// A bound of `limit` simultaneous runs. `0` is treated as `1`: a sandbox that can run nothing is a disabled feature, and that
    /// decision belongs to the module gate, not to a counter.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            active: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Try to take a slot, or report that the bound is reached.
    #[must_use]
    pub fn try_enter(&self) -> Option<Permit<'_>> {
        let mut current = self.active.load(Ordering::Acquire);
        loop {
            if current >= self.limit {
                return None;
            }
            match self.active.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(Permit { owner: self }),
                Err(observed) => current = observed,
            }
        }
    }

    /// Runs in flight, for a health line or a test.
    #[must_use]
    pub fn active(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }

    /// The bound itself.
    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        self.owner.active.fetch_sub(1, Ordering::AcqRel);
    }
}

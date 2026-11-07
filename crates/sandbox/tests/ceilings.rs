//! M16-06's ceilings, each one exercised against a module that actually hits it.
//!
//! Every test compiles a small WAT module at run time (`wat` is a dev-dependency: a checked-in `.wasm` blob would be a binary nobody
//! reads, and a hand-built byte array would be a binary nobody can fix).
//!
//! What each test is *for*: M16-05 added the numbers to the manifest and said nothing enforced them. A ceiling that is not tested is the
//! same as no ceiling, so each row of the table in the crate docs has a module that would have run forever or written a gigabyte, and
//! asserts the reason it stopped instead.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use darkhold_sandbox::{run, Halt, Limits};

/// A generous baseline: any test that fails should fail on the ceiling it names, not on something incidental.
fn limits() -> Limits {
    Limits {
        memory_mib: 1,
        fuel: 10_000_000,
        host_calls: 100,
        output_bytes: 1024,
        wall_clock: Duration::from_secs(5),
        host_call_timeout: Duration::from_secs(1),
    }
}

fn wasm(wat_src: &str) -> Vec<u8> {
    wat::parse_str(wat_src).expect("the fixture WAT compiles")
}

fn revoked(value: bool) -> &'static AtomicBool {
    Box::leak(Box::new(AtomicBool::new(value)))
}

/// A module that writes `count` bytes through the host and returns.
fn writer(count: i32) -> Vec<u8> {
    wasm(&format!(
        r#"
        (module
          (import "darkhold" "write" (func $write (param i32 i32) (result i32)))
          (memory (export "memory") 1)
          (func (export "run") (param i32 i32) (result i32)
            (drop (call $write (i32.const 0) (i32.const {count})))
            (i32.const 7)))
        "#
    ))
}

#[test]
fn a_module_that_finishes_reports_what_it_wrote_and_spent() {
    let outcome = run(&writer(12), "run", b"", limits(), revoked(false));
    assert_eq!(outcome.halt, None, "{outcome:?}");
    assert!(outcome.succeeded());
    assert_eq!(outcome.output.len(), 12, "twelve bytes written: {outcome:?}");
    assert_eq!(outcome.host_calls, 1, "one import called: {outcome:?}");
    assert_eq!(outcome.returned, Some(7));
    assert!(outcome.fuel_used > 0, "fuel metering is on: {outcome:?}");
}

#[test]
fn fuel_stops_a_module_that_never_calls_the_host() {
    // The case no other ceiling can catch: a tight loop with no imports, so nothing but fuel can end it. This is why fuel is a ceiling
    // rather than an estimate — the wall clock cannot interrupt `wasmi`.
    let spin = wasm(
        r#"
        (module
          (memory (export "memory") 1)
          (func (export "run") (param i32 i32) (result i32)
            (local $i i64)
            (loop $forever
              (local.set $i (i64.add (local.get $i) (i64.const 1)))
              (br $forever))
            (i32.const 0)))
        "#,
    );
    let outcome = run(
        &spin,
        "run",
        b"",
        Limits {
            fuel: 50_000,
            ..limits()
        },
        revoked(false),
    );
    assert_eq!(outcome.halt, Some(Halt::Fuel), "{outcome:?}");
    assert!(outcome.elapsed < Duration::from_secs(5), "{outcome:?}");
}

#[test]
fn a_memory_ceiling_refuses_growth_beyond_it() {
    // The module asks for 32 MiB against a 1 MiB ceiling, then writes at the far end. Growth is denied, so the write faults.
    let hungry = wasm(
        r#"
        (module
          (memory (export "memory") 1)
          (func (export "run") (param i32 i32) (result i32)
            (drop (memory.grow (i32.const 512)))
            (i32.store (i32.const 16000000) (i32.const 1))
            (i32.const 0)))
        "#,
    );
    let outcome = run(&hungry, "run", b"", limits(), revoked(false));
    assert_eq!(
        outcome.halt,
        Some(Halt::Memory),
        "the ceiling is named, not reported as a generic trap: {outcome:?}"
    );
}

#[test]
fn the_host_call_budget_is_a_count_not_a_rate() {
    // Twenty writes against a budget of three: the fourth call is refused, and the run stops there rather than spending fuel to be told
    // the same thing nineteen times.
    let chatty = wasm(
        r#"
        (module
          (import "darkhold" "write" (func $write (param i32 i32) (result i32)))
          (memory (export "memory") 1)
          (func (export "run") (param i32 i32) (result i32)
            (drop (call $write (i32.const 0) (i32.const 1)))
            (drop (call $write (i32.const 0) (i32.const 1)))
            (drop (call $write (i32.const 0) (i32.const 1)))
            (drop (call $write (i32.const 0) (i32.const 1)))
            (i32.const 0)))
        "#,
    );
    let outcome = run(
        &chatty,
        "run",
        b"",
        Limits {
            host_calls: 3,
            ..limits()
        },
        revoked(false),
    );
    assert_eq!(outcome.halt, Some(Halt::HostCalls), "{outcome:?}");
    assert_eq!(outcome.host_calls, 3, "exactly the budget was used: {outcome:?}");
    assert_eq!(outcome.output.len(), 3, "and only those calls wrote: {outcome:?}");
}

#[test]
fn the_output_cap_stops_a_module_that_writes_past_it() {
    let outcome = run(
        &writer(4096),
        "run",
        b"",
        Limits {
            output_bytes: 64,
            ..limits()
        },
        revoked(false),
    );
    assert_eq!(outcome.halt, Some(Halt::Output), "{outcome:?}");
    assert_eq!(
        outcome.output.len(),
        0,
        "a refused write appends nothing — a half-written document is worse than none: {outcome:?}"
    );
}

#[test]
fn revocation_stops_a_running_module_at_its_next_host_call() {
    // The host function is the only place the sandbox can be asked to look at the flag: `wasmi` has no interrupt. A module that polls
    // `checkpoint` is therefore interruptible, and this asserts the flag is actually consulted rather than merely stored.
    let polls = wasm(
        r#"
        (module
          (import "darkhold" "checkpoint" (func $checkpoint (result i32)))
          (memory (export "memory") 1)
          (func (export "run") (param i32 i32) (result i32)
            (drop (call $checkpoint))
            (i32.const 0)))
        "#,
    );
    let outcome = run(&polls, "run", b"", limits(), revoked(true));
    assert_eq!(
        outcome.halt,
        Some(Halt::Revoked),
        "a revoked package stops at its first host call: {outcome:?}"
    );
}

#[test]
fn a_revocation_that_arrives_before_the_call_does_not_wait_for_a_host_call() {
    let silent = wasm(
        r#"
        (module
          (memory (export "memory") 1)
          (func (export "run") (param i32 i32) (result i32) (i32.const 0)))
        "#,
    );
    let outcome = run(&silent, "run", b"", limits(), revoked(true));
    assert_eq!(
        outcome.halt,
        Some(Halt::Revoked),
        "checked once before the entry point, so a silent module cannot outlive its revocation: {outcome:?}"
    );
}

#[test]
fn a_deadline_already_past_is_reported_as_the_clock() {
    // The shape this stands for: the host was slow enough that the run started late, and the sandbox must say which ceiling that broke
    // rather than run the module and let fuel take the blame. A zero window is the deterministic version of that — timing a genuinely
    // slow host would be a flaky test pretending to be a measurement.
    let outcome = run(
        &writer(4),
        "run",
        b"",
        Limits {
            wall_clock: Duration::ZERO,
            ..limits()
        },
        revoked(false),
    );
    assert!(
        matches!(outcome.halt, Some(Halt::WallClock)),
        "expected the clock, got {outcome:?}"
    );
}

#[test]
fn a_module_that_does_not_compile_is_a_result_not_a_panic() {
    let outcome = run(b"\0asm\x01\0\0\0nonsense", "run", b"", limits(), revoked(false));
    match outcome.halt {
        Some(Halt::Trap(message)) => assert!(!message.is_empty(), "the reason is carried"),
        other => panic!("expected a trap, got {other:?}"),
    }
}

#[test]
fn a_missing_entry_point_names_the_export() {
    let outcome = run(
        b"\0asm\x01\0\0\0",
        "run",
        b"",
        limits(),
        revoked(false),
    );
    match outcome.halt {
        Some(Halt::Trap(message)) => assert!(
            message.contains("run"),
            "the trap names the export that was missing: {message}"
        ),
        other => panic!("expected a trap, got {other:?}"),
    }
}

#[test]
fn a_host_call_that_outlives_its_budget_stops_the_run() {
    // A zero per-call budget: the call is over before it is measured, so every host call must report the host as the ceiling. This is the
    // mechanism a slow import queue lands on — the check is after the work, so a host that took too long is named as such rather than
    // being blamed on the module's fuel.
    let outcome = run(
        &writer(4),
        "run",
        b"",
        Limits {
            host_call_timeout: Duration::ZERO,
            ..limits()
        },
        revoked(false),
    );
    assert_eq!(outcome.halt, Some(Halt::HostTimeout), "{outcome:?}");
}

#[test]
fn the_concurrency_bound_counts_and_frees() {
    let bound = darkhold_sandbox::Concurrency::new(2);
    assert_eq!(bound.limit(), 2);
    let first = bound.try_enter().expect("first slot");
    let second = bound.try_enter().expect("second slot");
    assert_eq!(bound.active(), 2);
    assert!(
        bound.try_enter().is_none(),
        "the third run is refused, not queued: a queue in front of a sandbox is a backlog with no ceiling"
    );
    drop(first);
    assert_eq!(bound.active(), 1);
    let third = bound.try_enter().expect("a freed slot is reusable");
    drop((second, third));
    assert_eq!(bound.active(), 0);
}

#[test]
fn a_zero_concurrency_bound_still_runs_one() {
    // `0` is a mistake or a disabled feature; the module gate disables actions, so this crate reads it as one rather than silently
    // refusing everything forever.
    let bound = darkhold_sandbox::Concurrency::new(0);
    assert_eq!(bound.limit(), 1);
    assert!(bound.try_enter().is_some());
}

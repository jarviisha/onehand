//! onehand patch: cumulative foreground-thread timings for diagnostic builds.
//!
//! These are elapsed host times, not GPU timestamps or on-CPU samples. Paint
//! includes the other render stages; do not add it to them. No timers, logging
//! or counter updates are compiled into builds without the `profiling` feature.

use std::{cell::Cell, time::Instant};

// Bounded, payload-free event traces are separate from the timing counters so
// collecting a diagnostic trace is an explicit choice, not benchmark overhead.
use std::{cell::RefCell, collections::VecDeque, sync::OnceLock};

pub struct Event {
    pub nanos: u64,
    pub kind: &'static str,
    pub bytes: usize,
    pub requested: bool,
}

thread_local! {
    static EVENTS: RefCell<VecDeque<Event>> = const { RefCell::new(VecDeque::new()) };
}

pub(crate) fn event(kind: &'static str, bytes: usize, requested: bool) {
    record(kind, bytes, requested, std::time::Duration::ZERO);
}

/// Record a GPUI submission collected later, on the same monotonic time axis
/// as the PTY events. This is not a compositor presentation timestamp.
pub fn presented(age: std::time::Duration) {
    record("present", 0, false, age);
}

fn record(kind: &'static str, bytes: usize, requested: bool, age: std::time::Duration) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    static START: OnceLock<Instant> = OnceLock::new();
    if !*ENABLED.get_or_init(|| std::env::var_os("ONEHAND_TERMINAL_TRACE").is_some()) {
        return;
    }
    let nanos = START.get_or_init(Instant::now).elapsed().saturating_sub(age).as_nanos() as u64;
    EVENTS.with_borrow_mut(|events| {
        if events.len() == 4096 {
            events.pop_front();
        }
        events.push_back(Event { nanos, kind, bytes, requested });
    });
}

/// Drain a bounded diagnostic trace on the foreground thread. PTY contents and
/// keystrokes are never recorded. More than 4,096 events between drains evicts
/// old entries; use only short traces, not this API as a lossless recorder.
pub fn take_events() -> Vec<Event> {
    EVENTS.with_borrow_mut(|events| events.drain(..).collect())
}

#[derive(Clone, Copy, Default)]
pub struct Sample {
    pub calls: u64,
    pub nanos: u64,
    pub units: u64,
}

#[derive(Clone, Copy)]
pub(crate) enum Stage {
    Parse,
    Paint,
    Backgrounds,
    Boxes,
    Runs,
    Shape,
    Glyphs,
    Cursor,
}

const NAMES: [&str; 8] = [
    "parse", "paint", "backgrounds", "boxes", "runs", "shape", "glyphs", "cursor",
];

thread_local! {
    static SAMPLES: Cell<[Sample; 8]> = const { Cell::new([Sample { calls: 0, nanos: 0, units: 0 }; 8]) };
}

/// Read on the GPUI foreground thread. Reading does not request a frame.
pub fn snapshot() -> [(&'static str, Sample); 8] {
    let samples = SAMPLES.get();
    std::array::from_fn(|index| (NAMES[index], samples[index]))
}

pub(crate) struct Span {
    stage: Stage,
    units: u64,
    start: Instant,
}

impl Span {
    pub(crate) fn new(stage: Stage, units: usize) -> Self {
        Self { stage, units: units as u64, start: Instant::now() }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        let nanos = self.start.elapsed().as_nanos() as u64;
        let mut samples = SAMPLES.get();
        let sample = &mut samples[self.stage as usize];
        sample.calls += 1;
        sample.nanos += nanos;
        sample.units += self.units;
        SAMPLES.set(samples);
    }
}

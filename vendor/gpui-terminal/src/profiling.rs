//! onehand patch: cumulative foreground-thread timings for diagnostic builds.
//!
//! These are elapsed host times, not GPU timestamps or on-CPU samples. Paint
//! includes the other render stages; do not add it to them. No timers, logging
//! or counter updates are compiled into builds without the `profiling` feature.

use std::{cell::Cell, time::Instant};

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

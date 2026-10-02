//! Pipelines: a run of named steps, each a prompt, a command or a person's
//! approval, with onehand checking every agent turn against its gates before
//! the run moves on.
//!
//! The parts:
//! - a [`Template`] says what the steps are, and is kept one file each
//!   ([`store`]) beside the ones onehand ships ([`builtin`]);
//! - [`validate`] says what keeps one from being saved or run;
//! - a [`PipelineRun`] is one run as pure state: it takes reports and says
//!   what to do next, and its snapshot is written in order by [`files`];
//! - [`Facts`] and [`Mark`] are what is read of the work, never the agent's
//!   word for it.

pub mod builtin;
mod facts;
pub mod files;
mod prompt;
mod run;
pub mod store;
mod template;
mod validate;

pub use facts::{run_command_blocking, Facts, Mark};
pub use run::{Action, Brief, Marks, Outcome, PipelineRun, Setup, Stop, Transition};
pub use template::{GateKind, Place, StepKind, StepSpec, Template, SCHEMA_VERSION};
pub use validate::{validate, Problem};

#[cfg(test)]
mod tests;

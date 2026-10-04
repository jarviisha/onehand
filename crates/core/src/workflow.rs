//! Workflows: a run of named steps, each a prompt, a command or a person's
//! approval, with onehand checking every agent turn against its gates before
//! the run moves on.
//!
//! The parts:
//! - a [`Template`] says what the steps are, and is kept one file each
//!   ([`store`]) beside the ones onehand ships ([`builtin`]);
//! - [`validate`] says what keeps one from being saved or run;
//! - a [`Run`] is one run as pure state: it takes reports and says
//!   what to do next, and is kept in its task (`crate::task`);
//! - [`Facts`] and [`Mark`] are what is read of the work, never the agent's
//!   word for it.

pub mod builtin;
mod facts;
mod prompt;
mod run;
pub mod store;
mod template;
mod validate;

pub use facts::{run_command_blocking, Facts, Mark};
pub use prompt::first_prompt;
pub use run::{Action, Brief, Outcome, Run, Setup, Stop, Visit};
pub use template::{GateKind, Place, StepKind, StepSpec, Template, SCHEMA_VERSION};
pub use validate::validate;

/// The branch a run on a worktree works on: `workflow/<title words>`, built
/// only from characters a branch name may hold.
pub fn branch_for(title: &str) -> String {
    format!("workflow/{}", store::slug(title))
}

#[cfg(test)]
mod tests;

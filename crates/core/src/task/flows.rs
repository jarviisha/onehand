//! Whole application flows across drives, with every drive real and joined
//! by its links, and no front end: what a front end does is only to carry
//! out what comes back, which [`harness`] plays with core's blocking bodies
//! on scratch repositories.

pub(crate) mod harness;
mod schedule;
mod unattended;

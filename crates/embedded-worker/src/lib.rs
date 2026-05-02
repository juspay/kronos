//! Kronos worker pipeline as an embeddable library. Moved from `kronos-worker`
//! in Plan 2 of the embedded-mode initiative.

pub mod backoff;
pub mod dispatcher;
pub mod pipeline;
pub mod poller;

mod builder;
mod error;
mod handle;
mod worker;

pub use builder::WorkerBuilder;
pub use error::BuildError;
pub use handle::WorkerHandle;
pub use worker::Worker;

//! Kronos worker pipeline as an embeddable library. Moved from `kronos-worker`
//! in Plan 2 of the embedded-mode initiative; the public builder/handle API is
//! introduced in Tasks 3-5.

pub mod backoff;
pub mod dispatcher;
pub mod pipeline;
pub mod poller;

//! `kronos-api` server library — exposes the handlers, middleware, router,
//! and extractors so integration tests can build an in-process Actix app.

pub mod dashboard;
pub mod extractors;
pub mod handlers;
pub mod middleware;
pub mod router;

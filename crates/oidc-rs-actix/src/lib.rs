//! Actix-Web adapter for [`oidc-rs`](https://docs.rs/oidc-rs).
//!
//! Provides an [`AuthMiddleware`] that authenticates inbound requests against
//! the configured [`oidc_rs::AuthConfig`] (Bearer JWT validation + Basic→JWT
//! exchange) and injects an [`oidc_rs::Identity`] into request extensions.
//! The [`Authenticated`] extractor reads that identity in handlers.

#![deny(rust_2018_idioms)]
#![warn(missing_docs)]

mod error;
mod extractor;
mod middleware;

pub use error::to_response as auth_error_to_response;
pub use extractor::Authenticated;
pub use middleware::{AuthMiddleware, AuthMode, AuthState};

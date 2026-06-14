//! Lightweight OIDC Resource Server primitives for Rust services.
//!
//! See the crate README for an overview.

#![deny(rust_2018_idioms)]
#![warn(missing_docs)]

mod identity;

pub use identity::{AuthError, Claims, Identity};

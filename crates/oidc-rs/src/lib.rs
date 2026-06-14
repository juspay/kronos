//! Lightweight OIDC Resource Server primitives for Rust services.
//!
//! See the crate README for an overview.

#![deny(rust_2018_idioms)]
#![warn(missing_docs)]

mod config;
mod exchanger;
mod identity;
mod validator;

pub use config::{AuthConfig, AuthConfigBuilder, BuildError, EnabledConfig};
pub use exchanger::BasicExchanger;
pub use identity::{AuthError, Claims, Identity};
pub use validator::Validator;

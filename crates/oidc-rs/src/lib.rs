//! Lightweight OIDC Resource Server primitives for Rust services.
//!
//! - [`Validator`] verifies inbound JWTs against a configured OIDC issuer's
//!   JWKS.
//! - [`BasicExchanger`] caches `client_credentials`-grant exchanges so that
//!   API callers can present `Authorization: Basic` and avoid the extra
//!   token-endpoint roundtrip.
//! - [`AuthConfig`] is a framework- and env-agnostic builder.
//! - [`Identity`] is the result attached to authenticated requests.
//!
//! See `oidc-rs-actix` for the Actix-Web middleware that ties these together.

#![deny(rust_2018_idioms)]
#![warn(missing_docs)]

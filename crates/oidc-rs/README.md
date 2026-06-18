# oidc-rs

Lightweight OIDC Resource Server primitives for Rust services. Validates inbound JWTs against an OIDC issuer's JWKS, and exchanges `Authorization: Basic <base64(client_id:client_secret)>` credentials against the issuer's `client_credentials` grant (cached, single-flighted) so that machine clients don't pay a token-endpoint roundtrip per request.

Framework-agnostic. See `oidc-rs-actix` for the Actix-Web adapter.

## Status

Working title; API is pre-1.0 and may evolve. Currently used in production by [Kronos](https://github.com/anthropics/kronos).

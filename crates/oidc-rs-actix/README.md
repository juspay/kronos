# oidc-rs-actix

Actix-Web adapter for [`oidc-rs`](../oidc-rs/README.md). Provides:

- `AuthMiddleware` — a `Transform` that runs auth (Bearer + Basic) on every request.
- `Authenticated(Identity)` — extractor for handlers.

## Status

Working title; API is pre-1.0.

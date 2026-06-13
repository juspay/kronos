# OIDC Authentication for Kronos — Design

**Status:** Proposed
**Date:** 2026-06-14
**Author:** brainstorming session with Natarajan

---

## Summary

Replace Kronos's single shared bearer token (`TE_API_KEY`) with a real authentication
model that supports both interactive dashboard users and machine-to-machine (M2M)
callers, while keeping the change surgical and operator-friendly. All credentials
live in an external OIDC provider; Kronos itself stores nothing. The API accepts
two header formats — `Bearer <jwt>` and `Basic <client_id:client_secret>` — both
of which resolve to a JWT that is validated against the configured OIDC issuer.
A `TE_AUTH_MODE=disabled` env exists for local development.

The dashboard becomes a public OIDC client (PKCE in WASM) holding tokens in memory
only, with no refresh tokens and no browser-storage persistence. On token expiry
or tab reload it redirects to the IdP, which silently bounces back if the IdP's
own SSO session is still alive.

This change is identity-only. It does **not** introduce per-org / per-workspace
authorization — `X-Org-Id` / `X-Workspace-Id` headers continue to be trusted once
a caller is authenticated, matching today's behaviour. That layer is a deliberate
follow-up.

---

## Scope & non-goals

**In scope**

- Two credential formats at the API edge, both producing a validated OIDC JWT:
  - `Authorization: Bearer <jwt>` — validated against the configured issuer's JWKS.
  - `Authorization: Basic <base64(client_id:client_secret)>` — exchanged by Kronos
    against the IdP's token endpoint via the `client_credentials` grant, then
    validated through the same path as Bearer.
- A `TE_AUTH_MODE` env: `enabled` (default) or `disabled` (bypass for local dev).
- Single OIDC provider per Kronos instance, configured via env (issuer URL +
  accepted audiences).
- Dashboard performs authorization-code + PKCE as a public client in WASM. Tokens
  live in a Leptos signal in memory; no `localStorage` / `sessionStorage`
  persistence beyond the brief PKCE-verifier-across-redirect window.
- Identity (issuer + subject + email/name for humans; issuer + subject for M2M)
  is captured per request and surfaced into request extensions and structured logs.
- New endpoints:
  - `GET /v1/auth/whoami` — returns the current identity.
  - `POST /v1/auth/cache/flush` — evicts entries from the Basic-credentials cache
    (operational tool for credential rotation).
- In-memory cache for Basic→JWT exchanges, with per-client_id single-flight,
  negative-caching of IdP rejections, and a configurable hard TTL cap.

**Out of scope** (explicit non-goals — deferred to later work)

- Mapping authenticated identity to allowed orgs / workspaces. `X-Org-Id` and
  `X-Workspace-Id` headers remain trusted once authenticated.
- A Kronos-side `users`, `memberships`, or roles model.
- Multi-issuer / multi-tenant IdP support.
- Server-side sessions / BFF pattern (rejected in favour of SPA-side tokens).
- Refresh tokens or token storage in browser web-storage.
- CLI device-code flow.
- Token introspection (RFC 7662) — JWT signature validation only.
- "Log out everywhere" / token revocation lists. Token validity is bounded by
  the JWT `exp`; rotation in the IdP is honoured at most one cache-TTL later
  unless `/v1/auth/cache/flush` is called.
- Dashboard role-gated UI.

---

## Authentication flows

### M2M (`Authorization: Basic`)

Per request:

1. Parse the `Authorization: Basic <…>` header → `(client_id, secret)`.
2. Cache key `= (client_id, sha256(secret))`. Look up in the in-memory
   Basic-exchange cache.
   - **Hit (not within 60 s of expiry):** use the cached JWT, validate it
     through the JWT path below, attach identity, forward.
   - **Miss or near-expiry:** acquire the per-`client_id` single-flight lock,
     re-check the cache (another waiter may have just populated it), then:
3. POST to the IdP's token endpoint with `grant_type=client_credentials`,
   `client_id`, `client_secret`, `audience=TE_OIDC_M2M_AUDIENCE` (if set),
   `scope=TE_OIDC_M2M_SCOPE` (if set). The `openidconnect` crate handles
   discovery of the token endpoint from `<issuer>/.well-known/openid-configuration`
   and chooses HTTP-Basic vs form-encoded credential transport per IdP metadata.
4. On success: cache `(client_id, sha256(secret)) → (jwt, expires_at)` where
   `expires_at = min(jwt.exp, now + TE_OIDC_BASIC_CACHE_TTL_SEC) - 60s`. Validate
   the JWT through the same code path as Bearer, attach `Identity::M2M`, forward.
5. On IdP `401`/`403`: insert a **negative cache** entry for 30 s; return
   `401 Unauthorized` to the caller. Subsequent requests with the same
   `(client_id, sha256(secret))` short-circuit to `401` for the negative-cache
   lifetime without retrying the IdP.
6. On IdP network/5xx error: insert a short (5 s) negative cache entry; return
   `503 Service Unavailable` with `Retry-After: 5`.

**Single-flight** prevents a thundering herd at restart: 100 services arriving
simultaneously with the same Basic creds produce one outbound token-exchange
request, not 100. Implementation: a `tokio::sync::Mutex<()>` per cache slot,
stored in a `dashmap::DashMap`.

**Why hash the secret in the cache key:** the raw secret enters the middleware
briefly. Hashing the key avoids the raw secret living in the cache structure
across requests, reducing exposure in crash dumps and stray debug output. This
is hygiene, not storage-at-rest defence (the JWT and the hash both live in
process memory).

### Human (`Authorization: Bearer`)

Per request:

1. Parse the `Authorization: Bearer <jwt>` header.
2. Extract the JWS header's `kid`. Look up the JWK in the in-memory JWKS cache.
3. On miss: trigger a JWKS refetch (rate-limited to once per 10 s to prevent
   thrash on unknown-kid floods). If still missing, return `401`.
4. Verify:
   - signature
   - `iss == TE_OIDC_ISSUER`
   - `aud` intersects `TE_OIDC_AUDIENCES` (configured as a comma-separated list)
   - `exp > now - 60 s` (60 s clock-skew tolerance)
   - `nbf <= now + 60 s` (if present)
5. Extract `sub`, `email`, `name` (whichever are present). Attach
   `Identity::Human { iss, sub, email, name }`, forward.

JWKS is fetched at startup. **Startup failure mode:** if the JWKS fetch fails
at startup, the API still binds and starts serving — auth requests return
`503 Service Unavailable` with `Retry-After` until JWKS lands. Background refresh
runs every `TE_OIDC_JWKS_REFRESH_SEC` (default 300 s).

### Dashboard (PKCE in WASM)

The dashboard is a public OIDC client. No client secret is held by the browser.

1. Any protected route loaded with no token in `LoginState` triggers:
   - Generate `code_verifier` (43–128 chars, URL-safe random).
   - Generate `state` and `nonce` (cryptographic random).
   - Persist `code_verifier` + `state` to `sessionStorage` under a single key
     (it survives the redirect, is same-origin, and is wiped on successful
     callback).
   - Redirect the browser to the IdP's `authorization_endpoint` with
     `response_type=code`, `client_id`, `redirect_uri`, `scope=openid email profile`,
     `state`, `nonce`, `code_challenge=S256(code_verifier)`,
     `code_challenge_method=S256`.
2. IdP redirects back to `redirect_uri?code=…&state=…`.
3. Callback handler: read `code_verifier` + `state` from `sessionStorage`,
   wipe it, verify the returned `state` matches.
4. POST to the IdP's `token_endpoint` with `grant_type=authorization_code`,
   `code`, `redirect_uri`, `client_id`, `code_verifier`.
5. Validate the returned `id_token` (signature against IdP JWKS, `iss`, `aud`,
   `nonce`). Place `(access_token, id_token, exp)` into the `LoginState`
   Leptos signal. **Tokens never touch web storage.**
6. The API client adds `Authorization: Bearer <access_token>` to all calls.
   On any `401`, the dashboard discards `LoginState` and restarts from step 1.

**Lifecycle:**

- Token expiry, tab reload, new tab → no token in memory → step 1.
- If the IdP's own SSO session is still alive, the browser bounces through
  `/authorize` invisibly (no user-visible login screen).
- If the IdP's session is gone, the user sees the IdP login UI.
- Logout: clear `LoginState` and redirect to the IdP's `end_session_endpoint`
  (if advertised in discovery), else just clear local state and return to `/`.

### `TE_AUTH_MODE=disabled`

The auth middleware short-circuits and injects `Identity::Disabled` into request
extensions. The API server emits a single `WARN`-level log at startup:

> `Auth disabled (TE_AUTH_MODE=disabled). Do not use in production.`

The dashboard, compiled with `TE_AUTH_DISABLED=1`, skips OIDC entirely and makes
unauthenticated API calls.

---

## Identity model

```rust
// crates/common/src/auth.rs
pub enum Identity {
    Disabled,
    Human { iss: String, sub: String, email: Option<String>, name: Option<String> },
    M2M    { iss: String, sub: String, scopes: Vec<String> },
}
```

The `Human` vs `M2M` variant is chosen by **which header path produced the
JWT**, not by inspecting JWT claims. This is reliable across IdPs (claim
conventions for distinguishing M2M from interactive tokens vary).

`Identity` is placed into the Actix request extensions by `AuthMiddleware` and
read by the `AuthenticatedRequest` extractor — handler signatures stay the same.

---

## Endpoints

All listed endpoints require an authenticated identity unless noted.

| Method | Path | Description |
|---|---|---|
| `GET` | `/v1/auth/whoami` | Returns the current `Identity` as JSON. Used by the dashboard post-login and for debugging. |
| `POST` | `/v1/auth/cache/flush` | Evicts entries from the Basic-exchange cache. Body: `{ "client_id": "..." }` to evict one client, or `{}` to flush all. Returns counts of positive- and negative-cache entries evicted. |

Response shape for `/v1/auth/whoami`:

```jsonc
// human
{ "type": "human", "iss": "...", "sub": "...",
  "email": "alice@example.com", "name": "Alice" }
// m2m
{ "type": "m2m", "iss": "...", "sub": "...",
  "scopes": ["jobs.read", "jobs.write"] }
// disabled
{ "type": "disabled" }
```

**No `m2m_clients` management endpoints exist.** All M2M credentials are created,
rotated, and revoked in the IdP admin console.

---

## Configuration

### API server env

| Env | Default | Description |
|---|---|---|
| `TE_AUTH_MODE` | `enabled` | `enabled` \| `disabled`. `disabled` bypasses all auth and synthesizes `Identity::Disabled`. |
| `TE_OIDC_ISSUER` | *(required if enabled)* | OIDC issuer URL. Discovery → `/.well-known/openid-configuration` provides token endpoint and JWKS URI. |
| `TE_OIDC_AUDIENCES` | *(required if enabled)* | Comma-separated list of accepted `aud` values on inbound JWTs (typically the dashboard `client_id` plus any M2M audiences). |
| `TE_OIDC_M2M_AUDIENCE` | *(empty)* | The `audience` value Kronos requests when exchanging Basic credentials. Required for Auth0-style IdPs; ignored if the IdP does not use the parameter. |
| `TE_OIDC_M2M_SCOPE` | *(empty)* | Space-separated scopes Kronos requests during Basic exchange. |
| `TE_OIDC_BASIC_CACHE_TTL_SEC` | `3600` | Hard cap on Basic→JWT cache lifetime, regardless of the JWT's own `exp`. |
| `TE_OIDC_JWKS_REFRESH_SEC` | `300` | Background JWKS refresh interval. |
| `TE_API_KEY` | — | **Removed.** If set with `TE_AUTH_MODE=enabled`, a one-line WARN is emitted at startup pointing at the migration docs. |

**Startup validation:** when `TE_AUTH_MODE=enabled`, the API exits non-zero
with a single clear error message if `TE_OIDC_ISSUER` or `TE_OIDC_AUDIENCES`
is missing.

### Dashboard env (compile-time, baked into WASM)

| Env | Default | Description |
|---|---|---|
| `TE_AUTH_DISABLED` | `0` | If `1`, the dashboard skips the OIDC flow entirely. Intended for use against an API running in `TE_AUTH_MODE=disabled`. |
| `TE_OIDC_ISSUER` | *(required unless disabled)* | Same as the API. |
| `TE_OIDC_CLIENT_ID` | *(required unless disabled)* | Dashboard's public OIDC `client_id` at the IdP. |
| `TE_OIDC_REDIRECT_URL` | *(required unless disabled)* | e.g. `https://kronos.example.com/dashboard/auth/callback`. Must be registered with the IdP for the dashboard client. |

### docker-compose defaults

- `docker-compose.yml` (dev): defaults to `TE_AUTH_MODE=disabled`. `just dev`
  works unchanged.
- `docker-compose.prod.yml`: requires the operator to set OIDC envs explicitly;
  startup fails fast otherwise.

---

## Code structure

Uses the Rust 2018+ flat module style (`module.rs` next to `module/` directory),
matching the existing convention in `crates/common/src` and the rest of the
codebase. No `mod.rs` files.

### `crates/common/`

- `src/auth.rs` — declares submodules, re-exports `Identity`, `AuthError`,
  `AuthConfig`, `Validator`, `BasicExchanger`.
- `src/auth/config.rs` — `AuthConfig` parsed from env, with `Enabled { issuer,
  audiences, m2m_audience, m2m_scope, basic_cache_ttl, jwks_refresh }` and
  `Disabled` variants.
- `src/auth/oidc.rs` — JWKS client + JWT validator. Wraps the `openidconnect`
  crate; provides `Validator::validate(jwt: &str) -> Result<Claims, AuthError>`.
- `src/auth/token_exchange.rs` — `BasicExchanger` struct holding:
  - the positive cache (`DashMap<(String, [u8; 32]), CachedJwt>`),
  - the negative cache (`DashMap<(String, [u8; 32]), Instant>`),
  - per-`client_id` single-flight `DashMap<String, Arc<Mutex<()>>>`,
  - and the OIDC token-endpoint client.

### `crates/api/`

- `src/middleware.rs` — add `AuthMiddleware` Actix `Transform` alongside the
  existing `RequestId`.
- `src/extractors.rs` — `AuthenticatedRequest` becomes a thin extractor that
  reads `Identity` from request extensions. Callers don't change their
  signatures; the field type behind the extractor narrows from "bool-ish" to
  `Identity` access.
- `src/handlers/auth.rs` — `GET /v1/auth/whoami`, `POST /v1/auth/cache/flush`.
- `src/router.rs` — wires `AuthMiddleware` over `/v1/*` and registers the new
  routes.

The existing `crates/api/src/handlers.rs` already declares a `handlers/`
submodule (the same flat pattern), so `handlers/auth.rs` fits without touching
the parent's style.

### `crates/dashboard/` (Leptos/WASM)

- `src/auth.rs` — declares submodules; exports a global `LoginState` Leptos
  signal and redirect helpers.
- `src/auth/pkce.rs` — `code_verifier` + `state` + `nonce` generation,
  `sessionStorage` persistence across the IdP redirect.
- `src/auth/callback.rs` — callback route handler: validates `state`, exchanges
  code for tokens, validates `id_token`, populates `LoginState`.
- `src/api/client.rs` — injects `Authorization: Bearer` from `LoginState`; on
  `401` discards `LoginState` and kicks the PKCE flow.

### Dependencies

- Added: `openidconnect ≈ 3.x` in `kronos-common` and `kronos-dashboard`.
  The `dashboard` build needs the crate's `wasm-bindgen`-compatible features
  (verified at integration time; fallback is a ~150-line hand-rolled PKCE flow
  if bundle-size or compile complaints emerge).
- Added: `dashmap` (already a transitive dep of several crates in the workspace).
- Removed: nothing (no `argon2` was ever introduced since the M2M-storage path
  was discarded during design).

---

## Migration & operator impact

This change is **breaking** for any caller using
`Authorization: Bearer dev-api-key` against an API with `TE_AUTH_MODE=enabled`.

### Local development

- `docker-compose.yml` defaults `TE_AUTH_MODE=disabled`. `just dev` runs
  unchanged.
- The README's curl examples drop the `Authorization` header in their default
  form, with a note that production deployments require it.

### Production

- `docker-compose.prod.yml` requires `TE_AUTH_MODE`, `TE_OIDC_ISSUER`, and
  `TE_OIDC_AUDIENCES`. Startup fails fast with a clear error when any are
  missing, pointing at a new README section.
- README gains a "Setting up authentication" section with three subsections:
  - Keycloak (free, self-hostable, docker-composable in one block) — primary
    walkthrough.
  - Auth0 — shorter note with the `audience` parameter requirement.
  - Okta — shorter note.

### TypeScript CLI / SDK

- The Smithy model enables both `httpBasicAuth` and `httpBearerAuth` traits on
  the service. The generator emits both credential options.
- CLI env vars (mutually exclusive, checked in order):
  - `KRONOS_CLIENT_ID` + `KRONOS_CLIENT_SECRET` → Basic
  - `KRONOS_BEARER_TOKEN` → Bearer
  - none → no `Authorization` header (works only against
    `TE_AUTH_MODE=disabled`).

### Rust client (`crates/client/`)

- Add `Config::with_basic(client_id, client_secret)` and
  `Config::with_bearer(token)`.
- Existing `Config::with_token` becomes a deprecated alias of `with_bearer`.

### Dashboard internal wiring being replaced

- `crates/dashboard/src/config.rs` — the `api_key: String` field on the
  dashboard config struct, populated from a `window.apiKey` JS global at boot
  (`crates/dashboard/src/app.rs`, `crates/dashboard/src/api/client.rs:28`),
  goes away.
- Every `config.api_key`-injected `Authorization: Bearer …` call site in
  `crates/dashboard/src/api/client.rs` (around twenty of them) is replaced by
  a single source of truth: the bearer header is supplied by reading the
  current `access_token` from the `LoginState` Leptos signal at request time.
  In `TE_AUTH_DISABLED=1` builds, the header is omitted entirely.

### Removed env

- `TE_API_KEY` — removed from `crates/common/src/config.rs` and
  `crates/common/src/env.rs`. If still set when `TE_AUTH_MODE=enabled`, a single
  WARN is logged at startup with a pointer to the migration docs.

---

## Testing

### Unit (`crates/common/src/auth/`)

- **JWT validator:**
  - valid token → ok
  - expired → error
  - clock skew within ±60 s → ok; beyond → error
  - wrong `iss` → error
  - `aud` not in configured list → error
  - bad signature → error
  - unknown `kid` (with JWKS refetch triggered, still missing) → error
  - missing `kid` → error
- **Token-exchange cache:**
  - cold miss → IdP call → cached
  - hit before near-expiry → no IdP call
  - hit within 60 s of expiry → refresh path triggered
  - negative cache after IdP 401 → second request short-circuits to 401 without
    hitting IdP
  - negative cache after IdP 5xx → 5 s only
  - `TE_OIDC_BASIC_CACHE_TTL_SEC` cap honoured when JWT `exp` is further out
- **Single-flight:**
  - 50 concurrent requests for the same `(client_id, secret)` against a mock
    IdP → exactly one outbound token-exchange call
- **Cache flush:**
  - by `client_id` → only matching entries removed
  - empty body → both positive and negative caches drained

### Integration (`crates/api/tests/`)

A mock OIDC issuer is spun up per test (using `wiremock` or the
`openidconnect` crate's test utilities — chosen during implementation based on
ergonomics).

- `TE_AUTH_MODE=disabled` → unauthenticated requests succeed;
  `/v1/auth/whoami` returns `{"type":"disabled"}`.
- Basic with valid creds → `200`; identity is `m2m`.
- Basic with invalid creds → `401`; second request within 30 s does not hit
  the mock IdP.
- Basic followed by `POST /v1/auth/cache/flush` → next request re-exchanges
  against the mock IdP.
- Bearer with valid JWT → `200`; identity is `human`.
- Bearer with expired JWT → `401`.
- Missing `Authorization` header → `401`.
- Startup with `TE_AUTH_MODE=enabled` and missing `TE_OIDC_ISSUER` → process
  exits non-zero with the expected error.

### Dashboard

Full browser automation is out of scope for this change. A manual checklist
lands in the README:

- Cold load → IdP redirect → login → land on dashboard with "logged in as …".
- Force access-token expiry → next navigation produces a seamless IdP bounce
  if the IdP SSO session is still alive.
- Logout → in-memory state cleared, redirect to IdP `end_session_endpoint`
  when advertised, otherwise back to the root.
- A CI build with `TE_AUTH_DISABLED=1` exercises the dashboard's no-OIDC code
  path without needing an IdP.

---

## Open questions / risks

- **`openidconnect` crate WASM build.** Compiles to WASM in principle, but the
  exact feature set and bundle-size impact will only be known at implementation.
  Fallback: hand-rolled ~150-line PKCE flow. Decision deferred to implementation.
- **Rotation grace window.** A rotated IdP secret keeps working in the
  Basic-exchange cache for up to `TE_OIDC_BASIC_CACHE_TTL_SEC` minus 60 s.
  Operators rotating secrets must either accept this lag or call
  `/v1/auth/cache/flush`. Documented.
- **IdP variance for the `client_credentials` grant.** Most major IdPs support
  it (Auth0, Okta, Keycloak, Azure AD, Cognito). Some lightweight or
  social-login-focused OIDC providers do not. Documented in the README as a
  prerequisite.
- **Worker not affected.** Workers don't expose an inbound API surface and
  don't authenticate inbound requests — they're explicitly out of scope.
- **Tenant authorization deferred.** Today's behaviour (any authenticated
  caller can address any org/workspace via header) is preserved. A follow-up
  spec will introduce identity-to-tenant authorization.

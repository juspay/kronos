//! PKCE (RFC 7636) artifact generation. Verifier + state + nonce are parked
//! in sessionStorage across the IdP redirect (same-origin, wiped on callback).
//! Tokens are NEVER stored here — only the verifier+state+nonce, which are
//! useless without the matching `code` from the IdP.

use base64::Engine;
use gloo_storage::{SessionStorage, Storage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const STORAGE_KEY: &str = "kronos_pkce";

/// PKCE-related artifacts that must outlive the IdP redirect round-trip:
/// the verifier (needed for the token exchange), the state (CSRF defence),
/// the nonce (ID-token replay defence), and where to navigate after the
/// callback completes successfully.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PkceArtifacts {
    pub code_verifier: String,
    pub state: String,
    pub nonce: String,
    pub return_to: String,
}

/// Generate a fresh set of PKCE artifacts. `return_to` should be the path
/// the user was trying to reach when they got bounced to the IdP.
pub fn generate_pkce(return_to: &str) -> PkceArtifacts {
    PkceArtifacts {
        code_verifier: random_base64url(64),
        state: random_base64url(24),
        nonce: random_base64url(24),
        return_to: return_to.to_string(),
    }
}

/// Derive the `code_challenge` (SHA-256 of the verifier, base64url-no-pad)
/// for the `/authorize` request.
pub fn code_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize())
}

/// Persist artifacts to sessionStorage. Same-origin, survives the
/// `/authorize` redirect, wiped by [`take`] on callback completion.
pub fn persist(artifacts: &PkceArtifacts) {
    let _ = SessionStorage::set(STORAGE_KEY, artifacts);
}

/// Pop the stored artifacts (delete-on-read). Returns `None` if the
/// callback was reached without a prior `persist` (probably a bookmark or
/// a stale tab).
pub fn take() -> Option<PkceArtifacts> {
    let val: Option<PkceArtifacts> = SessionStorage::get(STORAGE_KEY).ok();
    SessionStorage::delete(STORAGE_KEY);
    val
}

fn random_base64url(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).expect("getrandom failed");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&buf)
}

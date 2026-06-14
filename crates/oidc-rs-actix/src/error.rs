//! Map [`oidc_rs::AuthError`] to Actix [`HttpResponse`].

use actix_web::HttpResponse;
use oidc_rs::AuthError;

/// Build an HTTP error response for the given auth failure. 401 for client
/// errors, 503 (with `Retry-After: 5`) for transient IdP failures.
pub fn to_response(err: &AuthError) -> HttpResponse {
    let body = serde_json::json!({
        "error": { "code": code(err), "message": err.to_string() }
    });
    match err {
        AuthError::IdpUnreachable(_) => HttpResponse::ServiceUnavailable()
            .insert_header(("Retry-After", "5"))
            .json(body),
        _ => HttpResponse::Unauthorized().json(body),
    }
}

fn code(err: &AuthError) -> &'static str {
    match err {
        AuthError::MissingHeader => "MISSING_AUTHORIZATION",
        AuthError::MalformedHeader => "MALFORMED_AUTHORIZATION",
        AuthError::IdpRejected => "INVALID_CREDENTIALS",
        AuthError::Expired => "TOKEN_EXPIRED",
        AuthError::BadSignature => "BAD_SIGNATURE",
        AuthError::BadIssuer(_) => "BAD_ISSUER",
        AuthError::BadAudience => "BAD_AUDIENCE",
        AuthError::IdpUnreachable(_) => "IDP_UNREACHABLE",
        AuthError::IdpMalformedResponse(_) => "IDP_MALFORMED_RESPONSE",
    }
}

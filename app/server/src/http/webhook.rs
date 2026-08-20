//! Where a payment provider's callback lands.
//!
//! Neither the storefront's key nor the back office's token opens this: what
//! authenticates it is a signature over the exact body, checked here, because
//! the secret belongs to whoever configured the provider and tezgah never
//! sees it. That is the whole reason `Surface::Webhook` exists in the route
//! table rather than this being an admin route with the gate turned off.
//!
//! **Recorded, not acted on.** `receive_callback` writes the delivery down —
//! once, on conflict do nothing against the provider's own event id — and
//! answers. Capturing, moving an order's state, anything that follows from
//! what the provider said, is a second step against a row that is now
//! durable, so a crash between the two resumes rather than loses. What has
//! arrived and not been acted on is `GET /admin/payment-webhooks`.
//!
//! Unset secret, unmounted route. A callback endpoint that believes anybody
//! is worse than one that is not there: a provider retries a 404 and says so
//! on its dashboard, while an unsigned endpoint accepts a forged capture
//! quietly.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use subtle::ConstantTimeEq;
use tezgah::api::admin_order;
use tezgah::ports::{Actor, Ctx, Host};

use crate::deliver::signature;
use crate::http::{ApiError, AppState, begin};

/// Mounted only when a secret was configured. The second half of the tuple is
/// what `http::mod` adds to the count of bound routes.
pub fn router(secret: bool) -> (Router<AppState>, Vec<(&'static str, &'static str)>) {
    if !secret {
        return (Router::new(), Vec::new());
    }

    (
        Router::new().route("/webhooks/payments/{provider}", post(receive)),
        vec![("POST", "/webhooks/payments/{provider}")],
    )
}

/// The body is taken as bytes and parsed here rather than by `Json`, because
/// the signature is over what arrived. Letting axum deserialise first and
/// re-serialising to check would compare a signature against bytes the
/// provider never sent — the classic way a correct signature fails.
async fn receive(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Result<Json<admin_order::CallbackView>, ApiError> {
    let Some(secret) = state.webhook_secret.as_deref() else {
        return Err(tezgah::Error::denied().into());
    };

    let sent = headers
        .get("x-provider-signature")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    let expected = signature(secret, &String::from_utf8_lossy(&body));

    // Constant time, and the same refusal whether the header was missing,
    // malformed or wrong: an endpoint that answers differently to a
    // near-miss tells whoever is guessing that they are close.
    if !bool::from(sent.as_bytes().ct_eq(expected.as_bytes())) {
        return Err(tezgah::Error::denied().into());
    }

    let parsed: admin_order::ProviderCallback = serde_json::from_slice(&body).map_err(|err| {
        tezgah::Error::invalid(format!("a callback this shop cannot read: {err}"))
    })?;

    let mut tx = begin(&state.pool, state.scope).await?;
    // `System`, because nobody is asking: a provider's callback is this
    // installation acting on what it was told, and attributing it to an
    // operator would put a person's name on something they did not do.
    let ctx = Ctx::new(state.scope, Actor::System, state.host.as_ref() as &dyn Host);
    let view = admin_order::receive_callback(&mut tx, &ctx, &provider, parsed).await?;
    tx.commit().await?;

    Ok(Json(view))
}

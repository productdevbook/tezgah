//! Uploading an image, and serving it back.
//!
//! Two routes, and neither is one of tezgah's: the crate stores no files and
//! declares no path for one. `../README.md` says which of this binary's own
//! routes are in that position — `/health`, `/docs`, the accounts, and these.
//!
//! Mounted only when `TEZGAH_FILE_DIR` is set. A shop that hosts its images
//! somewhere else keeps doing that, and the panel goes on taking a URL.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::files::MAX_BYTES;
use crate::http::{ApiError, AppState};

/// The upload is gated by `require_operator` where `http::mod` layers it;
/// reading one back is not, because an image on a storefront is public and a
/// signed URL for a product photo is ceremony.
pub fn router(store: bool) -> (Router<AppState>, Vec<(&'static str, &'static str)>) {
    if !store {
        return (Router::new(), Vec::new());
    }

    (
        Router::new().route("/files/{name}", get(serve)),
        vec![("GET", "/files/{name}")],
    )
}

pub fn admin_router(store: bool) -> (Router<AppState>, Vec<(&'static str, &'static str)>) {
    if !store {
        return (Router::new(), Vec::new());
    }

    (
        Router::new()
            .route("/admin/files", post(upload))
            // Axum's own default is 2 MB and this needs its own: the store
            // refuses anything larger anyway, and a request cut off by the
            // framework arrives as a protocol error rather than as the
            // sentence explaining the limit.
            .layer(DefaultBodyLimit::max(MAX_BYTES + 64 * 1024)),
        vec![("POST", "/admin/files")],
    )
}

#[derive(Debug, serde::Serialize)]
pub struct StoredView {
    /// Where it is now. This is what goes in a product's `thumbnail_url`.
    pub url: String,
}

async fn upload(
    State(state): State<AppState>,
    mut form: Multipart,
) -> Result<Json<StoredView>, ApiError> {
    let Some(store) = state.files.as_ref() else {
        return Err(tezgah::Error::not_found("file store").into());
    };

    while let Some(field) = form
        .next_field()
        .await
        .map_err(|err| tezgah::Error::invalid(format!("that upload is malformed: {err}")))?
    {
        // The content type the browser attached, checked against a list
        // rather than believed — `files::extension_for` is where that happens
        // and why an SVG is not an image here.
        let declared = field
            .content_type()
            .map(str::to_owned)
            .unwrap_or_else(|| "application/octet-stream".into());

        let bytes: Bytes = field
            .bytes()
            .await
            .map_err(|err| tezgah::Error::invalid(format!("that upload stopped: {err}")))?;

        let url = store.save(&declared, &bytes).await?;
        return Ok(Json(StoredView { url }));
    }

    Err(tezgah::Error::invalid("that upload carried no file").into())
}

async fn serve(State(state): State<AppState>, Path(name): Path<String>) -> Response {
    let Some(store) = state.files.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };

    match store.read(&name).await {
        Ok((bytes, kind)) => (
            [
                (header::CONTENT_TYPE, HeaderValue::from_static(kind)),
                // The type is the one this binary chose when it stored the
                // file, and `nosniff` is what stops a browser deciding it
                // knows better from the bytes.
                (
                    header::X_CONTENT_TYPE_OPTIONS,
                    HeaderValue::from_static("nosniff"),
                ),
                // Immutable: the name carries a uuid, so the bytes behind one
                // never change.
                (
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=31536000, immutable"),
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

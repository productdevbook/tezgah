//! Digital products, on both surfaces.
//!
//! The back office says what a variant carries and can take a right back; a
//! shopper lists what they own, asks for a link and spends one download of it.
//!
//! # The key leaves once a download is counted
//!
//! [`redeem`] hands back the storage key, and it is the only thing here that
//! ever does. Listing an entitlement does not: a list is read far more often
//! than a file is fetched, and a key that arrives with every page is a key in
//! every log between here and the browser.

use serde::{Deserialize, Serialize};

use crate::digital;
use crate::error::Result;
use crate::id::{CustomerId, DigitalContentId, OrderEntitlementId, OrderId, VariantId};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, Ctx, Tx};

use super::{Method, Route, Surface, signed_in};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct List {
    pub after: Option<String>,
    pub limit: Option<u32>,
}

impl List {
    fn paging(&self) -> Result<Paging> {
        let limit = self.limit.unwrap_or(crate::page::DEFAULT_LIMIT);
        match self.after.as_deref() {
            Some(text) => Ok(Paging::after(Cursor::decode(text)?, limit)),
            None => Ok(Paging::first(limit)),
        }
    }
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ContentView {
    pub id: DigitalContentId,
    pub variant_id: VariantId,
    pub content_key: String,
    pub name: String,
    pub max_downloads: Option<i32>,
    pub valid_days: Option<i32>,
    pub auto_grant: bool,
    pub rank: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<digital::DigitalContent> for ContentView {
    fn from(row: digital::DigitalContent) -> Self {
        ContentView {
            id: row.id,
            variant_id: row.variant_id,
            content_key: row.content_key,
            name: row.name,
            max_downloads: row.max_downloads,
            valid_days: row.valid_days,
            auto_grant: row.auto_grant,
            rank: row.rank,
            created_at: row.created_at,
        }
    }
}

/// An entitlement as it leaves the building. No `content_key`: what a shopper
/// needs is whether they may still download it, not where it is kept.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EntitlementView {
    pub id: OrderEntitlementId,
    pub order_id: OrderId,
    pub digital_content_id: DigitalContentId,
    pub max_downloads: Option<i32>,
    pub download_count: i32,
    pub remaining: Option<i32>,
    pub granted_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub revoked_reason: Option<String>,
}

impl From<digital::OrderEntitlement> for EntitlementView {
    fn from(row: digital::OrderEntitlement) -> Self {
        EntitlementView {
            remaining: row.remaining(),
            id: row.id,
            order_id: row.order_id,
            digital_content_id: row.digital_content_id,
            max_downloads: row.max_downloads,
            download_count: row.download_count,
            granted_at: row.granted_at,
            expires_at: row.expires_at,
            revoked_at: row.revoked_at,
            revoked_reason: row.revoked_reason,
        }
    }
}

/// The one answer carrying a token, the way an issued gift card is the one
/// answer carrying a code.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TokenView {
    pub token: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub entitlement: EntitlementView,
}

/// What the host hands to its own storage. tezgah has counted the download by
/// the time this is returned.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DownloadView {
    pub entitlement_id: OrderEntitlementId,
    pub content_key: String,
    pub download_count: i32,
    pub remaining: Option<i32>,
}

// ---------------------------------------------------------------------------
// Admin
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PutContent {
    pub content_key: String,
    pub name: String,
    pub max_downloads: Option<i32>,
    pub valid_days: Option<i32>,
    pub auto_grant: Option<bool>,
    pub rank: Option<i32>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn put_content(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
    body: PutContent,
) -> Result<ContentView> {
    let new = digital::NewContent {
        content_key: body.content_key,
        name: body.name,
        max_downloads: body.max_downloads,
        valid_days: body.valid_days,
        auto_grant: body.auto_grant,
        rank: body.rank,
        metadata: body.metadata,
    };

    Ok(ContentView::from(
        digital::put_content(tx, ctx, variant_id, new).await?,
    ))
}

pub async fn list_content(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
) -> Result<Vec<ContentView>> {
    Ok(digital::contents(tx, ctx, variant_id)
        .await?
        .into_iter()
        .map(ContentView::from)
        .collect())
}

pub async fn delete_content(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: DigitalContentId) -> Result<()> {
    digital::delete_content(tx, ctx, id).await
}

pub async fn list_order_entitlements(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
) -> Result<Vec<EntitlementView>> {
    Ok(digital::entitlements(tx, ctx, order_id)
        .await?
        .into_iter()
        .map(EntitlementView::from)
        .collect())
}

#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RevokeEntitlements {
    pub reason: Option<String>,
}

/// Takes an order's entitlements back by hand. The refund path does this on
/// its own; this is for the case somebody has to answer without one.
pub async fn revoke_entitlements(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    body: RevokeEntitlements,
) -> Result<Vec<EntitlementView>> {
    Ok(digital::revoke(tx, ctx, order_id, body.reason.as_deref())
        .await?
        .into_iter()
        .map(EntitlementView::from)
        .collect())
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

pub async fn my_entitlements(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: List,
) -> Result<Page<EntitlementView>> {
    let customer = signed_in(ctx)?;
    let page = digital::for_customer(tx, ctx, customer, query.paging()?).await?;

    Ok(Page {
        items: page.items.into_iter().map(EntitlementView::from).collect(),
        next: page.next,
    })
}

pub async fn create_token(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderEntitlementId,
) -> Result<TokenView> {
    let customer: CustomerId = signed_in(ctx)?;
    let issued = digital::issue_token(tx, ctx, id, Some(customer)).await?;

    Ok(TokenView {
        token: issued.token,
        expires_at: issued.expires_at,
        entitlement: EntitlementView::from(issued.entitlement),
    })
}

#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Redeem {
    pub token: String,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

/// Spends one download. The count has moved by the time this answers, so a
/// host that fails to serve the bytes afterwards has still spent one — which
/// is the safe way round for the shop and the reason the token is short-lived.
pub async fn redeem(tx: &mut Tx<'_>, ctx: &Ctx<'_>, body: Redeem) -> Result<DownloadView> {
    let customer = signed_in(ctx)?;
    let taken = digital::redeem(
        tx,
        ctx,
        &body.token,
        digital::Access {
            as_customer: Some(customer),
            ip: body.ip,
            user_agent: body.user_agent,
        },
    )
    .await?;

    Ok(DownloadView {
        entitlement_id: taken.entitlement_id,
        content_key: taken.content_key,
        download_count: taken.download_count,
        remaining: taken.remaining,
    })
}

pub(super) static ROUTES: &[Route] = &[
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/variants/{id}/digital-content",
        action: Action::Write,
        domain: "digital",
        query: None,
        summary: "Put a file on a variant",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/variants/{id}/digital-content",
        action: Action::View,
        domain: "digital",
        query: Some(super::query_schema::<super::Paged>),
        summary: "List the files a variant carries",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/digital-content/{id}",
        action: Action::Delete,
        domain: "digital",
        query: None,
        summary: "Take a file off a variant",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/orders/{id}/entitlements",
        action: Action::View,
        domain: "digital",
        query: None,
        summary: "What this order's money bought the right to",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/orders/{id}/entitlements/revoke",
        action: Action::Settle,
        domain: "digital",
        query: None,
        summary: "Take an order's downloads back",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/entitlements",
        action: Action::View,
        domain: "digital",
        query: None,
        summary: "What I may download",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/entitlements/{id}/token",
        action: Action::View,
        domain: "digital",
        query: None,
        summary: "Ask for a download link",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/downloads",
        action: Action::View,
        domain: "digital",
        query: None,
        summary: "Spend one download of a link",
    },
];

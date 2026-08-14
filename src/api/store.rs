//! What a storefront may ask.
//!
//! Everything here is reachable by somebody who is not signed in, or signed in
//! as a shopper. Two rules follow from that and are not negotiable per-route:
//! a draft product is invisible, and a customer only ever reaches their own
//! cart, their own orders and their own addresses.

use serde::{Deserialize, Serialize};

use crate::catalogue;
use crate::error::Result;
use crate::id::{ProductId, VariantId};
use crate::page::{Page, Paging};
use crate::ports::{Ctx, Tx};

use super::{Method, Route, Surface};
use crate::ports::Action;

/// A product as a storefront sees it.
///
/// Deliberately not `catalogue::Product`: that carries the status, the internal
/// notes and whatever a migration adds next, and none of that should reach a
/// shopper because nobody remembered to strip it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductView {
    pub id: ProductId,
    pub handle: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub thumbnail: Option<String>,
}

impl From<catalogue::Product> for ProductView {
    fn from(row: catalogue::Product) -> Self {
        ProductView {
            id: row.id,
            handle: row.handle,
            title: row.title,
            subtitle: row.subtitle,
            description: row.description,
            thumbnail: row.thumbnail,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantView {
    pub id: VariantId,
    pub title: String,
    pub sku: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListProducts {
    pub after: Option<String>,
    pub limit: Option<u32>,
    pub collection_id: Option<ProductId>,
}

impl ListProducts {
    fn paging(&self) -> Result<Paging> {
        let limit = self.limit.unwrap_or(crate::page::DEFAULT_LIMIT);
        match &self.after {
            Some(text) => Ok(Paging::after(crate::page::Cursor::decode(text)?, limit)),
            None => Ok(Paging::first(limit)),
        }
    }
}

/// Published products only, and never anything else, whatever is asked for.
pub async fn list_products(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListProducts,
) -> Result<Page<ProductView>> {
    let filter = catalogue::ProductFilter {
        status: Some(catalogue::ProductStatus::Published),
        ..Default::default()
    };

    let page = catalogue::products(tx, ctx, filter, query.paging()?).await?;

    Ok(Page {
        items: page.items.into_iter().map(ProductView::from).collect(),
        next: page.next,
    })
}

pub async fn get_product(tx: &mut Tx<'_>, ctx: &Ctx<'_>, handle: &str) -> Result<ProductView> {
    let row = catalogue::product_by_handle(tx, ctx, handle).await?;

    if row.status != catalogue::ProductStatus::Published {
        // Not "forbidden": telling a stranger a draft exists is telling them
        // something about the shop they were not shown.
        return Err(crate::Error::not_found("product"));
    }

    Ok(ProductView::from(row))
}

pub(super) static ROUTES: &[Route] = &[
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/products",
        action: Action::View,
        domain: "catalogue",
        summary: "List published products",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/products/{handle}",
        action: Action::View,
        domain: "catalogue",
        summary: "Fetch one published product by its handle",
    },
];

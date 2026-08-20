//! What a back office may ask of the catalogue, its prices and its stock.
//!
//! The difference from [`super::store`] is not a longer list of fields: it is
//! that a draft and an archived product are visible here, because the person
//! looking is the one who wrote them. Everything else follows the same rule —
//! a view is not a row, and `scope` leaves in nothing.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::batch;
use crate::catalogue;
use crate::error::{Error, Result};
use crate::id::{
    BundleComponentId, CategoryId, CollectionId, InventoryItemId, InventoryLevelId, LineItemId,
    OptionId, OptionValueId, PriceId, PriceListId, PriceSetId, ProductId, ProductImageId,
    ProductTagId, ProductTypeId, ReservationId, SalesChannelId, StockLocationId, StockTransferId,
    VariantId,
};
use crate::inventory;
use crate::money::{Currency, Money};
use crate::order;
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, Ctx, Tx};
use crate::pricing;
use crate::store;

use super::{Method, Route, Surface};

// ---------------------------------------------------------------------------
// Paging, asked for the one way
// ---------------------------------------------------------------------------

fn paging(after: Option<&str>, limit: Option<u32>) -> Result<Paging> {
    let limit = limit.unwrap_or(crate::page::DEFAULT_LIMIT);
    match after {
        Some(text) => Ok(Paging::after(Cursor::decode(text)?, limit)),
        None => Ok(Paging::first(limit)),
    }
}

fn map_page<T, U: From<T>>(page: Page<T>) -> Page<U> {
    Page {
        items: page.items.into_iter().map(U::from).collect(),
        next: page.next,
    }
}

/// Tells a field left out apart from one sent as `null`.
///
/// serde reads `null` into an `Option` as `None`, which makes "leave it alone"
/// and "clear it" the same request; this keeps them two.
fn double_option<'de, T, D>(deserializer: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

/// Every plain listing takes this and nothing else.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    pub after: Option<String>,
    pub limit: Option<u32>,
}

impl ListQuery {
    fn paging(&self) -> Result<Paging> {
        paging(self.after.as_deref(), self.limit)
    }
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

/// A product as its own shop sees it: the status included, because deciding
/// what to do next is the whole point of the screen this feeds.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProductView {
    pub id: ProductId,
    pub handle: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub status: catalogue::ProductStatus,
    pub rejected_reason: Option<String>,
    pub thumbnail_url: Option<String>,
    pub is_discountable: bool,
    pub product_type_id: Option<ProductTypeId>,
    pub product_collection_id: Option<CollectionId>,
    pub weight: Option<Decimal>,
    pub length: Option<Decimal>,
    pub height: Option<Decimal>,
    pub width: Option<Decimal>,
    pub material: Option<String>,
    pub hs_code: Option<String>,
    pub origin_country: Option<String>,
    pub external_id: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<catalogue::Product> for ProductView {
    fn from(row: catalogue::Product) -> Self {
        ProductView {
            id: row.id,
            handle: row.handle,
            title: row.title,
            subtitle: row.subtitle,
            description: row.description,
            status: row.status,
            rejected_reason: row.rejected_reason,
            thumbnail_url: row.thumbnail_url,
            is_discountable: row.is_discountable,
            product_type_id: row.product_type_id,
            product_collection_id: row.product_collection_id,
            weight: row.weight,
            length: row.length,
            height: row.height,
            width: row.width,
            material: row.material,
            hs_code: row.hs_code,
            origin_country: row.origin_country,
            external_id: row.external_id,
            metadata: row.metadata,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VariantView {
    pub id: VariantId,
    pub product_id: ProductId,
    pub title: String,
    pub sku: Option<String>,
    pub barcode: Option<String>,
    pub ean: Option<String>,
    pub upc: Option<String>,
    pub weight: Option<Decimal>,
    pub length: Option<Decimal>,
    pub height: Option<Decimal>,
    pub width: Option<Decimal>,
    pub material: Option<String>,
    pub hs_code: Option<String>,
    pub origin_country: Option<String>,
    pub mid_code: Option<String>,
    pub manages_inventory: bool,
    pub allows_backorder: bool,
    pub rank: i32,
    pub withdrawal_exclusion: Option<String>,
    pub is_giftcard: bool,
    pub requires_shipping: bool,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<catalogue::ProductVariant> for VariantView {
    fn from(row: catalogue::ProductVariant) -> Self {
        VariantView {
            id: row.id,
            product_id: row.product_id,
            title: row.title,
            sku: row.sku,
            barcode: row.barcode,
            ean: row.ean,
            upc: row.upc,
            weight: row.weight,
            length: row.length,
            height: row.height,
            width: row.width,
            material: row.material,
            hs_code: row.hs_code,
            origin_country: row.origin_country,
            mid_code: row.mid_code,
            manages_inventory: row.manages_inventory,
            allows_backorder: row.allows_backorder,
            rank: row.rank,
            withdrawal_exclusion: row.withdrawal_exclusion_reason,
            is_giftcard: row.is_giftcard,
            requires_shipping: row.requires_shipping,
            metadata: row.metadata,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionView {
    pub id: OptionId,
    pub product_id: ProductId,
    pub title: String,
    pub rank: i32,
}

impl From<catalogue::ProductOption> for OptionView {
    fn from(row: catalogue::ProductOption) -> Self {
        OptionView {
            id: row.id,
            product_id: row.product_id,
            title: row.title,
            rank: row.rank,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionValueView {
    pub id: OptionValueId,
    pub option_id: OptionId,
    pub value: String,
    pub rank: i32,
}

impl From<catalogue::ProductOptionValue> for OptionValueView {
    fn from(row: catalogue::ProductOptionValue) -> Self {
        OptionValueView {
            id: row.id,
            option_id: row.option_id,
            value: row.value,
            rank: row.rank,
        }
    }
}

/// One option with everything it can be set to — the grid a variant list is
/// drawn from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionMatrixView {
    pub option: OptionView,
    pub values: Vec<OptionValueView>,
}

impl From<catalogue::OptionWithValues> for OptionMatrixView {
    fn from(row: catalogue::OptionWithValues) -> Self {
        OptionMatrixView {
            option: OptionView::from(row.option),
            values: row.values.into_iter().map(OptionValueView::from).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionView {
    pub id: CollectionId,
    pub handle: String,
    pub title: String,
    pub external_id: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<catalogue::ProductCollection> for CollectionView {
    fn from(row: catalogue::ProductCollection) -> Self {
        CollectionView {
            id: row.id,
            handle: row.handle,
            title: row.title,
            external_id: row.external_id,
            metadata: row.metadata,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryView {
    pub id: CategoryId,
    pub parent_id: Option<CategoryId>,
    pub name: String,
    pub handle: String,
    pub description: String,
    pub rank: i32,
    pub is_active: bool,
    /// Only a back office sees this one: an internal category is a shelf in the
    /// warehouse rather than a page on the shop.
    pub is_internal: bool,
    pub depth: usize,
    pub external_id: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<catalogue::ProductCategory> for CategoryView {
    fn from(row: catalogue::ProductCategory) -> Self {
        CategoryView {
            depth: row.depth(),
            id: row.id,
            parent_id: row.parent_id,
            name: row.name,
            handle: row.handle,
            description: row.description,
            rank: row.rank,
            is_active: row.is_active,
            is_internal: row.is_internal,
            external_id: row.external_id,
            metadata: row.metadata,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagView {
    pub id: ProductTagId,
    pub value: String,
    pub external_id: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<catalogue::ProductTag> for TagView {
    fn from(row: catalogue::ProductTag) -> Self {
        TagView {
            id: row.id,
            value: row.value,
            external_id: row.external_id,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeView {
    pub id: ProductTypeId,
    pub value: String,
    pub external_id: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<catalogue::ProductType> for TypeView {
    fn from(row: catalogue::ProductType) -> Self {
        TypeView {
            id: row.id,
            value: row.value,
            external_id: row.external_id,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageView {
    pub id: ProductImageId,
    pub product_id: ProductId,
    pub url: String,
    pub alt_text: Option<String>,
    pub rank: i32,
}

impl From<catalogue::ProductImage> for ImageView {
    fn from(row: catalogue::ProductImage) -> Self {
        ImageView {
            id: row.id,
            product_id: row.product_id,
            url: row.url,
            alt_text: row.alt_text,
            rank: row.rank,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationView {
    pub product_id: ProductId,
    pub locale: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub handle: Option<String>,
}

impl From<catalogue::ProductTranslation> for TranslationView {
    fn from(row: catalogue::ProductTranslation) -> Self {
        TranslationView {
            product_id: row.product_id,
            locale: row.locale,
            title: row.title,
            subtitle: row.subtitle,
            description: row.description,
            handle: row.handle,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalisedView {
    pub product_id: ProductId,
    pub locale: Option<String>,
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub handle: String,
    pub is_fallback: bool,
}

impl From<catalogue::Localised> for LocalisedView {
    fn from(row: catalogue::Localised) -> Self {
        LocalisedView {
            product_id: row.product_id,
            locale: row.locale,
            title: row.title,
            subtitle: row.subtitle,
            description: row.description,
            handle: row.handle,
            is_fallback: row.is_fallback,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PriceSetView {
    pub id: PriceSetId,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<pricing::PriceSet> for PriceSetView {
    fn from(row: pricing::PriceSet) -> Self {
        PriceSetView {
            id: row.id,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PriceView {
    pub id: PriceId,
    pub price_set_id: PriceSetId,
    pub price_list_id: Option<PriceListId>,
    pub title: Option<String>,
    pub amount: Decimal,
    pub currency_code: String,
    pub min_quantity: Option<i32>,
    pub max_quantity: Option<i32>,
    pub rules_count: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<pricing::Price> for PriceView {
    fn from(row: pricing::Price) -> Self {
        PriceView {
            id: row.id,
            price_set_id: row.price_set_id,
            price_list_id: row.price_list_id,
            title: row.title,
            amount: row.amount,
            currency_code: row.currency_code,
            min_quantity: row.min_quantity,
            max_quantity: row.max_quantity,
            rules_count: row.rules_count,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceRuleView {
    pub id: uuid::Uuid,
    pub price_id: PriceId,
    pub attribute: String,
    pub value: String,
    pub operator: String,
    pub priority: i32,
}

impl From<pricing::PriceRule> for PriceRuleView {
    fn from(row: pricing::PriceRule) -> Self {
        PriceRuleView {
            id: row.id,
            price_id: row.price_id,
            attribute: row.attribute,
            value: row.value,
            operator: row.operator,
            priority: row.priority,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceListView {
    pub id: PriceListId,
    pub title: String,
    pub description: Option<String>,
    pub kind: String,
    pub status: String,
    pub starts_at: Option<chrono::DateTime<chrono::Utc>>,
    pub ends_at: Option<chrono::DateTime<chrono::Utc>>,
    pub rules_count: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<pricing::PriceList> for PriceListView {
    fn from(row: pricing::PriceList) -> Self {
        PriceListView {
            id: row.id,
            title: row.title,
            description: row.description,
            kind: row.kind,
            status: row.status,
            starts_at: row.starts_at,
            ends_at: row.ends_at,
            rules_count: row.rules_count,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceListRuleView {
    pub id: uuid::Uuid,
    pub price_list_id: PriceListId,
    pub attribute: String,
    pub allowed_values: Vec<String>,
}

impl From<pricing::PriceListRule> for PriceListRuleView {
    fn from(row: pricing::PriceListRule) -> Self {
        PriceListRuleView {
            id: row.id,
            price_list_id: row.price_list_id,
            attribute: row.attribute,
            allowed_values: row.allowed_values,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricePreferenceView {
    pub id: uuid::Uuid,
    pub attribute: String,
    pub value: Option<String>,
    pub is_tax_inclusive: bool,
}

impl From<pricing::PricePreference> for PricePreferenceView {
    fn from(row: pricing::PricePreference) -> Self {
        PricePreferenceView {
            id: row.id,
            attribute: row.attribute,
            value: row.value,
            is_tax_inclusive: row.is_tax_inclusive,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StockLocationView {
    pub id: StockLocationId,
    pub name: String,
    pub address_id: Option<uuid::Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<inventory::StockLocation> for StockLocationView {
    fn from(row: inventory::StockLocation) -> Self {
        StockLocationView {
            id: row.id,
            name: row.name,
            address_id: row.address_id,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InventoryItemView {
    pub id: InventoryItemId,
    pub sku: Option<String>,
    pub title: Option<String>,
    pub requires_shipping: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<inventory::InventoryItem> for InventoryItemView {
    fn from(row: inventory::InventoryItem) -> Self {
        InventoryItemView {
            id: row.id,
            sku: row.sku,
            title: row.title,
            requires_shipping: row.requires_shipping,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InventoryLevelView {
    pub id: InventoryLevelId,
    pub inventory_item_id: InventoryItemId,
    pub location_id: StockLocationId,
    pub stocked_quantity: i32,
    pub reserved_quantity: i32,
    pub incoming_quantity: i32,
    pub available_quantity: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<inventory::InventoryLevel> for InventoryLevelView {
    fn from(row: inventory::InventoryLevel) -> Self {
        InventoryLevelView {
            id: row.id,
            inventory_item_id: row.inventory_item_id,
            location_id: row.location_id,
            stocked_quantity: row.stocked_quantity,
            reserved_quantity: row.reserved_quantity,
            incoming_quantity: row.incoming_quantity,
            available_quantity: row.available_quantity,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReservationView {
    pub id: ReservationId,
    pub inventory_item_id: InventoryItemId,
    pub location_id: StockLocationId,
    pub quantity: i32,
    pub line_item_id: Option<LineItemId>,
    pub allows_backorder: bool,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<inventory::Reservation> for ReservationView {
    fn from(row: inventory::Reservation) -> Self {
        ReservationView {
            id: row.id,
            inventory_item_id: row.inventory_item_id,
            location_id: row.location_id,
            quantity: row.quantity,
            line_item_id: row.line_item_id,
            allows_backorder: row.allows_backorder,
            expires_at: row.expires_at,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockTransferView {
    pub id: StockTransferId,
    pub inventory_item_id: InventoryItemId,
    pub from_location_id: StockLocationId,
    pub to_location_id: StockLocationId,
    pub quantity: i32,
    pub status: String,
    pub reason: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<inventory::StockTransfer> for StockTransferView {
    fn from(row: inventory::StockTransfer) -> Self {
        StockTransferView {
            id: row.id,
            inventory_item_id: row.inventory_item_id,
            from_location_id: row.from_location_id,
            to_location_id: row.to_location_id,
            quantity: row.quantity,
            status: row.status,
            reason: row.reason,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantInventoryItemView {
    pub variant_id: VariantId,
    pub inventory_item_id: InventoryItemId,
    pub required_quantity: i32,
}

impl From<inventory::VariantInventoryItem> for VariantInventoryItemView {
    fn from(row: inventory::VariantInventoryItem) -> Self {
        VariantInventoryItemView {
            variant_id: row.variant_id,
            inventory_item_id: row.inventory_item_id,
            required_quantity: row.required_quantity,
        }
    }
}

// ---------------------------------------------------------------------------
// Products
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListProducts {
    pub after: Option<String>,
    pub limit: Option<u32>,
    /// Left out means every status, drafts and archives included. That is the
    /// difference between this surface and the storefront's.
    pub status: Option<catalogue::ProductStatus>,
    pub collection: Option<CollectionId>,
    pub product_type: Option<ProductTypeId>,
    pub category: Option<CategoryId>,
    pub tag: Option<ProductTagId>,
}

pub async fn list_products(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListProducts,
) -> Result<Page<ProductView>> {
    let filter = catalogue::ProductFilter {
        status: query.status,
        collection: query.collection,
        product_type: query.product_type,
        category: query.category,
        tag: query.tag,
        channels: None,
    };
    let page = catalogue::products(
        tx,
        ctx,
        filter,
        paging(query.after.as_deref(), query.limit)?,
    )
    .await?;
    Ok(map_page(page))
}

#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateProduct {
    pub handle: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub status: Option<catalogue::ProductStatus>,
    pub thumbnail_url: Option<String>,
    pub is_discountable: Option<bool>,
    pub product_type_id: Option<ProductTypeId>,
    pub product_collection_id: Option<CollectionId>,
    pub weight: Option<Decimal>,
    pub length: Option<Decimal>,
    pub height: Option<Decimal>,
    pub width: Option<Decimal>,
    pub material: Option<String>,
    pub hs_code: Option<String>,
    pub origin_country: Option<String>,
    pub external_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn create_product(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreateProduct,
) -> Result<ProductView> {
    let new = catalogue::NewProduct {
        handle: body.handle,
        title: body.title,
        subtitle: body.subtitle,
        description: body.description,
        status: body.status,
        thumbnail_url: body.thumbnail_url,
        is_discountable: body.is_discountable,
        product_type_id: body.product_type_id,
        product_collection_id: body.product_collection_id,
        weight: body.weight,
        length: body.length,
        height: body.height,
        width: body.width,
        material: body.material,
        hs_code: body.hs_code,
        origin_country: body.origin_country,
        external_id: body.external_id,
        metadata: body.metadata,
    };
    Ok(ProductView::from(
        catalogue::create_product(tx, ctx, new).await?,
    ))
}

pub async fn get_product(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductId) -> Result<ProductView> {
    Ok(ProductView::from(catalogue::product(tx, ctx, id).await?))
}

/// A field left out is left alone; `null` on a nullable link clears it.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateProduct {
    pub handle: Option<String>,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub thumbnail_url: Option<String>,
    pub is_discountable: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    pub product_type_id: Option<Option<ProductTypeId>>,
    #[serde(default, deserialize_with = "double_option")]
    pub product_collection_id: Option<Option<CollectionId>>,
    pub weight: Option<Decimal>,
    pub length: Option<Decimal>,
    pub height: Option<Decimal>,
    pub width: Option<Decimal>,
    pub material: Option<String>,
    pub hs_code: Option<String>,
    pub origin_country: Option<String>,
    pub external_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn update_product(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ProductId,
    body: UpdateProduct,
) -> Result<ProductView> {
    let patch = catalogue::ProductPatch {
        handle: body.handle,
        title: body.title,
        subtitle: body.subtitle,
        description: body.description,
        thumbnail_url: body.thumbnail_url,
        is_discountable: body.is_discountable,
        product_type_id: body.product_type_id,
        product_collection_id: body.product_collection_id,
        weight: body.weight,
        length: body.length,
        height: body.height,
        width: body.width,
        material: body.material,
        hs_code: body.hs_code,
        origin_country: body.origin_country,
        external_id: body.external_id,
        metadata: body.metadata,
    };
    Ok(ProductView::from(
        catalogue::update_product(tx, ctx, id, patch).await?,
    ))
}

pub async fn delete_product(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductId) -> Result<()> {
    catalogue::delete_product(tx, ctx, id).await
}

pub async fn publish_product(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductId) -> Result<ProductView> {
    Ok(ProductView::from(
        catalogue::publish_product(tx, ctx, id).await?,
    ))
}

pub async fn archive_product(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductId) -> Result<ProductView> {
    Ok(ProductView::from(
        catalogue::archive_product(tx, ctx, id).await?,
    ))
}

pub async fn submit_product_for_review(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ProductId,
) -> Result<ProductView> {
    Ok(ProductView::from(
        catalogue::submit_for_review(tx, ctx, id).await?,
    ))
}

pub async fn approve_product(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductId) -> Result<ProductView> {
    Ok(ProductView::from(
        catalogue::approve_product(tx, ctx, id).await?,
    ))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectProduct {
    pub reason: String,
}

pub async fn reject_product(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ProductId,
    body: RejectProduct,
) -> Result<ProductView> {
    Ok(ProductView::from(
        catalogue::reject_product(tx, ctx, id, &body.reason).await?,
    ))
}

// ---------------------------------------------------------------------------
// Product images
// ---------------------------------------------------------------------------

pub async fn list_images(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
) -> Result<Vec<ImageView>> {
    let rows = catalogue::images(tx, ctx, product_id).await?;
    Ok(rows.into_iter().map(ImageView::from).collect())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddImage {
    pub url: String,
    pub alt_text: Option<String>,
    pub rank: Option<i32>,
}

pub async fn add_image(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    body: AddImage,
) -> Result<ImageView> {
    let new = catalogue::NewImage {
        url: body.url,
        alt_text: body.alt_text,
        rank: body.rank,
    };
    Ok(ImageView::from(
        catalogue::add_image(tx, ctx, product_id, new).await?,
    ))
}

pub async fn remove_image(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductImageId) -> Result<()> {
    catalogue::remove_image(tx, ctx, id).await
}

// ---------------------------------------------------------------------------
// Images on a variant
// ---------------------------------------------------------------------------

pub async fn list_variant_images(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
) -> Result<Vec<ImageView>> {
    Ok(catalogue::images_for_variant(tx, ctx, variant_id)
        .await?
        .into_iter()
        .map(ImageView::from)
        .collect())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachVariantImage {
    pub image_id: ProductImageId,
}

pub async fn attach_image_to_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
    body: AttachVariantImage,
) -> Result<()> {
    catalogue::attach_image_to_variant(tx, ctx, variant_id, body.image_id).await
}

pub async fn detach_image_from_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
    image_id: ProductImageId,
) -> Result<()> {
    catalogue::detach_image_from_variant(tx, ctx, variant_id, image_id).await
}

// ---------------------------------------------------------------------------
// Tags on a product
// ---------------------------------------------------------------------------

pub async fn list_product_tags(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
) -> Result<Vec<TagView>> {
    let rows = catalogue::product_tags(tx, ctx, product_id).await?;
    Ok(rows.into_iter().map(TagView::from).collect())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachTag {
    pub tag_id: ProductTagId,
}

pub async fn tag_product(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    body: AttachTag,
) -> Result<()> {
    catalogue::tag_product(tx, ctx, product_id, body.tag_id).await
}

pub async fn untag_product(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    tag_id: ProductTagId,
) -> Result<()> {
    catalogue::untag_product(tx, ctx, product_id, tag_id).await
}

// ---------------------------------------------------------------------------
// Categories on a product
// ---------------------------------------------------------------------------

pub async fn list_product_categories(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
) -> Result<Vec<CategoryView>> {
    let rows = catalogue::product_categories(tx, ctx, product_id).await?;
    Ok(rows.into_iter().map(CategoryView::from).collect())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachCategory {
    pub category_id: CategoryId,
}

pub async fn add_product_to_category(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    body: AttachCategory,
) -> Result<()> {
    catalogue::add_product_to_category(tx, ctx, product_id, body.category_id).await
}

pub async fn remove_product_from_category(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    category_id: CategoryId,
) -> Result<()> {
    catalogue::remove_product_from_category(tx, ctx, product_id, category_id).await
}

// ---------------------------------------------------------------------------
// Product sales channels
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ProductChannelView {
    pub id: SalesChannelId,
    pub name: String,
    pub is_disabled: bool,
}

impl From<store::SalesChannel> for ProductChannelView {
    fn from(row: store::SalesChannel) -> Self {
        ProductChannelView {
            id: row.id,
            name: row.name,
            is_disabled: row.is_disabled,
        }
    }
}

pub async fn list_product_channels(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
) -> Result<Vec<ProductChannelView>> {
    let rows = catalogue::channels_for_product(tx, ctx, product_id).await?;
    Ok(rows.into_iter().map(ProductChannelView::from).collect())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachChannel {
    pub sales_channel_id: SalesChannelId,
}

pub async fn add_product_to_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    body: AttachChannel,
) -> Result<()> {
    catalogue::add_product_to_channel(tx, ctx, product_id, body.sales_channel_id).await
}

pub async fn remove_product_from_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    channel_id: SalesChannelId,
) -> Result<()> {
    catalogue::remove_product_from_channel(tx, ctx, product_id, channel_id).await
}

// ---------------------------------------------------------------------------
// Product options
// ---------------------------------------------------------------------------

pub async fn option_matrix(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
) -> Result<Vec<OptionMatrixView>> {
    let rows = catalogue::option_matrix(tx, ctx, product_id).await?;
    Ok(rows.into_iter().map(OptionMatrixView::from).collect())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddOption {
    pub title: String,
    pub rank: Option<i32>,
}

pub async fn add_option(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    body: AddOption,
) -> Result<OptionView> {
    let row =
        catalogue::add_option(tx, ctx, product_id, &body.title, body.rank.unwrap_or(0)).await?;
    Ok(OptionView::from(row))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddOptionValue {
    pub value: String,
    pub rank: Option<i32>,
}

pub async fn add_option_value(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    option_id: OptionId,
    body: AddOptionValue,
) -> Result<OptionValueView> {
    let row = catalogue::add_option_value(tx, ctx, option_id, &body.value, body.rank.unwrap_or(0))
        .await?;
    Ok(OptionValueView::from(row))
}

// ---------------------------------------------------------------------------
// Variants
// ---------------------------------------------------------------------------

pub async fn list_variants(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    query: ListQuery,
) -> Result<Page<VariantView>> {
    let page = catalogue::variants(tx, ctx, product_id, query.paging()?).await?;
    Ok(map_page(page))
}

#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateVariant {
    pub title: String,
    pub sku: Option<String>,
    pub barcode: Option<String>,
    pub ean: Option<String>,
    pub upc: Option<String>,
    pub weight: Option<Decimal>,
    pub length: Option<Decimal>,
    pub height: Option<Decimal>,
    pub width: Option<Decimal>,
    pub material: Option<String>,
    pub hs_code: Option<String>,
    pub origin_country: Option<String>,
    pub mid_code: Option<String>,
    pub manages_inventory: Option<bool>,
    pub allows_backorder: Option<bool>,
    pub rank: Option<i32>,
    /// Why buying this is outside the right of withdrawal.
    pub withdrawal_exclusion: Option<String>,
    /// Whether selling this sells money rather than goods.
    pub is_giftcard: Option<bool>,
    /// Whether a line selling this needs a shipping address. Defaults to
    /// `true`: not tracking a variant's stock is not the same fact as it
    /// being a digital good.
    pub requires_shipping: Option<bool>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn create_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    body: CreateVariant,
) -> Result<VariantView> {
    let new = catalogue::NewVariant {
        title: body.title,
        sku: body.sku,
        barcode: body.barcode,
        ean: body.ean,
        upc: body.upc,
        weight: body.weight,
        length: body.length,
        height: body.height,
        width: body.width,
        material: body.material,
        hs_code: body.hs_code,
        origin_country: body.origin_country,
        mid_code: body.mid_code,
        manages_inventory: body.manages_inventory,
        allows_backorder: body.allows_backorder,
        rank: body.rank,
        withdrawal_exclusion: body
            .withdrawal_exclusion
            .as_deref()
            .map(order::WithdrawalExclusion::parse)
            .transpose()?,
        is_giftcard: body.is_giftcard,
        requires_shipping: body.requires_shipping,
        metadata: body.metadata,
    };
    Ok(VariantView::from(
        catalogue::create_variant(tx, ctx, product_id, new).await?,
    ))
}

pub async fn get_variant(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: VariantId) -> Result<VariantView> {
    Ok(VariantView::from(catalogue::variant(tx, ctx, id).await?))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateVariant {
    pub title: Option<String>,
    pub sku: Option<String>,
    pub barcode: Option<String>,
    pub ean: Option<String>,
    pub upc: Option<String>,
    pub weight: Option<Decimal>,
    pub length: Option<Decimal>,
    pub height: Option<Decimal>,
    pub width: Option<Decimal>,
    pub material: Option<String>,
    pub hs_code: Option<String>,
    pub origin_country: Option<String>,
    pub mid_code: Option<String>,
    pub manages_inventory: Option<bool>,
    pub allows_backorder: Option<bool>,
    pub rank: Option<i32>,
    /// Absent leaves it alone; `null` puts the variant back inside the right.
    #[serde(default, deserialize_with = "double_option")]
    pub withdrawal_exclusion: Option<Option<String>>,
    pub is_giftcard: Option<bool>,
    pub requires_shipping: Option<bool>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn update_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: VariantId,
    body: UpdateVariant,
) -> Result<VariantView> {
    let patch = catalogue::VariantPatch {
        title: body.title,
        sku: body.sku,
        barcode: body.barcode,
        ean: body.ean,
        upc: body.upc,
        weight: body.weight,
        length: body.length,
        height: body.height,
        width: body.width,
        material: body.material,
        hs_code: body.hs_code,
        origin_country: body.origin_country,
        mid_code: body.mid_code,
        manages_inventory: body.manages_inventory,
        allows_backorder: body.allows_backorder,
        rank: body.rank,
        withdrawal_exclusion: match body.withdrawal_exclusion {
            Some(Some(reason)) => Some(Some(order::WithdrawalExclusion::parse(&reason)?)),
            Some(None) => Some(None),
            None => None,
        },
        is_giftcard: body.is_giftcard,
        requires_shipping: body.requires_shipping,
        metadata: body.metadata,
    };
    Ok(VariantView::from(
        catalogue::update_variant(tx, ctx, id, patch).await?,
    ))
}

pub async fn delete_variant(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: VariantId) -> Result<()> {
    catalogue::delete_variant(tx, ctx, id).await
}

pub async fn variant_options(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
) -> Result<Vec<OptionValueView>> {
    let rows = catalogue::variant_options(tx, ctx, variant_id).await?;
    Ok(rows.into_iter().map(OptionValueView::from).collect())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetVariantOptions {
    pub values: Vec<OptionValueId>,
}

pub async fn set_variant_options(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
    body: SetVariantOptions,
) -> Result<()> {
    catalogue::set_variant_options(tx, ctx, variant_id, &body.values).await
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerateVariants {
    /// Combinations the shop does not sell, each one value per option.
    #[serde(default)]
    pub exclude: Vec<Vec<OptionValueId>>,
    pub manages_inventory: Option<bool>,
    pub allows_backorder: Option<bool>,
}

pub async fn generate_variants(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    body: GenerateVariants,
) -> Result<Vec<VariantView>> {
    let plan = catalogue::VariantPlan {
        exclude: body
            .exclude
            .into_iter()
            .map(catalogue::Combination)
            .collect(),
        manages_inventory: body.manages_inventory,
        allows_backorder: body.allows_backorder,
    };
    let made = catalogue::generate_variants(tx, ctx, product_id, plan).await?;
    Ok(made.into_iter().map(VariantView::from).collect())
}

// ---------------------------------------------------------------------------
// Collections
// ---------------------------------------------------------------------------

pub async fn list_collections(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListQuery,
) -> Result<Page<CollectionView>> {
    Ok(map_page(
        catalogue::collections(tx, ctx, query.paging()?).await?,
    ))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCollection {
    pub title: String,
    pub handle: String,
    pub external_id: Option<String>,
}

pub async fn create_collection(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreateCollection,
) -> Result<CollectionView> {
    Ok(CollectionView::from(
        catalogue::create_collection(
            tx,
            ctx,
            &body.title,
            &body.handle,
            body.external_id.as_deref(),
        )
        .await?,
    ))
}

pub async fn get_collection(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CollectionId,
) -> Result<CollectionView> {
    Ok(CollectionView::from(
        catalogue::collection(tx, ctx, id).await?,
    ))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateCollection {
    pub title: Option<String>,
    pub handle: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub external_id: Option<Option<String>>,
}

pub async fn update_collection(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CollectionId,
    body: UpdateCollection,
) -> Result<CollectionView> {
    let row = catalogue::update_collection(
        tx,
        ctx,
        id,
        body.title.as_deref(),
        body.handle.as_deref(),
        body.external_id.as_ref().map(|inner| inner.as_deref()),
    )
    .await?;
    Ok(CollectionView::from(row))
}

pub async fn delete_collection(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CollectionId) -> Result<()> {
    catalogue::delete_collection(tx, ctx, id).await
}

// ---------------------------------------------------------------------------
// Types and tags
// ---------------------------------------------------------------------------

pub async fn list_types(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListQuery,
) -> Result<Page<TypeView>> {
    Ok(map_page(catalogue::types(tx, ctx, query.paging()?).await?))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateValue {
    pub value: String,
    pub external_id: Option<String>,
}

pub async fn create_type(tx: &mut Tx<'_>, ctx: &Ctx<'_>, body: CreateValue) -> Result<TypeView> {
    Ok(TypeView::from(
        catalogue::create_type(tx, ctx, &body.value, body.external_id.as_deref()).await?,
    ))
}

pub async fn delete_type(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductTypeId) -> Result<()> {
    catalogue::delete_type(tx, ctx, id).await
}

pub async fn list_tags(tx: &mut Tx<'_>, ctx: &Ctx<'_>, query: ListQuery) -> Result<Page<TagView>> {
    Ok(map_page(catalogue::tags(tx, ctx, query.paging()?).await?))
}

pub async fn create_tag(tx: &mut Tx<'_>, ctx: &Ctx<'_>, body: CreateValue) -> Result<TagView> {
    Ok(TagView::from(
        catalogue::create_tag(tx, ctx, &body.value, body.external_id.as_deref()).await?,
    ))
}

pub async fn delete_tag(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductTagId) -> Result<()> {
    catalogue::delete_tag(tx, ctx, id).await
}

// ---------------------------------------------------------------------------
// Categories
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListCategories {
    pub after: Option<String>,
    pub limit: Option<u32>,
    /// Left out lists the roots.
    pub parent_id: Option<CategoryId>,
}

pub async fn list_categories(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListCategories,
) -> Result<Page<CategoryView>> {
    let page = catalogue::categories(
        tx,
        ctx,
        query.parent_id,
        paging(query.after.as_deref(), query.limit)?,
    )
    .await?;
    Ok(map_page(page))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCategory {
    pub parent_id: Option<CategoryId>,
    pub name: String,
    pub handle: String,
    pub description: Option<String>,
    pub rank: Option<i32>,
    pub is_active: Option<bool>,
    pub is_internal: Option<bool>,
    pub external_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn create_category(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreateCategory,
) -> Result<CategoryView> {
    let new = catalogue::NewCategory {
        parent_id: body.parent_id,
        name: body.name,
        handle: body.handle,
        description: body.description,
        rank: body.rank,
        is_active: body.is_active,
        is_internal: body.is_internal,
        external_id: body.external_id,
        metadata: body.metadata,
    };
    Ok(CategoryView::from(
        catalogue::create_category(tx, ctx, new).await?,
    ))
}

pub async fn get_category(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CategoryId) -> Result<CategoryView> {
    Ok(CategoryView::from(catalogue::category(tx, ctx, id).await?))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateCategory {
    pub name: Option<String>,
    pub handle: Option<String>,
    pub description: Option<String>,
    pub rank: Option<i32>,
    pub is_active: Option<bool>,
    pub is_internal: Option<bool>,
    pub external_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn update_category(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CategoryId,
    body: UpdateCategory,
) -> Result<CategoryView> {
    let patch = catalogue::CategoryPatch {
        name: body.name,
        handle: body.handle,
        description: body.description,
        rank: body.rank,
        is_active: body.is_active,
        is_internal: body.is_internal,
        external_id: body.external_id,
        metadata: body.metadata,
    };
    Ok(CategoryView::from(
        catalogue::update_category(tx, ctx, id, patch).await?,
    ))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveCategory {
    /// `null` makes it a root.
    pub parent_id: Option<CategoryId>,
}

pub async fn move_category(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CategoryId,
    body: MoveCategory,
) -> Result<CategoryView> {
    Ok(CategoryView::from(
        catalogue::move_category(tx, ctx, id, body.parent_id).await?,
    ))
}

pub async fn category_subtree(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    root: CategoryId,
    query: ListQuery,
) -> Result<Page<CategoryView>> {
    Ok(map_page(
        catalogue::category_subtree(tx, ctx, root, query.paging()?).await?,
    ))
}

pub async fn delete_category(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CategoryId) -> Result<()> {
    catalogue::delete_category(tx, ctx, id).await
}

// ---------------------------------------------------------------------------
// Translations
// ---------------------------------------------------------------------------

pub async fn list_translations(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
) -> Result<Vec<TranslationView>> {
    let rows = catalogue::translations(tx, ctx, product_id).await?;
    Ok(rows.into_iter().map(TranslationView::from).collect())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PutTranslation {
    pub locale: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub handle: Option<String>,
}

pub async fn put_translation(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    body: PutTranslation,
) -> Result<TranslationView> {
    let translation = catalogue::ProductTranslation {
        product_id,
        locale: body.locale,
        title: body.title,
        subtitle: body.subtitle,
        description: body.description,
        handle: body.handle,
    };
    Ok(TranslationView::from(
        catalogue::put_translation(tx, ctx, product_id, translation).await?,
    ))
}

pub async fn remove_translation(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    locale: &str,
) -> Result<()> {
    catalogue::remove_translation(tx, ctx, product_id, locale).await
}

/// What the product reads as in one language, with the fallback said out loud
/// rather than hidden — an untranslated field is the thing a translator is
/// looking for.
pub async fn localised(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    locale: &str,
) -> Result<LocalisedView> {
    Ok(LocalisedView::from(
        catalogue::localised(tx, ctx, product_id, locale).await?,
    ))
}

// ---------------------------------------------------------------------------
// Category translations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryTranslationView {
    pub category_id: CategoryId,
    pub locale: String,
    pub name: String,
    pub description: Option<String>,
}

impl From<catalogue::CategoryTranslation> for CategoryTranslationView {
    fn from(row: catalogue::CategoryTranslation) -> Self {
        CategoryTranslationView {
            category_id: row.category_id,
            locale: row.locale,
            name: row.name,
            description: row.description,
        }
    }
}

pub async fn list_category_translations(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    category_id: CategoryId,
) -> Result<Vec<CategoryTranslationView>> {
    let rows = catalogue::category_translations(tx, ctx, category_id).await?;
    Ok(rows
        .into_iter()
        .map(CategoryTranslationView::from)
        .collect())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PutCategoryTranslation {
    pub locale: String,
    pub name: String,
    pub description: Option<String>,
}

pub async fn put_category_translation(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    category_id: CategoryId,
    body: PutCategoryTranslation,
) -> Result<CategoryTranslationView> {
    let translation = catalogue::CategoryTranslation {
        category_id,
        locale: body.locale,
        name: body.name,
        description: body.description,
    };
    Ok(CategoryTranslationView::from(
        catalogue::put_category_translation(tx, ctx, category_id, translation).await?,
    ))
}

pub async fn remove_category_translation(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    category_id: CategoryId,
    locale: &str,
) -> Result<()> {
    catalogue::remove_category_translation(tx, ctx, category_id, locale).await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalisedCategoryView {
    pub category_id: CategoryId,
    pub locale: Option<String>,
    pub name: String,
    pub description: String,
    pub is_fallback: bool,
}

impl From<catalogue::LocalisedCategory> for LocalisedCategoryView {
    fn from(row: catalogue::LocalisedCategory) -> Self {
        LocalisedCategoryView {
            category_id: row.category_id,
            locale: row.locale,
            name: row.name,
            description: row.description,
            is_fallback: row.is_fallback,
        }
    }
}

pub async fn localised_category(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    category_id: CategoryId,
    locale: &str,
) -> Result<LocalisedCategoryView> {
    Ok(LocalisedCategoryView::from(
        catalogue::localised_category(tx, ctx, category_id, locale).await?,
    ))
}

// ---------------------------------------------------------------------------
// Price sets and prices
// ---------------------------------------------------------------------------

pub async fn create_price_set(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<PriceSetView> {
    Ok(PriceSetView::from(
        pricing::create_price_set(tx, ctx).await?,
    ))
}

pub async fn get_price_set(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: PriceSetId) -> Result<PriceSetView> {
    Ok(PriceSetView::from(pricing::price_set(tx, ctx, id).await?))
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LinkPriceSet {
    pub price_set_id: PriceSetId,
}

pub async fn link_variant_price_set(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
    body: LinkPriceSet,
) -> Result<()> {
    pricing::link_variant(tx, ctx, variant_id, body.price_set_id).await
}

// ---------------------------------------------------------------------------
// Bundles
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetBundlePrice {
    /// `null` un-marks the variant as a bundle.
    pub mode: Option<String>,
    pub discount_percent: Option<Decimal>,
}

pub async fn set_bundle_price(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
    body: SetBundlePrice,
) -> Result<()> {
    let mode = body
        .mode
        .as_deref()
        .map(pricing::BundlePriceMode::parse)
        .transpose()?;
    pricing::set_bundle_price_mode(tx, ctx, variant_id, mode, body.discount_percent).await
}

#[derive(Debug, Clone, Serialize)]
pub struct BundleComponentView {
    pub id: BundleComponentId,
    pub bundle_variant_id: VariantId,
    pub component_variant_id: VariantId,
    pub quantity: i32,
    pub sort_order: i32,
}

impl From<pricing::BundleComponent> for BundleComponentView {
    fn from(row: pricing::BundleComponent) -> Self {
        BundleComponentView {
            id: row.id,
            bundle_variant_id: row.bundle_variant_id,
            component_variant_id: row.component_variant_id,
            quantity: row.quantity,
            sort_order: row.sort_order,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewBundleComponentInput {
    pub component_variant_id: VariantId,
    pub quantity: i32,
    #[serde(default)]
    pub sort_order: i32,
}

pub async fn add_bundle_component(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    bundle_variant_id: VariantId,
    body: NewBundleComponentInput,
) -> Result<BundleComponentView> {
    Ok(BundleComponentView::from(
        pricing::add_bundle_component(
            tx,
            ctx,
            bundle_variant_id,
            pricing::NewBundleComponent {
                component_variant_id: body.component_variant_id,
                quantity: body.quantity,
                sort_order: body.sort_order,
            },
        )
        .await?,
    ))
}

pub async fn remove_bundle_component(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    bundle_variant_id: VariantId,
    component_variant_id: VariantId,
) -> Result<()> {
    pricing::remove_bundle_component(tx, ctx, bundle_variant_id, component_variant_id).await
}

pub async fn list_bundle_components(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    bundle_variant_id: VariantId,
) -> Result<Vec<BundleComponentView>> {
    Ok(pricing::bundle_components(tx, ctx, bundle_variant_id)
        .await?
        .into_iter()
        .map(BundleComponentView::from)
        .collect())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundlePriceQuery {
    pub currency_code: String,
    #[serde(default = "one")]
    pub quantity: i32,
}

fn one() -> i32 {
    1
}

#[derive(Debug, Clone, Serialize)]
pub struct BundlePriceComponentView {
    pub component_variant_id: VariantId,
    pub quantity: i32,
    pub unit_price: Decimal,
    pub allocated_total: Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct BundlePriceView {
    pub mode: String,
    pub total: Decimal,
    pub currency_code: String,
    pub components: Vec<BundlePriceComponentView>,
}

/// What the bundle would be sold for right now, and the share of it each
/// component was allocated. A host prices a bundle line the same way it
/// prices an ordinary one: ask once, before adding it to a cart.
pub async fn bundle_price(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    bundle_variant_id: VariantId,
    query: BundlePriceQuery,
) -> Result<BundlePriceView> {
    let currency = Currency::parse(&query.currency_code)?;
    let at = pricing::PriceContext::new(currency, query.quantity.max(1));
    let price = pricing::resolve_bundle(tx, ctx, bundle_variant_id, &at)
        .await?
        .ok_or_else(|| Error::invalid("that variant is not a bundle"))?;

    Ok(BundlePriceView {
        mode: price.mode.as_str().to_string(),
        total: price.total.amount,
        currency_code: query.currency_code,
        components: price
            .components
            .into_iter()
            .map(|c| BundlePriceComponentView {
                component_variant_id: c.component_variant_id,
                quantity: c.quantity,
                unit_price: c.unit_price.amount,
                allocated_total: c.allocated_total.amount,
            })
            .collect(),
    })
}

pub async fn list_prices(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    set: PriceSetId,
    query: ListQuery,
) -> Result<Page<PriceView>> {
    Ok(map_page(
        pricing::prices(tx, ctx, set, query.paging()?).await?,
    ))
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PriceRuleInput {
    pub attribute: String,
    pub value: String,
    pub operator: String,
    pub priority: Option<i32>,
}

impl From<PriceRuleInput> for pricing::NewPriceRule {
    fn from(input: PriceRuleInput) -> Self {
        pricing::NewPriceRule {
            attribute: input.attribute,
            value: input.value,
            operator: input.operator,
            priority: input.priority.unwrap_or(0),
        }
    }
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddPrice {
    pub price_set_id: PriceSetId,
    pub price_list_id: Option<PriceListId>,
    pub title: Option<String>,
    pub amount: Decimal,
    /// ISO 4217, checked here rather than at the database.
    pub currency_code: String,
    pub min_quantity: Option<i32>,
    pub max_quantity: Option<i32>,
    #[serde(default)]
    pub rules: Vec<PriceRuleInput>,
}

pub async fn add_price(tx: &mut Tx<'_>, ctx: &Ctx<'_>, body: AddPrice) -> Result<PriceView> {
    let currency = Currency::parse(&body.currency_code)?;
    let new = pricing::NewPrice {
        price_set_id: body.price_set_id,
        price_list_id: body.price_list_id,
        title: body.title,
        amount: Money::new(body.amount, currency),
        min_quantity: body.min_quantity,
        max_quantity: body.max_quantity,
        rules: body
            .rules
            .into_iter()
            .map(pricing::NewPriceRule::from)
            .collect(),
    };
    Ok(PriceView::from(pricing::add_price(tx, ctx, new).await?))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatePrice {
    pub title: Option<String>,
    pub amount: Option<Decimal>,
    /// Required whenever `amount` is sent: an amount without a currency is not
    /// an amount.
    pub currency_code: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub min_quantity: Option<Option<i32>>,
    #[serde(default, deserialize_with = "double_option")]
    pub max_quantity: Option<Option<i32>>,
}

pub async fn update_price(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PriceId,
    body: UpdatePrice,
) -> Result<PriceView> {
    let amount = match (body.amount, body.currency_code.as_deref()) {
        (Some(amount), Some(code)) => Some(Money::new(amount, Currency::parse(code)?)),
        (Some(_), None) => return Err(Error::invalid("an amount needs a currency beside it")),
        (None, _) => None,
    };
    let change = pricing::PriceUpdate {
        title: body.title,
        amount,
        min_quantity: body.min_quantity,
        max_quantity: body.max_quantity,
    };
    Ok(PriceView::from(
        pricing::update_price(tx, ctx, id, change).await?,
    ))
}

pub async fn delete_price(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: PriceId) -> Result<()> {
    pricing::delete_price(tx, ctx, id).await
}

pub async fn list_price_rules(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    price: PriceId,
) -> Result<Vec<PriceRuleView>> {
    let rows = pricing::price_rules(tx, ctx, price).await?;
    Ok(rows.into_iter().map(PriceRuleView::from).collect())
}

pub async fn add_price_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    price: PriceId,
    body: PriceRuleInput,
) -> Result<PriceRuleView> {
    Ok(PriceRuleView::from(
        pricing::add_price_rule(tx, ctx, price, body.into()).await?,
    ))
}

pub async fn remove_price_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    price: PriceId,
    rule: uuid::Uuid,
) -> Result<()> {
    pricing::remove_price_rule(tx, ctx, price, rule).await
}

// ---------------------------------------------------------------------------
// Price lists
// ---------------------------------------------------------------------------

pub async fn list_price_lists(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListQuery,
) -> Result<Page<PriceListView>> {
    Ok(map_page(
        pricing::price_lists(tx, ctx, query.paging()?).await?,
    ))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePriceList {
    pub title: String,
    pub description: Option<String>,
    pub kind: String,
    pub status: String,
    pub starts_at: Option<chrono::DateTime<chrono::Utc>>,
    pub ends_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn create_price_list(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreatePriceList,
) -> Result<PriceListView> {
    let new = pricing::NewPriceList {
        title: body.title,
        description: body.description,
        kind: body.kind,
        status: body.status,
        starts_at: body.starts_at,
        ends_at: body.ends_at,
    };
    Ok(PriceListView::from(
        pricing::create_price_list(tx, ctx, new).await?,
    ))
}

pub async fn get_price_list(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PriceListId,
) -> Result<PriceListView> {
    Ok(PriceListView::from(pricing::price_list(tx, ctx, id).await?))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatePriceList {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub starts_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    #[serde(default, deserialize_with = "double_option")]
    pub ends_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
}

pub async fn update_price_list(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PriceListId,
    body: UpdatePriceList,
) -> Result<PriceListView> {
    let change = pricing::PriceListUpdate {
        title: body.title,
        description: body.description,
        status: body.status,
        starts_at: body.starts_at,
        ends_at: body.ends_at,
    };
    Ok(PriceListView::from(
        pricing::update_price_list(tx, ctx, id, change).await?,
    ))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddPriceListRule {
    pub attribute: String,
    pub allowed_values: Vec<String>,
}

pub async fn add_price_list_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PriceListId,
    body: AddPriceListRule,
) -> Result<PriceListRuleView> {
    Ok(PriceListRuleView::from(
        pricing::add_price_list_rule(tx, ctx, id, body.attribute, body.allowed_values).await?,
    ))
}

// ---------------------------------------------------------------------------
// Price preferences
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindPricePreference {
    pub attribute: String,
    pub value: Option<String>,
}

/// `None` rather than a 404: no preference set is an answer, and it is the
/// common one.
pub async fn get_price_preference(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: FindPricePreference,
) -> Result<Option<PricePreferenceView>> {
    let found =
        pricing::price_preference(tx, ctx, &query.attribute, query.value.as_deref()).await?;
    Ok(found.map(PricePreferenceView::from))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetPricePreference {
    pub attribute: String,
    pub value: Option<String>,
    pub is_tax_inclusive: bool,
}

pub async fn set_price_preference(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: SetPricePreference,
) -> Result<PricePreferenceView> {
    Ok(PricePreferenceView::from(
        pricing::set_price_preference(tx, ctx, body.attribute, body.value, body.is_tax_inclusive)
            .await?,
    ))
}

// ---------------------------------------------------------------------------
// Stock locations
// ---------------------------------------------------------------------------

pub async fn list_stock_locations(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListQuery,
) -> Result<Page<StockLocationView>> {
    Ok(map_page(
        inventory::stock_locations(tx, ctx, query.paging()?).await?,
    ))
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateStockLocation {
    pub name: String,
    pub address: Option<StockLocationAddressIn>,
}

/// Where a location is. The country is what makes it the origin of a supply,
/// so it is required and the rest of the address is not.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StockLocationAddressIn {
    pub address_1: String,
    pub address_2: Option<String>,
    pub company: Option<String>,
    pub city: Option<String>,
    pub country_code: String,
    pub province: Option<String>,
    pub postal_code: Option<String>,
    pub phone: Option<String>,
}

impl From<StockLocationAddressIn> for inventory::NewStockLocationAddress {
    fn from(body: StockLocationAddressIn) -> Self {
        inventory::NewStockLocationAddress {
            address_1: body.address_1,
            address_2: body.address_2,
            company: body.company,
            city: body.city,
            country_code: body.country_code,
            province: body.province,
            postal_code: body.postal_code,
            phone: body.phone,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockLocationAddressView {
    pub id: uuid::Uuid,
    pub address_1: String,
    pub address_2: Option<String>,
    pub company: Option<String>,
    pub city: Option<String>,
    pub country_code: String,
    pub province: Option<String>,
    pub postal_code: Option<String>,
    pub phone: Option<String>,
}

impl From<inventory::StockLocationAddress> for StockLocationAddressView {
    fn from(row: inventory::StockLocationAddress) -> Self {
        StockLocationAddressView {
            id: row.id,
            address_1: row.address_1,
            address_2: row.address_2,
            company: row.company,
            city: row.city,
            country_code: row.country_code,
            province: row.province,
            postal_code: row.postal_code,
            phone: row.phone,
        }
    }
}

pub async fn set_stock_location_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StockLocationId,
    body: StockLocationAddressIn,
) -> Result<StockLocationAddressView> {
    Ok(StockLocationAddressView::from(
        inventory::set_stock_location_address(tx, ctx, id, body.into()).await?,
    ))
}

pub async fn get_stock_location_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StockLocationId,
) -> Result<Option<StockLocationAddressView>> {
    Ok(inventory::stock_location_address(tx, ctx, id)
        .await?
        .map(StockLocationAddressView::from))
}

pub async fn create_stock_location(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreateStockLocation,
) -> Result<StockLocationView> {
    let new = inventory::NewStockLocation {
        name: body.name,
        address: body.address.map(Into::into),
    };
    Ok(StockLocationView::from(
        inventory::create_stock_location(tx, ctx, new).await?,
    ))
}

pub async fn get_stock_location(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StockLocationId,
) -> Result<StockLocationView> {
    Ok(StockLocationView::from(
        inventory::stock_location(tx, ctx, id).await?,
    ))
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenameStockLocation {
    pub name: String,
}

pub async fn rename_stock_location(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StockLocationId,
    body: RenameStockLocation,
) -> Result<StockLocationView> {
    Ok(StockLocationView::from(
        inventory::rename_stock_location(tx, ctx, id, &body.name).await?,
    ))
}

pub async fn delete_stock_location(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StockLocationId,
) -> Result<()> {
    inventory::delete_stock_location(tx, ctx, id).await
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkSalesChannel {
    pub sales_channel_id: SalesChannelId,
}

pub async fn link_sales_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StockLocationId,
    body: LinkSalesChannel,
) -> Result<()> {
    inventory::link_sales_channel(tx, ctx, id, body.sales_channel_id).await
}

pub async fn unlink_sales_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StockLocationId,
    sales_channel_id: SalesChannelId,
) -> Result<()> {
    inventory::unlink_sales_channel(tx, ctx, id, sales_channel_id).await
}

pub async fn list_locations_for_sales_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    sales_channel_id: SalesChannelId,
    query: ListQuery,
) -> Result<Page<StockLocationView>> {
    Ok(map_page(
        inventory::locations_for_sales_channel(tx, ctx, sales_channel_id, query.paging()?).await?,
    ))
}

// ---------------------------------------------------------------------------
// Inventory items and levels
// ---------------------------------------------------------------------------

pub async fn list_inventory_items(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListQuery,
) -> Result<Page<InventoryItemView>> {
    Ok(map_page(
        inventory::inventory_items(tx, ctx, query.paging()?).await?,
    ))
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateInventoryItem {
    pub sku: Option<String>,
    pub title: Option<String>,
    pub requires_shipping: bool,
}

pub async fn create_inventory_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreateInventoryItem,
) -> Result<InventoryItemView> {
    let new = inventory::NewInventoryItem {
        sku: body.sku,
        title: body.title,
        requires_shipping: body.requires_shipping,
    };
    Ok(InventoryItemView::from(
        inventory::create_inventory_item(tx, ctx, new).await?,
    ))
}

pub async fn get_inventory_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: InventoryItemId,
) -> Result<InventoryItemView> {
    Ok(InventoryItemView::from(
        inventory::inventory_item(tx, ctx, id).await?,
    ))
}

pub async fn delete_inventory_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: InventoryItemId,
) -> Result<()> {
    inventory::delete_inventory_item(tx, ctx, id).await
}

pub async fn list_levels(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: InventoryItemId,
    query: ListQuery,
) -> Result<Page<InventoryLevelView>> {
    Ok(map_page(
        inventory::levels_for_item(tx, ctx, id, query.paging()?).await?,
    ))
}

pub async fn get_level(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: InventoryItemId,
    location_id: StockLocationId,
) -> Result<InventoryLevelView> {
    Ok(InventoryLevelView::from(
        inventory::level(tx, ctx, id, location_id).await?,
    ))
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetStock {
    pub location_id: StockLocationId,
    pub stocked_quantity: i32,
    pub incoming_quantity: i32,
}

pub async fn set_stock(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: InventoryItemId,
    body: SetStock,
) -> Result<InventoryLevelView> {
    let level = inventory::set_stock(
        tx,
        ctx,
        id,
        body.location_id,
        body.stocked_quantity,
        body.incoming_quantity,
    )
    .await?;
    Ok(InventoryLevelView::from(level))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdjustStock {
    pub delta: i32,
    pub reason: Option<String>,
}

pub async fn adjust_stock(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: InventoryItemId,
    location_id: StockLocationId,
    body: AdjustStock,
) -> Result<InventoryLevelView> {
    let level =
        inventory::adjust_stock(tx, ctx, id, location_id, body.delta, body.reason.as_deref())
            .await?;
    Ok(InventoryLevelView::from(level))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferStock {
    pub from_location_id: StockLocationId,
    pub to_location_id: StockLocationId,
    pub quantity: i32,
    pub reason: Option<String>,
}

pub async fn transfer_stock(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: InventoryItemId,
    body: TransferStock,
) -> Result<StockTransferView> {
    let transfer = inventory::transfer_stock(
        tx,
        ctx,
        id,
        body.from_location_id,
        body.to_location_id,
        body.quantity,
        body.reason.as_deref(),
    )
    .await?;
    Ok(StockTransferView::from(transfer))
}

pub async fn get_stock_transfer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StockTransferId,
) -> Result<StockTransferView> {
    Ok(StockTransferView::from(
        inventory::stock_transfer(tx, ctx, id).await?,
    ))
}

pub async fn list_stock_transfers(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: InventoryItemId,
    query: ListQuery,
) -> Result<Page<StockTransferView>> {
    Ok(map_page(
        inventory::transfers_for_item(tx, ctx, id, query.paging()?).await?,
    ))
}

// ---------------------------------------------------------------------------
// Inventory items behind a variant
// ---------------------------------------------------------------------------

pub async fn list_variant_inventory_items(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
) -> Result<Vec<VariantInventoryItemView>> {
    let rows = inventory::inventory_items_for_variant(tx, ctx, variant_id).await?;
    Ok(rows
        .into_iter()
        .map(VariantInventoryItemView::from)
        .collect())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachInventoryItem {
    pub inventory_item_id: InventoryItemId,
    pub required_quantity: i32,
}

pub async fn attach_inventory_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
    body: AttachInventoryItem,
) -> Result<VariantInventoryItemView> {
    let row = inventory::attach_inventory_item(
        tx,
        ctx,
        variant_id,
        body.inventory_item_id,
        body.required_quantity,
    )
    .await?;
    Ok(VariantInventoryItemView::from(row))
}

pub async fn detach_inventory_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
    inventory_item_id: InventoryItemId,
) -> Result<()> {
    inventory::detach_inventory_item(tx, ctx, variant_id, inventory_item_id).await
}

// ---------------------------------------------------------------------------
// Reservations
// ---------------------------------------------------------------------------

pub async fn list_reservations(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListQuery,
) -> Result<Page<ReservationView>> {
    Ok(map_page(
        inventory::reservations(tx, ctx, query.paging()?).await?,
    ))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateReservation {
    pub inventory_item_id: InventoryItemId,
    pub location_id: StockLocationId,
    pub quantity: i32,
    pub line_item_id: Option<LineItemId>,
    #[serde(default)]
    pub allows_backorder: bool,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn create_reservation(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreateReservation,
) -> Result<ReservationView> {
    let made = inventory::reserve(
        tx,
        ctx,
        body.inventory_item_id,
        body.location_id,
        body.quantity,
        body.line_item_id,
        body.allows_backorder,
        body.expires_at,
    )
    .await?;
    Ok(ReservationView::from(made))
}

pub async fn release_reservation(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ReservationId) -> Result<()> {
    inventory::release(tx, ctx, id).await
}

/// Turns a reservation into stock that has left, rather than stock that is
/// spoken for.
pub async fn fulfil_reservation(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ReservationId) -> Result<()> {
    inventory::fulfil(tx, ctx, id).await
}

// ---------------------------------------------------------------------------
// Many rows at once
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportRow {
    pub handle: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub status: Option<catalogue::ProductStatus>,
    pub variant_title: Option<String>,
    pub sku: Option<String>,
    pub price_amount: Option<Decimal>,
    pub price_currency: Option<String>,
}

impl From<ImportRow> for batch::ProductRow {
    fn from(row: ImportRow) -> Self {
        batch::ProductRow {
            handle: row.handle,
            title: row.title,
            subtitle: row.subtitle,
            description: row.description,
            status: row.status,
            variant_title: row.variant_title,
            sku: row.sku,
            price_amount: row.price_amount,
            price_currency: row.price_currency,
        }
    }
}

/// The rows themselves, never a URL to fetch them from: where the file lives is
/// the host's business, and tezgah does not read one.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportProductsBody {
    pub rows: Vec<ImportRow>,
    #[serde(default)]
    pub delete: Vec<ProductId>,
}

impl From<ImportProductsBody> for batch::ImportProducts {
    fn from(body: ImportProductsBody) -> Self {
        batch::ImportProducts {
            rows: body.rows.into_iter().map(batch::ProductRow::from).collect(),
            delete: body.delete,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectionView {
    pub row: usize,
    pub reason: String,
}

impl From<batch::Rejection> for RejectionView {
    fn from(one: batch::Rejection) -> Self {
        RejectionView {
            row: one.row,
            reason: one.reason,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResultView {
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
    pub rejected: Vec<RejectionView>,
}

impl From<batch::ImportResult> for ImportResultView {
    fn from(result: batch::ImportResult) -> Self {
        ImportResultView {
            created: result.created,
            updated: result.updated,
            deleted: result.deleted,
            rejected: result
                .rejected
                .into_iter()
                .map(RejectionView::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResultView {
    pub applied: usize,
    pub rejected: Vec<RejectionView>,
}

impl From<batch::BatchResult> for BatchResultView {
    fn from(result: batch::BatchResult) -> Self {
        BatchResultView {
            applied: result.applied,
            rejected: result
                .rejected
                .into_iter()
                .map(RejectionView::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductExportView {
    pub product_id: ProductId,
    pub handle: String,
    pub product_title: String,
    pub status: catalogue::ProductStatus,
    pub variant_id: VariantId,
    pub variant_title: String,
    pub sku: Option<String>,
    pub price_amount: Option<Decimal>,
    pub price_currency: Option<String>,
}

impl From<batch::ProductExport> for ProductExportView {
    fn from(row: batch::ProductExport) -> Self {
        ProductExportView {
            product_id: row.product_id,
            handle: row.handle,
            product_title: row.product_title,
            status: row.status,
            variant_id: row.variant_id,
            variant_title: row.variant_title,
            sku: row.sku,
            price_amount: row.price_amount,
            price_currency: row.price_currency,
        }
    }
}

pub async fn import_products(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: ImportProductsBody,
) -> Result<ImportResultView> {
    let result = batch::import_products(tx, ctx, body.into()).await?;
    Ok(ImportResultView::from(result))
}

/// The same engine as the import, with the deletions Medusa's `products/batch`
/// carries beside its writes.
pub async fn batch_products(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: ImportProductsBody,
) -> Result<ImportResultView> {
    import_products(tx, ctx, body).await
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportQuery {
    pub after: Option<String>,
    pub limit: Option<u32>,
    pub currency_code: Option<String>,
}

/// A page of rows, not a file. Rendering them into CSV and putting that
/// somewhere is the host's, which is where its media already lives.
pub async fn export_products(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ExportQuery,
) -> Result<Page<ProductExportView>> {
    let currency = match query.currency_code.as_deref() {
        Some(code) => Some(Currency::parse(code)?),
        None => None,
    };
    let page = batch::export_products(
        tx,
        ctx,
        currency,
        paging(query.after.as_deref(), query.limit)?,
    )
    .await?;
    Ok(map_page(page))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriceChangeRow {
    pub id: PriceId,
    pub amount: Decimal,
    pub currency_code: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatePricesBody {
    pub prices: Vec<PriceChangeRow>,
}

pub async fn batch_prices(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: UpdatePricesBody,
) -> Result<BatchResultView> {
    let mut changes = Vec::with_capacity(body.prices.len());
    for row in body.prices {
        changes.push(batch::PriceChange {
            price_id: row.id,
            amount: Money::new(row.amount, Currency::parse(&row.currency_code)?),
        });
    }
    Ok(BatchResultView::from(
        batch::update_prices(tx, ctx, changes).await?,
    ))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockLevelRowBody {
    pub inventory_item_id: InventoryItemId,
    pub location_id: StockLocationId,
    pub stocked_quantity: i32,
    pub incoming_quantity: Option<i32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetStockLevelsBody {
    pub levels: Vec<StockLevelRowBody>,
}

pub async fn batch_stock_levels(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: SetStockLevelsBody,
) -> Result<BatchResultView> {
    let rows = body
        .levels
        .into_iter()
        .map(|row| batch::StockLevelRow {
            inventory_item_id: row.inventory_item_id,
            location_id: row.location_id,
            stocked_quantity: row.stocked_quantity,
            incoming_quantity: row.incoming_quantity,
        })
        .collect();
    Ok(BatchResultView::from(
        batch::set_stock_levels(tx, ctx, rows).await?,
    ))
}

// ---------------------------------------------------------------------------
// The table
// ---------------------------------------------------------------------------

const CATALOGUE: &str = "catalogue";
const PRICING: &str = "pricing";
const INVENTORY: &str = "inventory";

pub(super) static ROUTES: &[Route] = &[
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/products",
        action: Action::View,
        domain: CATALOGUE,
        summary: "List products, drafts and archives included",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/products",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Create a product",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/products/import",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Create or update many products from flat rows",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/products/batch",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Write and delete many products in one call",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/products/export",
        action: Action::View,
        domain: CATALOGUE,
        summary: "A page of variants, flat enough to write as CSV",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/products/{id}",
        action: Action::View,
        domain: CATALOGUE,
        summary: "Fetch one product whatever its status",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/products/{id}",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Change a product",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/products/{id}",
        action: Action::Delete,
        domain: CATALOGUE,
        summary: "Delete a product",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/products/{id}/publish",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Put a product on sale",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/products/{id}/archive",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Take a product off sale without losing it",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/products/{id}/submit",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Submit a draft or a rejected product for review",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/products/{id}/approve",
        action: Action::Moderate,
        domain: CATALOGUE,
        summary: "Approve a proposed product and publish it",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/products/{id}/reject",
        action: Action::Moderate,
        domain: CATALOGUE,
        summary: "Reject a proposed product, recording why",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/products/{id}/images",
        action: Action::View,
        domain: CATALOGUE,
        summary: "List a product's images",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/products/{id}/images",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Add an image to a product",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/products/{id}/images/{image_id}",
        action: Action::Delete,
        domain: CATALOGUE,
        summary: "Remove an image from a product",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/products/{id}/tags",
        action: Action::View,
        domain: CATALOGUE,
        summary: "List a product's tags",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/products/{id}/tags",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Tag a product",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/products/{id}/tags/{tag_id}",
        action: Action::Delete,
        domain: CATALOGUE,
        summary: "Take a tag off a product",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/products/{id}/categories",
        action: Action::View,
        domain: CATALOGUE,
        summary: "List the categories a product is in",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/products/{id}/categories",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Put a product in a category",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/products/{id}/categories/{category_id}",
        action: Action::Delete,
        domain: CATALOGUE,
        summary: "Take a product out of a category",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/products/{id}/channels",
        action: Action::View,
        domain: CATALOGUE,
        summary: "List the sales channels a product is linked to",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/products/{id}/channels",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Link a product to a sales channel",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/products/{id}/channels/{sales_channel_id}",
        action: Action::Delete,
        domain: CATALOGUE,
        summary: "Unlink a product from a sales channel",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/products/{id}/options",
        action: Action::View,
        domain: CATALOGUE,
        summary: "List a product's options and their values",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/products/{id}/options",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Add an option to a product",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/product-options/{id}/values",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Add a value to an option",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/products/{id}/variants",
        action: Action::View,
        domain: CATALOGUE,
        summary: "List a product's variants",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/products/{id}/variants",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Create a variant of a product",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/products/{id}/variants/generate",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Make every combination of the options that is not excluded",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/product-variants/{id}",
        action: Action::View,
        domain: CATALOGUE,
        summary: "Fetch one variant",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/product-variants/{id}",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Change a variant",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/product-variants/{id}",
        action: Action::Delete,
        domain: CATALOGUE,
        summary: "Delete a variant",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/product-variants/{id}/options",
        action: Action::View,
        domain: CATALOGUE,
        summary: "The option values one variant stands for",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/product-variants/{id}/options",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Set which option values a variant stands for",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/product-variants/{id}/images",
        action: Action::View,
        domain: CATALOGUE,
        summary: "List a variant's own attached images",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/product-variants/{id}/images",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Attach one of the product's images to a variant",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/product-variants/{id}/images/{image_id}",
        action: Action::Delete,
        domain: CATALOGUE,
        summary: "Detach an image from a variant",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/product-categories",
        action: Action::View,
        domain: CATALOGUE,
        summary: "List categories under a parent, or the roots",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/product-categories",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Create a category",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/product-categories/{id}",
        action: Action::View,
        domain: CATALOGUE,
        summary: "Fetch one category",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/product-categories/{id}",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Change a category",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/product-categories/{id}",
        action: Action::Delete,
        domain: CATALOGUE,
        summary: "Delete a category",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/product-categories/{id}/move",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Move a category, and its subtree with it",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/product-categories/{id}/subtree",
        action: Action::View,
        domain: CATALOGUE,
        summary: "List a category and everything under it",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/product-tags",
        action: Action::View,
        domain: CATALOGUE,
        summary: "List tags",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/product-tags",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Create a tag",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/product-tags/{id}",
        action: Action::Delete,
        domain: CATALOGUE,
        summary: "Delete a tag",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/product-types",
        action: Action::View,
        domain: CATALOGUE,
        summary: "List product types",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/product-types",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Create a product type",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/product-types/{id}",
        action: Action::Delete,
        domain: CATALOGUE,
        summary: "Delete a product type",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/collections",
        action: Action::View,
        domain: CATALOGUE,
        summary: "List collections",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/collections",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Create a collection",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/collections/{id}",
        action: Action::View,
        domain: CATALOGUE,
        summary: "Fetch one collection",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/collections/{id}",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Change a collection",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/collections/{id}",
        action: Action::Delete,
        domain: CATALOGUE,
        summary: "Delete a collection",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/products/{id}/translations",
        action: Action::View,
        domain: CATALOGUE,
        summary: "List a product's translations",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/products/{id}/translations",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Write a product's translation into one locale",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/products/{id}/translations/{locale}",
        action: Action::View,
        domain: CATALOGUE,
        summary: "Read a product in one locale, saying where it fell back",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/products/{id}/translations/{locale}",
        action: Action::Delete,
        domain: CATALOGUE,
        summary: "Drop a product's translation for one locale",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/product-categories/{id}/translations",
        action: Action::View,
        domain: CATALOGUE,
        summary: "List a category's translations",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/product-categories/{id}/translations",
        action: Action::Write,
        domain: CATALOGUE,
        summary: "Write a category's translation into one locale",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/product-categories/{id}/translations/{locale}",
        action: Action::View,
        domain: CATALOGUE,
        summary: "Read a category in one locale, saying where it fell back",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/product-categories/{id}/translations/{locale}",
        action: Action::Delete,
        domain: CATALOGUE,
        summary: "Drop a category's translation for one locale",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/price-sets",
        action: Action::Write,
        domain: PRICING,
        summary: "Create a price set",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/price-sets/{id}",
        action: Action::View,
        domain: PRICING,
        summary: "Fetch one price set",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/price-sets/{id}/prices",
        action: Action::View,
        domain: PRICING,
        summary: "List the prices in a price set",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/product-variants/{id}/price-set",
        action: Action::Write,
        domain: PRICING,
        summary: "Point a variant at a price set",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/product-variants/{id}/bundle",
        action: Action::Write,
        domain: PRICING,
        summary: "Mark a variant as a bundle, and how its price is decided",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/product-variants/{id}/bundle/components",
        action: Action::View,
        domain: PRICING,
        summary: "List what a bundle is made of",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/product-variants/{id}/bundle/components",
        action: Action::Write,
        domain: PRICING,
        summary: "Add a component to a bundle",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/product-variants/{id}/bundle/components/{component_variant_id}",
        action: Action::Delete,
        domain: PRICING,
        summary: "Take a component out of a bundle",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/product-variants/{id}/bundle/price",
        action: Action::View,
        domain: PRICING,
        summary: "What the bundle prices to now, allocated across its components",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/prices",
        action: Action::Write,
        domain: PRICING,
        summary: "Add a price",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/prices/batch",
        action: Action::Write,
        domain: PRICING,
        summary: "Move many prices at once, one currency to a call",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/prices/{id}",
        action: Action::Write,
        domain: PRICING,
        summary: "Change a price",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/prices/{id}",
        action: Action::Delete,
        domain: PRICING,
        summary: "Delete a price",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/prices/{id}/rules",
        action: Action::View,
        domain: PRICING,
        summary: "List the rules that decide when a price applies",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/prices/{id}/rules",
        action: Action::Write,
        domain: PRICING,
        summary: "Add a rule to a price",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/prices/{id}/rules/{rule_id}",
        action: Action::Delete,
        domain: PRICING,
        summary: "Take a rule off a price",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/price-lists",
        action: Action::View,
        domain: PRICING,
        summary: "List price lists",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/price-lists",
        action: Action::Write,
        domain: PRICING,
        summary: "Create a price list",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/price-lists/{id}",
        action: Action::View,
        domain: PRICING,
        summary: "Fetch one price list",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/price-lists/{id}",
        action: Action::Write,
        domain: PRICING,
        summary: "Change a price list",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/price-lists/{id}/rules",
        action: Action::Write,
        domain: PRICING,
        summary: "Add a rule to a price list",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/price-preferences",
        action: Action::View,
        domain: PRICING,
        summary: "Whether an attribute's prices are quoted tax inclusive",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/price-preferences",
        action: Action::Write,
        domain: PRICING,
        summary: "Say whether an attribute's prices are quoted tax inclusive",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/stock-locations",
        action: Action::View,
        domain: INVENTORY,
        summary: "List stock locations",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/stock-locations",
        action: Action::Write,
        domain: INVENTORY,
        summary: "Create a stock location",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/stock-locations/{id}",
        action: Action::View,
        domain: INVENTORY,
        summary: "Fetch one stock location",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/stock-locations/{id}",
        action: Action::Write,
        domain: INVENTORY,
        summary: "Rename a stock location",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/stock-locations/{id}",
        action: Action::Delete,
        domain: INVENTORY,
        summary: "Delete a stock location",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/stock-locations/{id}/address",
        action: Action::View,
        domain: INVENTORY,
        summary: "Read where a stock location is",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/stock-locations/{id}/address",
        action: Action::Write,
        domain: INVENTORY,
        summary: "Say where a stock location is",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/stock-locations/{id}/sales-channels",
        action: Action::Write,
        domain: INVENTORY,
        summary: "Let a sales channel ship from a location",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/stock-locations/{id}/sales-channels/{sales_channel_id}",
        action: Action::Delete,
        domain: INVENTORY,
        summary: "Stop a sales channel shipping from a location",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/sales-channels/{id}/stock-locations",
        action: Action::View,
        domain: INVENTORY,
        summary: "List the locations that ship for a sales channel",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/inventory-items",
        action: Action::View,
        domain: INVENTORY,
        summary: "List inventory items",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/inventory-items",
        action: Action::Write,
        domain: INVENTORY,
        summary: "Create an inventory item",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/inventory-items/batch",
        action: Action::Write,
        domain: INVENTORY,
        summary: "Set the counted stock of many items at many locations",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/inventory-items/{id}",
        action: Action::View,
        domain: INVENTORY,
        summary: "Fetch one inventory item",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/inventory-items/{id}",
        action: Action::Delete,
        domain: INVENTORY,
        summary: "Delete an inventory item",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/inventory-items/{id}/location-levels",
        action: Action::View,
        domain: INVENTORY,
        summary: "What is where, for one inventory item",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/inventory-items/{id}/location-levels",
        action: Action::Write,
        domain: INVENTORY,
        summary: "Set the counted stock at one location",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/inventory-items/{id}/location-levels/{location_id}",
        action: Action::View,
        domain: INVENTORY,
        summary: "The stock of one item at one location",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/inventory-items/{id}/location-levels/{location_id}/adjust",
        action: Action::Write,
        domain: INVENTORY,
        summary: "Move the stock at one location by a delta",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/inventory-items/{id}/transfers",
        action: Action::Write,
        domain: INVENTORY,
        summary: "Move stock from one location to another in one act",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/inventory-items/{id}/transfers",
        action: Action::View,
        domain: INVENTORY,
        summary: "List an item's transfers between locations",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/stock-transfers/{id}",
        action: Action::View,
        domain: INVENTORY,
        summary: "Fetch one transfer",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/product-variants/{id}/inventory-items",
        action: Action::View,
        domain: INVENTORY,
        summary: "What a variant is made of, and how much of each",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/product-variants/{id}/inventory-items",
        action: Action::Write,
        domain: INVENTORY,
        summary: "Put an inventory item behind a variant",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/product-variants/{id}/inventory-items/{inventory_item_id}",
        action: Action::Delete,
        domain: INVENTORY,
        summary: "Take an inventory item out from behind a variant",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/reservations",
        action: Action::View,
        domain: INVENTORY,
        summary: "List reservations",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/reservations",
        action: Action::Write,
        domain: INVENTORY,
        summary: "Hold stock without taking it out",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/reservations/{id}",
        action: Action::Delete,
        domain: INVENTORY,
        summary: "Let a held quantity go",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/reservations/{id}/fulfil",
        action: Action::Write,
        domain: INVENTORY,
        summary: "Turn a hold into stock that has left",
    },
];

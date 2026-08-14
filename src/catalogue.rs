//! The catalogue: what is for sale, how it varies, and how it is filed.
//!
//! Shaped like [`crate::store`]: the transaction first, the context second, a
//! [`Permit`] before any row is touched, and every scoped query naming its
//! scope as well as trusting the policy.
//!
//! A product's own columns are the shop's own language. Every other language is
//! a row in `product_translation`, and reading one that is not there falls back
//! rather than failing — a half-translated catalogue still has to sell.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::id::{
    CategoryId, CollectionId, OptionId, OptionValueId, ProductId, ProductImageId, ProductTagId,
    ProductTypeId, VariantId,
};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, AuditEntry, Ctx, Event, Permit, Resource, Tx};

/// Most options one product may be generated from, and most variants one
/// generation may make. Both are refusals rather than clamps: a caller asking
/// for more has made a mistake somewhere further up.
pub const MAX_OPTIONS: usize = 20;
pub const MAX_COMBINATIONS: usize = 1_000;

/// Most rows returned by the reads that belong to one product — its images, its
/// options, its tags. They are bounded by the product rather than by the shop,
/// so they are not paged.
const MAX_ATTACHED: i64 = 200;

macro_rules! product_columns {
    () => {
        "id, handle, title, subtitle, description, status, thumbnail_url, is_discountable, \
         product_type_id, product_collection_id, weight, length, height, width, material, \
         hs_code, origin_country, external_id, metadata, created_at, updated_at"
    };
}

macro_rules! variant_columns {
    () => {
        "id, product_id, title, sku, barcode, ean, upc, weight, length, height, width, material, \
         hs_code, origin_country, mid_code, manages_inventory, allows_backorder, rank, \
         withdrawal_exclusion_reason, is_giftcard, metadata, created_at, updated_at"
    };
}

macro_rules! category_columns {
    () => {
        "id, parent_id, mpath, name, handle, description, rank, is_active, is_internal, metadata, \
         created_at, updated_at"
    };
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// Where a product is in its life. `draft` is invisible, `published` is for
/// sale, `archived` is kept for the orders that already name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProductStatus {
    Draft,
    Published,
    Archived,
}

impl ProductStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            ProductStatus::Draft => "draft",
            ProductStatus::Published => "published",
            ProductStatus::Archived => "archived",
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "draft" => Ok(ProductStatus::Draft),
            "published" => Ok(ProductStatus::Published),
            "archived" => Ok(ProductStatus::Archived),
            other => Err(Error::invalid(format!("{other:?} is not a product status"))),
        }
    }
}

impl std::fmt::Display for ProductStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl sqlx::Type<sqlx::Postgres> for ProductStatus {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        <String as sqlx::Type<sqlx::Postgres>>::type_info()
    }

    fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
        <String as sqlx::Type<sqlx::Postgres>>::compatible(ty)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for ProductStatus {
    fn decode(
        value: sqlx::postgres::PgValueRef<'r>,
    ) -> std::result::Result<Self, sqlx::error::BoxDynError> {
        let text = <&str as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        ProductStatus::parse(text).map_err(|_| format!("{text:?} is not a product status").into())
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for ProductStatus {
    fn encode_by_ref(
        &self,
        buf: &mut sqlx::postgres::PgArgumentBuffer,
    ) -> std::result::Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        let text = self.as_str();
        <&str as sqlx::Encode<sqlx::Postgres>>::encode_by_ref(&text, buf)
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Product {
    pub id: ProductId,
    pub handle: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub status: ProductStatus,
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

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ProductVariant {
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
    /// Why buying this is outside the right of withdrawal. `None` is the
    /// ordinary case: it may be sent back.
    pub withdrawal_exclusion_reason: Option<String>,
    /// Selling this is selling money, not goods: the line carries no tax and a
    /// card is printed when the money is taken.
    pub is_giftcard: bool,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ProductOption {
    pub id: OptionId,
    pub product_id: ProductId,
    pub title: String,
    pub rank: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ProductOptionValue {
    pub id: OptionValueId,
    pub option_id: OptionId,
    pub value: String,
    pub rank: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// One option with everything it can be set to, which is what a variant grid is
/// drawn from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionWithValues {
    pub option: ProductOption,
    pub values: Vec<ProductOptionValue>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ProductCollection {
    pub id: CollectionId,
    pub handle: String,
    pub title: String,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ProductCategory {
    pub id: CategoryId,
    pub parent_id: Option<CategoryId>,
    /// The dot-joined ids from the root down to and including this one.
    pub mpath: String,
    pub name: String,
    pub handle: String,
    pub description: String,
    pub rank: i32,
    pub is_active: bool,
    pub is_internal: bool,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl ProductCategory {
    /// How deep this category sits, the root being zero.
    pub fn depth(&self) -> usize {
        self.mpath.split('.').count().saturating_sub(1)
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ProductTag {
    pub id: ProductTagId,
    pub value: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ProductType {
    pub id: ProductTypeId,
    pub value: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ProductImage {
    pub id: ProductImageId,
    pub product_id: ProductId,
    pub url: String,
    pub alt_text: Option<String>,
    pub rank: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ProductTranslation {
    pub product_id: ProductId,
    pub locale: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub handle: Option<String>,
}

/// A product read in one language, whatever was actually found.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Localised {
    pub product_id: ProductId,
    /// The locale the text below is really in, which is not always the one
    /// asked for.
    pub locale: Option<String>,
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub handle: String,
    /// True when nothing was translated and the product's own columns answered.
    pub is_fallback: bool,
}

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct NewProduct {
    pub handle: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub status: Option<ProductStatus>,
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

/// What a patch does not name, it leaves alone. A nullable text column is
/// cleared by naming it with an empty string.
#[derive(Debug, Clone, Default)]
pub struct ProductPatch {
    pub handle: Option<String>,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub thumbnail_url: Option<String>,
    pub is_discountable: Option<bool>,
    /// `Some(None)` unfiles the product, `None` leaves it where it is.
    pub product_type_id: Option<Option<ProductTypeId>>,
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

#[derive(Debug, Clone, Default)]
pub struct ProductFilter {
    pub status: Option<ProductStatus>,
    pub collection: Option<CollectionId>,
    pub product_type: Option<ProductTypeId>,
    /// Matches the category and everything under it.
    pub category: Option<CategoryId>,
    pub tag: Option<ProductTagId>,
}

#[derive(Debug, Clone, Default)]
pub struct NewVariant {
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
    pub withdrawal_exclusion: Option<crate::order::WithdrawalExclusion>,
    pub is_giftcard: Option<bool>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct VariantPatch {
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
    /// `Some(None)` puts the variant back inside the withdrawal right; `None`
    /// leaves whatever it says now.
    pub withdrawal_exclusion: Option<Option<crate::order::WithdrawalExclusion>>,
    pub is_giftcard: Option<bool>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct NewCategory {
    pub parent_id: Option<CategoryId>,
    pub name: String,
    pub handle: String,
    pub description: Option<String>,
    pub rank: Option<i32>,
    pub is_active: Option<bool>,
    pub is_internal: Option<bool>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct CategoryPatch {
    pub name: Option<String>,
    pub handle: Option<String>,
    pub description: Option<String>,
    pub rank: Option<i32>,
    pub is_active: Option<bool>,
    pub is_internal: Option<bool>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct NewImage {
    pub url: String,
    pub alt_text: Option<String>,
    pub rank: Option<i32>,
}

/// One variant's worth of choices: exactly one value per option the product
/// has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combination(pub Vec<OptionValueId>);

impl Combination {
    fn key(&self) -> Vec<Uuid> {
        let mut ids: Vec<Uuid> = self.0.iter().map(|id| id.as_uuid()).collect();
        ids.sort_unstable();
        ids
    }
}

/// What a generation should and should not make.
#[derive(Debug, Clone, Default)]
pub struct VariantPlan {
    /// Combinations the shop does not sell — the green one in size 44 that was
    /// never made.
    pub exclude: Vec<Combination>,
    pub manages_inventory: Option<bool>,
    pub allows_backorder: Option<bool>,
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn required(field: &'static str, value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::invalid(format!("a {field} is needed")));
    }
    Ok(trimmed.to_owned())
}

fn handle(value: &str) -> Result<String> {
    let trimmed = required("handle", value)?;
    if trimmed.chars().any(char::is_whitespace) {
        return Err(Error::invalid("a handle has no spaces in it"));
    }
    Ok(trimmed.to_lowercase())
}

/// Empty means cleared, so a caller can unset a nullable column with the same
/// field it sets one with.
fn nullable(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

/// The same shape the database's own check constraint admits.
fn locale(value: &str) -> Result<String> {
    let trimmed = value.trim();
    let mut parts = trimmed.split('-');
    let language = parts.next().unwrap_or_default();
    let language_ok =
        (2..=3).contains(&language.len()) && language.chars().all(|c| c.is_ascii_lowercase());
    let rest_ok = parts.all(|part| {
        (2..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric())
    });

    if language_ok && rest_ok {
        Ok(trimmed.to_owned())
    } else {
        Err(Error::invalid(format!("{trimmed:?} is not a locale")))
    }
}

/// A guarded update returns nothing whether the row is missing or the value it
/// was handed already belongs to a sibling; only a second read tells them apart.
async fn refusal(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    table: &'static str,
    entity: &'static str,
    id: Uuid,
    what: &'static str,
) -> Error {
    let found: std::result::Result<Option<Uuid>, sqlx::Error> = sqlx::query_scalar(&format!(
        "select id from {table} where scope = $1 and id = $2 and deleted_at is null"
    ))
    .bind(ctx.scope.0)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await;

    match found {
        Ok(Some(_)) => Error::conflict(what),
        Ok(None) => Error::not_found(entity),
        Err(err) => Error::from(err),
    }
}

async fn note(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    action: Action,
    entity: &'static str,
    entity_id: Uuid,
    summary: serde_json::Value,
) -> Result<()> {
    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action,
            entity,
            entity_id,
            summary,
        },
    )
    .await
}

fn metadata_or_empty(given: Option<serde_json::Value>) -> serde_json::Value {
    given.unwrap_or_else(|| serde_json::json!({}))
}

// ---------------------------------------------------------------------------
// Product
// ---------------------------------------------------------------------------

pub async fn create_product(tx: &mut Tx<'_>, ctx: &Ctx<'_>, new: NewProduct) -> Result<Product> {
    let _: Permit = ctx.permit(Action::Write, Resource::Product { id: None })?;

    let title = required("title", &new.title)?;
    let handle = handle(&new.handle)?;
    let id = ProductId::new();

    let product = sqlx::query_as::<_, Product>(concat!(
        "insert into product (id, scope, handle, title, subtitle, description, status, \
         thumbnail_url, is_discountable, product_type_id, product_collection_id, weight, length, \
         height, width, material, hs_code, origin_country, external_id, metadata)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, \
         $19, $20)
         on conflict do nothing
         returning ",
        product_columns!()
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(&handle)
    .bind(&title)
    .bind(nullable(new.subtitle))
    .bind(nullable(new.description))
    .bind(new.status.unwrap_or(ProductStatus::Draft))
    .bind(nullable(new.thumbnail_url))
    .bind(new.is_discountable.unwrap_or(true))
    .bind(new.product_type_id.map(ProductTypeId::as_uuid))
    .bind(new.product_collection_id.map(CollectionId::as_uuid))
    .bind(new.weight)
    .bind(new.length)
    .bind(new.height)
    .bind(new.width)
    .bind(nullable(new.material))
    .bind(nullable(new.hs_code))
    .bind(nullable(new.origin_country))
    .bind(nullable(new.external_id))
    .bind(metadata_or_empty(new.metadata))
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that handle is already a product here"))?;

    note(
        tx,
        ctx,
        Action::Write,
        "product",
        id.as_uuid(),
        serde_json::json!({ "handle": product.handle, "title": product.title }),
    )
    .await?;

    Ok(product)
}

pub async fn product(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductId) -> Result<Product> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Product {
            id: Some(id.as_uuid()),
        },
    )?;

    sqlx::query_as::<_, Product>(concat!(
        "select ",
        product_columns!(),
        " from product where scope = $1 and id = $2 and deleted_at is null"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("product"))
}

pub async fn product_by_handle(tx: &mut Tx<'_>, ctx: &Ctx<'_>, wanted: &str) -> Result<Product> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    sqlx::query_as::<_, Product>(concat!(
        "select ",
        product_columns!(),
        " from product where scope = $1 and handle = $2 and deleted_at is null"
    ))
    .bind(ctx.scope.0)
    .bind(handle(wanted)?)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("product"))
}

pub async fn products(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    filter: ProductFilter,
    paging: Paging,
) -> Result<Page<Product>> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    let rows = sqlx::query_as::<_, Product>(concat!(
        "select ",
        product_columns!(),
        " from product p
         where p.scope = $1
           and p.deleted_at is null
           and ($2::text is null or p.status = $2)
           and ($3::uuid is null or p.product_collection_id = $3)
           and ($4::uuid is null or p.product_type_id = $4)
           and ($5::uuid is null or exists (
                 select 1
                 from product_category_link l
                 join product_category c on c.id = l.category_id and c.scope = p.scope
                 where l.scope = p.scope
                   and l.product_id = p.id
                   and c.mpath like (
                     select root.mpath || '%'
                     from product_category root
                     where root.scope = p.scope and root.id = $5
                   )
               ))
           and ($6::uuid is null or exists (
                 select 1 from product_tag_link t
                 where t.scope = p.scope and t.product_id = p.id and t.tag_id = $6
               ))
           and ($7::timestamptz is null or (p.created_at, p.id) > ($7, $8))
         order by p.created_at, p.id
         limit $9"
    ))
    .bind(ctx.scope.0)
    .bind(filter.status)
    .bind(filter.collection.map(CollectionId::as_uuid))
    .bind(filter.product_type.map(ProductTypeId::as_uuid))
    .bind(filter.category.map(CategoryId::as_uuid))
    .bind(filter.tag.map(ProductTagId::as_uuid))
    .bind(paging.after.map(|c| c.at))
    .bind(paging.after.map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| Cursor {
        at: row.created_at,
        id: row.id.as_uuid(),
    }))
}

pub async fn update_product(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ProductId,
    patch: ProductPatch,
) -> Result<Product> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Product {
            id: Some(id.as_uuid()),
        },
    )?;

    let title = match patch.title {
        Some(given) => Some(required("title", &given)?),
        None => None,
    };
    let new_handle = match patch.handle {
        Some(given) => Some(handle(&given)?),
        None => None,
    };

    let product = sqlx::query_as::<_, Product>(concat!(
        "update product set
             handle = coalesce($3, handle),
             title = coalesce($4, title),
             subtitle = case when $5::bool then $6 else subtitle end,
             description = case when $7::bool then $8 else description end,
             thumbnail_url = case when $9::bool then $10 else thumbnail_url end,
             is_discountable = coalesce($11, is_discountable),
             product_type_id = case when $12::bool then $13 else product_type_id end,
             product_collection_id = case when $14::bool then $15 else product_collection_id end,
             weight = coalesce($16, weight),
             length = coalesce($17, length),
             height = coalesce($18, height),
             width = coalesce($19, width),
             material = case when $20::bool then $21 else material end,
             hs_code = case when $22::bool then $23 else hs_code end,
             origin_country = case when $24::bool then $25 else origin_country end,
             external_id = case when $26::bool then $27 else external_id end,
             metadata = coalesce($28, metadata)
         where scope = $1 and id = $2 and deleted_at is null
           and not exists (
               select 1 from product other
               where other.scope = $1 and other.handle = $3 and other.id <> $2
                 and other.deleted_at is null
           )
         returning ",
        product_columns!()
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(new_handle)
    .bind(title)
    .bind(patch.subtitle.is_some())
    .bind(nullable(patch.subtitle))
    .bind(patch.description.is_some())
    .bind(nullable(patch.description))
    .bind(patch.thumbnail_url.is_some())
    .bind(nullable(patch.thumbnail_url))
    .bind(patch.is_discountable)
    .bind(patch.product_type_id.is_some())
    .bind(patch.product_type_id.flatten().map(ProductTypeId::as_uuid))
    .bind(patch.product_collection_id.is_some())
    .bind(
        patch
            .product_collection_id
            .flatten()
            .map(CollectionId::as_uuid),
    )
    .bind(patch.weight)
    .bind(patch.length)
    .bind(patch.height)
    .bind(patch.width)
    .bind(patch.material.is_some())
    .bind(nullable(patch.material))
    .bind(patch.hs_code.is_some())
    .bind(nullable(patch.hs_code))
    .bind(patch.origin_country.is_some())
    .bind(nullable(patch.origin_country))
    .bind(patch.external_id.is_some())
    .bind(nullable(patch.external_id))
    .bind(patch.metadata)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(product) = product else {
        return Err(refusal(
            tx,
            ctx,
            "product",
            "product",
            id.as_uuid(),
            "that handle is already a product here",
        )
        .await);
    };

    note(
        tx,
        ctx,
        Action::Write,
        "product",
        id.as_uuid(),
        serde_json::json!({ "handle": product.handle }),
    )
    .await?;

    Ok(product)
}

/// Moves a product's status. Archiving does not remove it: an order already
/// names it, and the row has to stay readable for that.
pub async fn set_product_status(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ProductId,
    status: ProductStatus,
) -> Result<Product> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Product {
            id: Some(id.as_uuid()),
        },
    )?;

    let product = sqlx::query_as::<_, Product>(concat!(
        "update product set status = $3
         where scope = $1 and id = $2 and deleted_at is null
         returning ",
        product_columns!()
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(status)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("product"))?;

    note(
        tx,
        ctx,
        Action::Write,
        "product",
        id.as_uuid(),
        serde_json::json!({ "status": status.as_str() }),
    )
    .await?;

    let name = match status {
        ProductStatus::Draft => "product.drafted",
        ProductStatus::Published => "product.published",
        ProductStatus::Archived => "product.archived",
    };
    ctx.emit(
        tx,
        Event {
            name,
            entity_id: id.as_uuid(),
            payload: serde_json::json!({ "handle": product.handle }),
        },
    )
    .await?;

    Ok(product)
}

pub async fn publish_product(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductId) -> Result<Product> {
    set_product_status(tx, ctx, id, ProductStatus::Published).await
}

pub async fn archive_product(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductId) -> Result<Product> {
    set_product_status(tx, ctx, id, ProductStatus::Archived).await
}

/// Soft: the handle is freed for reuse, the row stays for whatever already
/// points at it.
pub async fn delete_product(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductId) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Delete,
        Resource::Product {
            id: Some(id.as_uuid()),
        },
    )?;

    let deleted = sqlx::query(
        "update product set deleted_at = $3
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?
    .rows_affected();

    if deleted == 0 {
        return Err(Error::not_found("product"));
    }

    note(
        tx,
        ctx,
        Action::Delete,
        "product",
        id.as_uuid(),
        serde_json::json!({}),
    )
    .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Variant
// ---------------------------------------------------------------------------

pub async fn create_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    new: NewVariant,
) -> Result<ProductVariant> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Product {
            id: Some(product_id.as_uuid()),
        },
    )?;

    let title = required("title", &new.title)?;
    exists_product(tx, ctx, product_id).await?;
    let variant = insert_variant(tx, ctx, product_id, title, new).await?;

    note(
        tx,
        ctx,
        Action::Write,
        "product_variant",
        variant.id.as_uuid(),
        serde_json::json!({ "product": product_id.to_string(), "title": variant.title }),
    )
    .await?;

    Ok(variant)
}

async fn insert_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    title: String,
    new: NewVariant,
) -> Result<ProductVariant> {
    let id = VariantId::new();

    sqlx::query_as::<_, ProductVariant>(concat!(
        "insert into product_variant (id, scope, product_id, title, sku, barcode, ean, upc, \
         weight, length, height, width, material, hs_code, origin_country, mid_code, \
         manages_inventory, allows_backorder, rank, metadata, withdrawal_exclusion_reason, \
         is_giftcard)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, \
         $19, $20, $21, $22)
         on conflict do nothing
         returning ",
        variant_columns!()
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(product_id.as_uuid())
    .bind(title)
    .bind(nullable(new.sku))
    .bind(nullable(new.barcode))
    .bind(nullable(new.ean))
    .bind(nullable(new.upc))
    .bind(new.weight)
    .bind(new.length)
    .bind(new.height)
    .bind(new.width)
    .bind(nullable(new.material))
    .bind(nullable(new.hs_code))
    .bind(nullable(new.origin_country))
    .bind(nullable(new.mid_code))
    .bind(new.manages_inventory.unwrap_or(true))
    .bind(new.allows_backorder.unwrap_or(false))
    .bind(new.rank.unwrap_or(0))
    .bind(metadata_or_empty(new.metadata))
    .bind(
        new.withdrawal_exclusion
            .map(crate::order::WithdrawalExclusion::as_str),
    )
    .bind(new.is_giftcard.unwrap_or(false))
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that sku or barcode is already a variant here"))
}

pub async fn variant(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: VariantId) -> Result<ProductVariant> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    sqlx::query_as::<_, ProductVariant>(concat!(
        "select ",
        variant_columns!(),
        " from product_variant where scope = $1 and id = $2 and deleted_at is null"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("variant"))
}

/// What a variant makes true of the line that sells it.
#[derive(Debug, Clone, Copy, Default)]
pub struct LineFacts {
    /// Why buying it is outside the right of withdrawal; `None` is the
    /// ordinary case.
    pub withdrawal_exclusion: Option<crate::order::WithdrawalExclusion>,
    /// Whether selling it sells money rather than goods.
    pub is_giftcard: bool,
}

/// What each of these variants makes true of a line, keyed by variant. Only
/// the ones with something to say: a variant answering neither is not in the
/// map.
///
/// Bounded by what the caller passed in, and read in one statement because a
/// checkout asks this once for a whole cart.
pub async fn line_facts(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    ids: &[VariantId],
) -> Result<std::collections::HashMap<VariantId, LineFacts>> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    if ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let wanted: Vec<Uuid> = ids.iter().map(|id| id.as_uuid()).collect();
    let rows: Vec<(VariantId, Option<String>, bool)> = sqlx::query_as(
        "select id, withdrawal_exclusion_reason, is_giftcard from product_variant
         where scope = $1
           and id = any($2)
           and (withdrawal_exclusion_reason is not null or is_giftcard)",
    )
    .bind(ctx.scope.0)
    .bind(&wanted)
    .fetch_all(&mut **tx)
    .await?;

    rows.into_iter()
        .map(|(id, reason, is_giftcard)| {
            Ok((
                id,
                LineFacts {
                    withdrawal_exclusion: reason
                        .as_deref()
                        .map(crate::order::WithdrawalExclusion::parse)
                        .transpose()?,
                    is_giftcard,
                },
            ))
        })
        .collect()
}

pub async fn variants(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    paging: Paging,
) -> Result<Page<ProductVariant>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Product {
            id: Some(product_id.as_uuid()),
        },
    )?;

    let rows = sqlx::query_as::<_, ProductVariant>(concat!(
        "select ",
        variant_columns!(),
        " from product_variant
         where scope = $1
           and product_id = $2
           and deleted_at is null
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(product_id.as_uuid())
    .bind(paging.after.map(|c| c.at))
    .bind(paging.after.map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| Cursor {
        at: row.created_at,
        id: row.id.as_uuid(),
    }))
}

pub async fn update_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: VariantId,
    patch: VariantPatch,
) -> Result<ProductVariant> {
    let _: Permit = ctx.permit(Action::Write, Resource::Product { id: None })?;

    let title = match patch.title {
        Some(given) => Some(required("title", &given)?),
        None => None,
    };

    let variant = sqlx::query_as::<_, ProductVariant>(concat!(
        "update product_variant set
             title = coalesce($3, title),
             sku = case when $4::bool then $5 else sku end,
             barcode = case when $6::bool then $7 else barcode end,
             ean = case when $8::bool then $9 else ean end,
             upc = case when $10::bool then $11 else upc end,
             weight = coalesce($12, weight),
             length = coalesce($13, length),
             height = coalesce($14, height),
             width = coalesce($15, width),
             material = case when $16::bool then $17 else material end,
             hs_code = case when $18::bool then $19 else hs_code end,
             origin_country = case when $20::bool then $21 else origin_country end,
             mid_code = case when $22::bool then $23 else mid_code end,
             manages_inventory = coalesce($24, manages_inventory),
             allows_backorder = coalesce($25, allows_backorder),
             rank = coalesce($26, rank),
             metadata = coalesce($27, metadata),
             withdrawal_exclusion_reason = case when $28::bool then $29
                                           else withdrawal_exclusion_reason end,
             is_giftcard = coalesce($30, is_giftcard)
         where scope = $1 and id = $2 and deleted_at is null
           and not exists (
               select 1 from product_variant other
               where other.scope = $1 and other.id <> $2 and other.deleted_at is null
                 and (($4::bool and other.sku = $5) or ($6::bool and other.barcode = $7))
           )
         returning ",
        variant_columns!()
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(title)
    .bind(patch.sku.is_some())
    .bind(nullable(patch.sku))
    .bind(patch.barcode.is_some())
    .bind(nullable(patch.barcode))
    .bind(patch.ean.is_some())
    .bind(nullable(patch.ean))
    .bind(patch.upc.is_some())
    .bind(nullable(patch.upc))
    .bind(patch.weight)
    .bind(patch.length)
    .bind(patch.height)
    .bind(patch.width)
    .bind(patch.material.is_some())
    .bind(nullable(patch.material))
    .bind(patch.hs_code.is_some())
    .bind(nullable(patch.hs_code))
    .bind(patch.origin_country.is_some())
    .bind(nullable(patch.origin_country))
    .bind(patch.mid_code.is_some())
    .bind(nullable(patch.mid_code))
    .bind(patch.manages_inventory)
    .bind(patch.allows_backorder)
    .bind(patch.rank)
    .bind(patch.metadata)
    .bind(patch.withdrawal_exclusion.is_some())
    .bind(
        patch
            .withdrawal_exclusion
            .flatten()
            .map(crate::order::WithdrawalExclusion::as_str),
    )
    .bind(patch.is_giftcard)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(variant) = variant else {
        return Err(refusal(
            tx,
            ctx,
            "product_variant",
            "variant",
            id.as_uuid(),
            "that sku or barcode is already a variant here",
        )
        .await);
    };

    note(
        tx,
        ctx,
        Action::Write,
        "product_variant",
        id.as_uuid(),
        serde_json::json!({ "title": variant.title }),
    )
    .await?;

    Ok(variant)
}

pub async fn delete_variant(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: VariantId) -> Result<()> {
    let _: Permit = ctx.permit(Action::Delete, Resource::Product { id: None })?;

    let deleted = sqlx::query(
        "update product_variant set deleted_at = $3
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?
    .rows_affected();

    if deleted == 0 {
        return Err(Error::not_found("variant"));
    }

    note(
        tx,
        ctx,
        Action::Delete,
        "product_variant",
        id.as_uuid(),
        serde_json::json!({}),
    )
    .await?;

    Ok(())
}

async fn exists_product(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductId) -> Result<()> {
    let found: Option<Uuid> = sqlx::query_scalar(
        "select id from product where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    found.map(|_| ()).ok_or_else(|| Error::not_found("product"))
}

// ---------------------------------------------------------------------------
// Options and their values
// ---------------------------------------------------------------------------

pub async fn add_option(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    title: &str,
    rank: i32,
) -> Result<ProductOption> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Product {
            id: Some(product_id.as_uuid()),
        },
    )?;

    let title = required("title", title)?;
    if rank < 0 {
        return Err(Error::invalid("a rank does not go below zero"));
    }
    exists_product(tx, ctx, product_id).await?;

    let id = OptionId::new();
    let option = sqlx::query_as::<_, ProductOption>(
        "insert into product_option (id, scope, product_id, title, rank)
         values ($1, $2, $3, $4, $5)
         on conflict do nothing
         returning id, product_id, title, rank, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(product_id.as_uuid())
    .bind(&title)
    .bind(rank)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that option is already on this product"))?;

    note(
        tx,
        ctx,
        Action::Write,
        "product_option",
        id.as_uuid(),
        serde_json::json!({ "product": product_id.to_string(), "title": title }),
    )
    .await?;

    Ok(option)
}

pub async fn add_option_value(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    option_id: OptionId,
    value: &str,
    rank: i32,
) -> Result<ProductOptionValue> {
    let _: Permit = ctx.permit(Action::Write, Resource::Product { id: None })?;

    let value = required("value", value)?;
    if rank < 0 {
        return Err(Error::invalid("a rank does not go below zero"));
    }

    let owner: Option<Uuid> = sqlx::query_scalar(
        "select id from product_option where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(option_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;
    if owner.is_none() {
        return Err(Error::not_found("option"));
    }

    let id = OptionValueId::new();
    let row = sqlx::query_as::<_, ProductOptionValue>(
        "insert into product_option_value (id, scope, option_id, value, rank)
         values ($1, $2, $3, $4, $5)
         on conflict do nothing
         returning id, option_id, value, rank, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(option_id.as_uuid())
    .bind(&value)
    .bind(rank)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that value is already on this option"))?;

    note(
        tx,
        ctx,
        Action::Write,
        "product_option_value",
        id.as_uuid(),
        serde_json::json!({ "option": option_id.to_string(), "value": value }),
    )
    .await?;

    Ok(row)
}

/// Every option of a product with its values, in the order they are shown.
/// Bounded by the product rather than by the shop, so it is not a page.
pub async fn option_matrix(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
) -> Result<Vec<OptionWithValues>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Product {
            id: Some(product_id.as_uuid()),
        },
    )?;

    let options = sqlx::query_as::<_, ProductOption>(
        "select id, product_id, title, rank, created_at
         from product_option
         where scope = $1 and product_id = $2 and deleted_at is null
         order by rank, created_at, id
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(product_id.as_uuid())
    .bind(MAX_ATTACHED)
    .fetch_all(&mut **tx)
    .await?;

    let ids: Vec<Uuid> = options.iter().map(|o| o.id.as_uuid()).collect();
    let values = sqlx::query_as::<_, ProductOptionValue>(
        "select id, option_id, value, rank, created_at
         from product_option_value
         where scope = $1 and option_id = any($2) and deleted_at is null
         order by rank, value, id",
    )
    .bind(ctx.scope.0)
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;

    Ok(options
        .into_iter()
        .map(|option| {
            let mine = values
                .iter()
                .filter(|v| v.option_id == option.id)
                .cloned()
                .collect();
            OptionWithValues {
                option,
                values: mine,
            }
        })
        .collect())
}

/// One option and everything it may be set to.
pub async fn product_option(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OptionId,
) -> Result<OptionWithValues> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    let option = sqlx::query_as::<_, ProductOption>(
        "select id, product_id, title, rank, created_at
         from product_option
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("option"))?;

    let values = sqlx::query_as::<_, ProductOptionValue>(
        "select id, option_id, value, rank, created_at
         from product_option_value
         where scope = $1 and option_id = $2 and deleted_at is null
         order by rank, value, id
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(MAX_ATTACHED)
    .fetch_all(&mut **tx)
    .await?;

    Ok(OptionWithValues { option, values })
}

/// What one variant is: one value per option, in option order.
pub async fn variant_options(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
) -> Result<Vec<ProductOptionValue>> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    let rows = sqlx::query_as::<_, ProductOptionValue>(
        "select v.id, v.option_id, v.value, v.rank, v.created_at
         from product_variant_option_value pv
         join product_option_value v on v.id = pv.option_value_id and v.scope = pv.scope
         join product_option o on o.id = pv.option_id and o.scope = pv.scope
         where pv.scope = $1 and pv.variant_id = $2
         order by o.rank, o.created_at, v.rank
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(variant_id.as_uuid())
    .bind(MAX_ATTACHED)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

/// Sets the whole combination at once. A variant that leaves an axis unset
/// cannot be ordered, so a partial set is refused rather than stored.
pub async fn set_variant_options(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
    choices: &[OptionValueId],
) -> Result<()> {
    let _: Permit = ctx.permit(Action::Write, Resource::Product { id: None })?;

    let product_id: Option<Uuid> = sqlx::query_scalar(
        "select product_id from product_variant
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(variant_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;
    let product_id = product_id.ok_or_else(|| Error::not_found("variant"))?;

    let wanted: Vec<Uuid> = choices.iter().map(|id| id.as_uuid()).collect();
    let pairs: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "select v.id, v.option_id
         from product_option_value v
         join product_option o on o.id = v.option_id and o.scope = v.scope
         where v.scope = $1
           and v.id = any($2)
           and o.product_id = $3
           and v.deleted_at is null
           and o.deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(&wanted)
    .bind(product_id)
    .fetch_all(&mut **tx)
    .await?;

    if pairs.len() != wanted.len() {
        return Err(Error::invalid(
            "one of those values does not belong to this product",
        ));
    }

    let options: i64 = sqlx::query_scalar(
        "select count(*) from product_option
         where scope = $1 and product_id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(product_id)
    .fetch_one(&mut **tx)
    .await?;

    if options != pairs.len() as i64 {
        return Err(Error::invalid(
            "a variant needs one value for every option of its product",
        ));
    }

    sqlx::query("delete from product_variant_option_value where scope = $1 and variant_id = $2")
        .bind(ctx.scope.0)
        .bind(variant_id.as_uuid())
        .execute(&mut **tx)
        .await?;

    write_combination(tx, ctx, variant_id, &pairs).await?;

    note(
        tx,
        ctx,
        Action::Write,
        "product_variant_option_value",
        variant_id.as_uuid(),
        serde_json::json!({ "values": wanted.len() }),
    )
    .await
}

async fn write_combination(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
    pairs: &[(Uuid, Uuid)],
) -> Result<()> {
    for (value_id, option_id) in pairs {
        let written = sqlx::query(
            "insert into product_variant_option_value
                 (id, scope, variant_id, option_id, option_value_id)
             values ($1, $2, $3, $4, $5)
             on conflict do nothing",
        )
        .bind(Uuid::now_v7())
        .bind(ctx.scope.0)
        .bind(variant_id.as_uuid())
        .bind(option_id)
        .bind(value_id)
        .execute(&mut **tx)
        .await?;
        if written.rows_affected() == 0 {
            return Err(Error::conflict(
                "that option is already set on this variant",
            ));
        }
    }

    Ok(())
}

/// Makes a variant for every combination of the product's options that does not
/// already exist and is not excluded. Returns only what it created.
pub async fn generate_variants(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    plan: VariantPlan,
) -> Result<Vec<ProductVariant>> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Product {
            id: Some(product_id.as_uuid()),
        },
    )?;

    exists_product(tx, ctx, product_id).await?;
    let matrix = option_matrix(tx, ctx, product_id).await?;

    if matrix.is_empty() {
        return Err(Error::invalid(
            "a product needs an option before variants can be generated from it",
        ));
    }
    if matrix.len() > MAX_OPTIONS {
        return Err(Error::invalid(
            "that is more options than variants can be generated from",
        ));
    }

    let mut total: usize = 1;
    for axis in &matrix {
        if axis.values.is_empty() {
            return Err(Error::invalid(format!(
                "option {:?} has no values to generate from",
                axis.option.title
            )));
        }
        total = total
            .checked_mul(axis.values.len())
            .filter(|count| *count <= MAX_COMBINATIONS)
            .ok_or_else(|| Error::invalid("that is more combinations than one call may make"))?;
    }

    let excluded: Vec<Vec<Uuid>> = plan.exclude.iter().map(Combination::key).collect();
    let existing = existing_combinations(tx, ctx, product_id).await?;

    let mut made = Vec::new();
    for combination in cartesian(&matrix) {
        let mut key: Vec<Uuid> = combination.iter().map(|(value, _)| *value).collect();
        key.sort_unstable();
        if excluded.contains(&key) || existing.contains(&key) {
            continue;
        }

        let title = combination
            .iter()
            .map(|(_, label)| label.as_str())
            .collect::<Vec<_>>()
            .join(" / ");

        let variant = insert_variant(
            tx,
            ctx,
            product_id,
            title,
            NewVariant {
                manages_inventory: plan.manages_inventory,
                allows_backorder: plan.allows_backorder,
                rank: i32::try_from(made.len()).ok(),
                ..NewVariant::default()
            },
        )
        .await?;

        let pairs = pairs_for(&matrix, &combination);
        write_combination(tx, ctx, variant.id, &pairs).await?;

        note(
            tx,
            ctx,
            Action::Write,
            "product_variant",
            variant.id.as_uuid(),
            serde_json::json!({ "product": product_id.to_string(), "generated": true }),
        )
        .await?;

        made.push(variant);
    }

    Ok(made)
}

/// Every axis crossed with every other, one entry per axis carrying the value's
/// id and its label.
fn cartesian(matrix: &[OptionWithValues]) -> Vec<Vec<(Uuid, String)>> {
    let mut rows: Vec<Vec<(Uuid, String)>> = vec![Vec::new()];

    for axis in matrix {
        let mut next = Vec::with_capacity(rows.len() * axis.values.len());
        for row in &rows {
            for value in &axis.values {
                let mut longer = row.clone();
                longer.push((value.id.as_uuid(), value.value.clone()));
                next.push(longer);
            }
        }
        rows = next;
    }

    rows
}

fn pairs_for(matrix: &[OptionWithValues], combination: &[(Uuid, String)]) -> Vec<(Uuid, Uuid)> {
    combination
        .iter()
        .filter_map(|(value_id, _)| {
            matrix.iter().find_map(|axis| {
                axis.values
                    .iter()
                    .find(|value| value.id.as_uuid() == *value_id)
                    .map(|_| (*value_id, axis.option.id.as_uuid()))
            })
        })
        .collect()
}

async fn existing_combinations(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
) -> Result<Vec<Vec<Uuid>>> {
    let rows: Vec<(Uuid, Vec<Uuid>)> = sqlx::query_as(
        "select pv.variant_id, array_agg(pv.option_value_id order by pv.option_value_id)
         from product_variant_option_value pv
         join product_variant v on v.id = pv.variant_id and v.scope = pv.scope
         where pv.scope = $1 and v.product_id = $2 and v.deleted_at is null
         group by pv.variant_id",
    )
    .bind(ctx.scope.0)
    .bind(product_id.as_uuid())
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows.into_iter().map(|(_, values)| values).collect())
}

// ---------------------------------------------------------------------------
// Collection, type, tag
// ---------------------------------------------------------------------------

pub async fn create_collection(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    title: &str,
    handle_text: &str,
) -> Result<ProductCollection> {
    let _: Permit = ctx.permit(Action::Write, Resource::Product { id: None })?;

    let title = required("title", title)?;
    let handle = handle(handle_text)?;
    let id = CollectionId::new();

    let collection = sqlx::query_as::<_, ProductCollection>(
        "insert into product_collection (id, scope, handle, title)
         values ($1, $2, $3, $4)
         on conflict do nothing
         returning id, handle, title, metadata, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(&handle)
    .bind(&title)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that handle is already a collection here"))?;

    note(
        tx,
        ctx,
        Action::Write,
        "product_collection",
        id.as_uuid(),
        serde_json::json!({ "handle": handle }),
    )
    .await?;

    Ok(collection)
}

pub async fn update_collection(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CollectionId,
    title: Option<&str>,
    handle_text: Option<&str>,
) -> Result<ProductCollection> {
    let _: Permit = ctx.permit(Action::Write, Resource::Product { id: None })?;

    let title = match title {
        Some(given) => Some(required("title", given)?),
        None => None,
    };
    let new_handle = match handle_text {
        Some(given) => Some(handle(given)?),
        None => None,
    };

    let collection = sqlx::query_as::<_, ProductCollection>(
        "update product_collection
            set title = coalesce($3, title), handle = coalesce($4, handle)
         where scope = $1 and id = $2 and deleted_at is null
           and not exists (
               select 1 from product_collection other
               where other.scope = $1 and other.handle = $4 and other.id <> $2
                 and other.deleted_at is null
           )
         returning id, handle, title, metadata, created_at",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(title)
    .bind(new_handle)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(collection) = collection else {
        return Err(refusal(
            tx,
            ctx,
            "product_collection",
            "collection",
            id.as_uuid(),
            "that handle is already a collection here",
        )
        .await);
    };

    note(
        tx,
        ctx,
        Action::Write,
        "product_collection",
        id.as_uuid(),
        serde_json::json!({ "handle": collection.handle }),
    )
    .await?;

    Ok(collection)
}

pub async fn collection(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CollectionId,
) -> Result<ProductCollection> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    sqlx::query_as::<_, ProductCollection>(
        "select id, handle, title, metadata, created_at
         from product_collection
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("collection"))
}

pub async fn collections(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    paging: Paging,
) -> Result<Page<ProductCollection>> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    let rows = sqlx::query_as::<_, ProductCollection>(
        "select id, handle, title, metadata, created_at
         from product_collection
         where scope = $1
           and deleted_at is null
           and ($2::timestamptz is null or (created_at, id) > ($2, $3))
         order by created_at, id
         limit $4",
    )
    .bind(ctx.scope.0)
    .bind(paging.after.map(|c| c.at))
    .bind(paging.after.map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| Cursor {
        at: row.created_at,
        id: row.id.as_uuid(),
    }))
}

pub async fn delete_collection(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CollectionId) -> Result<()> {
    soft_delete(tx, ctx, "product_collection", "collection", id.as_uuid()).await
}

pub async fn create_type(tx: &mut Tx<'_>, ctx: &Ctx<'_>, value: &str) -> Result<ProductType> {
    let _: Permit = ctx.permit(Action::Write, Resource::Product { id: None })?;

    let value = required("value", value)?;
    let id = ProductTypeId::new();

    let row = sqlx::query_as::<_, ProductType>(
        "insert into product_type (id, scope, value)
         values ($1, $2, $3)
         on conflict do nothing
         returning id, value, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(&value)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that type is already here"))?;

    note(
        tx,
        ctx,
        Action::Write,
        "product_type",
        id.as_uuid(),
        serde_json::json!({ "value": value }),
    )
    .await?;

    Ok(row)
}

pub async fn types(tx: &mut Tx<'_>, ctx: &Ctx<'_>, paging: Paging) -> Result<Page<ProductType>> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    let rows = sqlx::query_as::<_, ProductType>(
        "select id, value, created_at
         from product_type
         where scope = $1
           and deleted_at is null
           and ($2::timestamptz is null or (created_at, id) > ($2, $3))
         order by created_at, id
         limit $4",
    )
    .bind(ctx.scope.0)
    .bind(paging.after.map(|c| c.at))
    .bind(paging.after.map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| Cursor {
        at: row.created_at,
        id: row.id.as_uuid(),
    }))
}

pub async fn product_type(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ProductTypeId,
) -> Result<ProductType> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    sqlx::query_as::<_, ProductType>(
        "select id, value, created_at
         from product_type
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("type"))
}

pub async fn delete_type(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductTypeId) -> Result<()> {
    soft_delete(tx, ctx, "product_type", "type", id.as_uuid()).await
}

pub async fn create_tag(tx: &mut Tx<'_>, ctx: &Ctx<'_>, value: &str) -> Result<ProductTag> {
    let _: Permit = ctx.permit(Action::Write, Resource::Product { id: None })?;

    let value = required("value", value)?;
    let id = ProductTagId::new();

    let row = sqlx::query_as::<_, ProductTag>(
        "insert into product_tag (id, scope, value)
         values ($1, $2, $3)
         on conflict do nothing
         returning id, value, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(&value)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that tag is already here"))?;

    note(
        tx,
        ctx,
        Action::Write,
        "product_tag",
        id.as_uuid(),
        serde_json::json!({ "value": value }),
    )
    .await?;

    Ok(row)
}

pub async fn tags(tx: &mut Tx<'_>, ctx: &Ctx<'_>, paging: Paging) -> Result<Page<ProductTag>> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    let rows = sqlx::query_as::<_, ProductTag>(
        "select id, value, created_at
         from product_tag
         where scope = $1
           and deleted_at is null
           and ($2::timestamptz is null or (created_at, id) > ($2, $3))
         order by created_at, id
         limit $4",
    )
    .bind(ctx.scope.0)
    .bind(paging.after.map(|c| c.at))
    .bind(paging.after.map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| Cursor {
        at: row.created_at,
        id: row.id.as_uuid(),
    }))
}

pub async fn product_tag(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductTagId) -> Result<ProductTag> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    sqlx::query_as::<_, ProductTag>(
        "select id, value, created_at
         from product_tag
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("tag"))
}

pub async fn delete_tag(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductTagId) -> Result<()> {
    soft_delete(tx, ctx, "product_tag", "tag", id.as_uuid()).await
}

pub async fn tag_product(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    tag_id: ProductTagId,
) -> Result<()> {
    link(
        tx,
        ctx,
        "product_tag_link",
        "tag_id",
        product_id,
        tag_id.as_uuid(),
    )
    .await
}

pub async fn untag_product(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    tag_id: ProductTagId,
) -> Result<()> {
    unlink(
        tx,
        ctx,
        "product_tag_link",
        "tag_id",
        product_id,
        tag_id.as_uuid(),
    )
    .await
}

/// A product's tags. Bounded by the product, so not a page.
pub async fn product_tags(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
) -> Result<Vec<ProductTag>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Product {
            id: Some(product_id.as_uuid()),
        },
    )?;

    let rows = sqlx::query_as::<_, ProductTag>(
        "select t.id, t.value, t.created_at
         from product_tag_link l
         join product_tag t on t.id = l.tag_id and t.scope = l.scope
         where l.scope = $1 and l.product_id = $2 and t.deleted_at is null
         order by t.value
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(product_id.as_uuid())
    .bind(MAX_ATTACHED)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

// ---------------------------------------------------------------------------
// Category
// ---------------------------------------------------------------------------

pub async fn create_category(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewCategory,
) -> Result<ProductCategory> {
    let _: Permit = ctx.permit(Action::Write, Resource::Product { id: None })?;

    let name = required("name", &new.name)?;
    let handle = handle(&new.handle)?;
    if new.rank.is_some_and(|rank| rank < 0) {
        return Err(Error::invalid("a rank does not go below zero"));
    }

    if let Some(parent) = new.parent_id {
        category(tx, ctx, parent).await?;
    }

    let id = CategoryId::new();
    let row = sqlx::query_as::<_, ProductCategory>(concat!(
        "insert into product_category
             (id, scope, parent_id, name, handle, description, rank, is_active, is_internal, \
         metadata)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         on conflict do nothing
         returning ",
        category_columns!(),
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.parent_id.map(CategoryId::as_uuid))
    .bind(&name)
    .bind(&handle)
    .bind(new.description.unwrap_or_default())
    .bind(new.rank.unwrap_or(0))
    .bind(new.is_active.unwrap_or(false))
    .bind(new.is_internal.unwrap_or(false))
    .bind(metadata_or_empty(new.metadata))
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that handle is already a category here"))?;

    note(
        tx,
        ctx,
        Action::Write,
        "product_category",
        id.as_uuid(),
        serde_json::json!({ "handle": handle, "parent": new.parent_id.map(|p| p.to_string()) }),
    )
    .await?;

    Ok(row)
}

pub async fn category(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CategoryId) -> Result<ProductCategory> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    sqlx::query_as::<_, ProductCategory>(concat!(
        "select ",
        category_columns!(),
        " from product_category where scope = $1 and id = $2 and deleted_at is null"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("category"))
}

/// The children of one category, or the roots when no parent is named.
pub async fn categories(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    parent: Option<CategoryId>,
    paging: Paging,
) -> Result<Page<ProductCategory>> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    let rows = sqlx::query_as::<_, ProductCategory>(concat!(
        "select ",
        category_columns!(),
        " from product_category
         where scope = $1
           and deleted_at is null
           and (($2::uuid is null and parent_id is null) or parent_id = $2)
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(parent.map(CategoryId::as_uuid))
    .bind(paging.after.map(|c| c.at))
    .bind(paging.after.map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| Cursor {
        at: row.created_at,
        id: row.id.as_uuid(),
    }))
}

/// The category and everything under it, however deep — one index scan on
/// `mpath` rather than a walk.
pub async fn category_subtree(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    root: CategoryId,
    paging: Paging,
) -> Result<Page<ProductCategory>> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    let rows = sqlx::query_as::<_, ProductCategory>(concat!(
        "select ",
        category_columns!(),
        " from product_category
         where scope = $1
           and deleted_at is null
           and mpath like (
             select root.mpath || '%'
             from product_category root
             where root.scope = $1 and root.id = $2
           )
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(root.as_uuid())
    .bind(paging.after.map(|c| c.at))
    .bind(paging.after.map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| Cursor {
        at: row.created_at,
        id: row.id.as_uuid(),
    }))
}

pub async fn update_category(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CategoryId,
    patch: CategoryPatch,
) -> Result<ProductCategory> {
    let _: Permit = ctx.permit(Action::Write, Resource::Product { id: None })?;

    let name = match patch.name {
        Some(given) => Some(required("name", &given)?),
        None => None,
    };
    let new_handle = match patch.handle {
        Some(given) => Some(handle(&given)?),
        None => None,
    };
    if patch.rank.is_some_and(|rank| rank < 0) {
        return Err(Error::invalid("a rank does not go below zero"));
    }

    let row = sqlx::query_as::<_, ProductCategory>(concat!(
        "update product_category set
             name = coalesce($3, name),
             handle = coalesce($4, handle),
             description = coalesce($5, description),
             rank = coalesce($6, rank),
             is_active = coalesce($7, is_active),
             is_internal = coalesce($8, is_internal),
             metadata = coalesce($9, metadata)
         where scope = $1 and id = $2 and deleted_at is null
           and not exists (
               select 1 from product_category other
               where other.scope = $1 and other.handle = $4 and other.id <> $2
                 and other.deleted_at is null
           )
         returning ",
        category_columns!()
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(name)
    .bind(new_handle)
    .bind(patch.description)
    .bind(patch.rank)
    .bind(patch.is_active)
    .bind(patch.is_internal)
    .bind(patch.metadata)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(row) = row else {
        return Err(refusal(
            tx,
            ctx,
            "product_category",
            "category",
            id.as_uuid(),
            "that handle is already a category here",
        )
        .await);
    };

    note(
        tx,
        ctx,
        Action::Write,
        "product_category",
        id.as_uuid(),
        serde_json::json!({ "handle": row.handle }),
    )
    .await?;

    Ok(row)
}

/// Moves a category, and with it everything underneath. A move into its own
/// subtree is refused here as well as by the trigger, so the caller gets an
/// answer rather than a database error.
pub async fn move_category(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CategoryId,
    parent: Option<CategoryId>,
) -> Result<ProductCategory> {
    let _: Permit = ctx.permit(Action::Write, Resource::Product { id: None })?;

    let moving = category(tx, ctx, id).await?;

    if let Some(parent) = parent {
        if parent == id {
            return Err(Error::invalid("a category cannot be its own parent"));
        }

        let into = category(tx, ctx, parent).await?;
        let needle = id.to_string();
        if into.mpath.split('.').any(|step| step == needle) {
            return Err(Error::invalid(
                "that would put a category inside its own subtree",
            ));
        }
    } else if moving.parent_id.is_none() {
        return Ok(moving);
    }

    let row = sqlx::query_as::<_, ProductCategory>(concat!(
        "update product_category set parent_id = $3
         where scope = $1 and id = $2 and deleted_at is null
         returning ",
        category_columns!()
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(parent.map(CategoryId::as_uuid))
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("category"))?;

    note(
        tx,
        ctx,
        Action::Write,
        "product_category",
        id.as_uuid(),
        serde_json::json!({ "parent": parent.map(|p| p.to_string()) }),
    )
    .await?;

    Ok(row)
}

pub async fn delete_category(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CategoryId) -> Result<()> {
    let _: Permit = ctx.permit(Action::Delete, Resource::Product { id: None })?;

    let children: i64 = sqlx::query_scalar(
        "select count(*) from product_category
         where scope = $1 and parent_id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;

    if children > 0 {
        return Err(Error::conflict(
            "that category still has categories under it",
        ));
    }

    soft_delete(tx, ctx, "product_category", "category", id.as_uuid()).await
}

pub async fn add_product_to_category(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    category_id: CategoryId,
) -> Result<()> {
    link(
        tx,
        ctx,
        "product_category_link",
        "category_id",
        product_id,
        category_id.as_uuid(),
    )
    .await
}

pub async fn remove_product_from_category(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    category_id: CategoryId,
) -> Result<()> {
    unlink(
        tx,
        ctx,
        "product_category_link",
        "category_id",
        product_id,
        category_id.as_uuid(),
    )
    .await
}

pub async fn product_categories(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
) -> Result<Vec<ProductCategory>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Product {
            id: Some(product_id.as_uuid()),
        },
    )?;

    let rows = sqlx::query_as::<_, ProductCategory>(
        "select c.id, c.parent_id, c.mpath, c.name, c.handle, c.description, c.rank, c.is_active, \
         c.is_internal, c.metadata, c.created_at, c.updated_at
         from product_category_link l
         join product_category c on c.id = l.category_id and c.scope = l.scope
         where l.scope = $1 and l.product_id = $2 and c.deleted_at is null
         order by c.mpath
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(product_id.as_uuid())
    .bind(MAX_ATTACHED)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

pub async fn add_image(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    new: NewImage,
) -> Result<ProductImage> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Product {
            id: Some(product_id.as_uuid()),
        },
    )?;

    let url = required("url", &new.url)?;
    exists_product(tx, ctx, product_id).await?;

    let id = ProductImageId::new();
    let image = sqlx::query_as::<_, ProductImage>(
        "insert into product_image (id, scope, product_id, url, alt_text, rank)
         values ($1, $2, $3, $4, $5, $6)
         returning id, product_id, url, alt_text, rank, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(product_id.as_uuid())
    .bind(&url)
    .bind(nullable(new.alt_text))
    .bind(new.rank.unwrap_or(0))
    .fetch_one(&mut **tx)
    .await?;

    note(
        tx,
        ctx,
        Action::Write,
        "product_image",
        id.as_uuid(),
        serde_json::json!({ "product": product_id.to_string() }),
    )
    .await?;

    Ok(image)
}

/// A product's images in the order they are shown. Bounded by the product, so
/// not a page.
pub async fn images(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
) -> Result<Vec<ProductImage>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Product {
            id: Some(product_id.as_uuid()),
        },
    )?;

    let rows = sqlx::query_as::<_, ProductImage>(
        "select id, product_id, url, alt_text, rank, created_at
         from product_image
         where scope = $1 and product_id = $2
         order by rank, created_at, id
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(product_id.as_uuid())
    .bind(MAX_ATTACHED)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

pub async fn remove_image(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductImageId) -> Result<()> {
    let _: Permit = ctx.permit(Action::Delete, Resource::Product { id: None })?;

    let gone = sqlx::query("delete from product_image where scope = $1 and id = $2")
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .execute(&mut **tx)
        .await?
        .rows_affected();

    if gone == 0 {
        return Err(Error::not_found("image"));
    }

    note(
        tx,
        ctx,
        Action::Delete,
        "product_image",
        id.as_uuid(),
        serde_json::json!({}),
    )
    .await
}

// ---------------------------------------------------------------------------
// Localisation
// ---------------------------------------------------------------------------

/// Writes one locale's text, replacing whatever was there for it.
pub async fn put_translation(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    translation: ProductTranslation,
) -> Result<ProductTranslation> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Product {
            id: Some(product_id.as_uuid()),
        },
    )?;

    let locale = locale(&translation.locale)?;
    let title = required("title", &translation.title)?;
    let translated_handle = match nullable(translation.handle) {
        Some(given) => Some(handle(&given)?),
        None => None,
    };
    exists_product(tx, ctx, product_id).await?;

    let row = sqlx::query_as::<_, ProductTranslation>(
        "insert into product_translation
             (id, scope, product_id, locale, title, subtitle, description, handle)
         select $1::uuid, $2::uuid, $3::uuid, $4::text, $5::text, $6::text, $7::text, $8::text
         where not exists (
             select 1 from product_translation other
             where other.scope = $2 and other.locale = $4 and other.handle = $8
               and other.product_id <> $3
         )
         on conflict (scope, product_id, locale) do update
             set title = excluded.title,
                 subtitle = excluded.subtitle,
                 description = excluded.description,
                 handle = excluded.handle
         returning product_id, locale, title, subtitle, description, handle",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(product_id.as_uuid())
    .bind(&locale)
    .bind(&title)
    .bind(nullable(translation.subtitle))
    .bind(nullable(translation.description))
    .bind(translated_handle)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that handle is already taken in this locale"))?;

    note(
        tx,
        ctx,
        Action::Write,
        "product_translation",
        product_id.as_uuid(),
        serde_json::json!({ "locale": locale }),
    )
    .await?;

    Ok(row)
}

pub async fn translations(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
) -> Result<Vec<ProductTranslation>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Product {
            id: Some(product_id.as_uuid()),
        },
    )?;

    let rows = sqlx::query_as::<_, ProductTranslation>(
        "select product_id, locale, title, subtitle, description, handle
         from product_translation
         where scope = $1 and product_id = $2
         order by locale
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(product_id.as_uuid())
    .bind(MAX_ATTACHED)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

pub async fn remove_translation(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    wanted: &str,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Delete,
        Resource::Product {
            id: Some(product_id.as_uuid()),
        },
    )?;

    let locale = locale(wanted)?;
    let gone = sqlx::query(
        "delete from product_translation
         where scope = $1 and product_id = $2 and locale = $3",
    )
    .bind(ctx.scope.0)
    .bind(product_id.as_uuid())
    .bind(&locale)
    .execute(&mut **tx)
    .await?
    .rows_affected();

    if gone == 0 {
        return Err(Error::not_found("translation"));
    }

    note(
        tx,
        ctx,
        Action::Delete,
        "product_translation",
        product_id.as_uuid(),
        serde_json::json!({ "locale": locale }),
    )
    .await
}

/// A product read in one language: the exact locale if it is there, then the
/// bare language of it, then the product's own columns. A missing translation
/// is a fallback rather than a refusal — a shop with one untranslated product
/// still has to sell it.
pub async fn localised(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    wanted: &str,
) -> Result<Localised> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Product {
            id: Some(product_id.as_uuid()),
        },
    )?;

    let locale = locale(wanted)?;
    let language = locale.split('-').next().unwrap_or(&locale).to_owned();
    let product = product(tx, ctx, product_id).await?;

    let found = sqlx::query_as::<_, ProductTranslation>(
        "select product_id, locale, title, subtitle, description, handle
         from product_translation
         where scope = $1 and product_id = $2 and locale in ($3, $4)
         order by case when locale = $3 then 0 else 1 end
         limit 1",
    )
    .bind(ctx.scope.0)
    .bind(product_id.as_uuid())
    .bind(&locale)
    .bind(&language)
    .fetch_optional(&mut **tx)
    .await?;

    Ok(match found {
        Some(row) => Localised {
            product_id,
            locale: Some(row.locale),
            title: row.title,
            subtitle: row.subtitle.or(product.subtitle),
            description: row.description.or(product.description),
            handle: row.handle.unwrap_or(product.handle),
            is_fallback: false,
        },
        None => Localised {
            product_id,
            locale: None,
            title: product.title,
            subtitle: product.subtitle,
            description: product.description,
            handle: product.handle,
            is_fallback: true,
        },
    })
}

// ---------------------------------------------------------------------------
// The two shapes shared by the small tables
// ---------------------------------------------------------------------------

async fn soft_delete(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    table: &'static str,
    entity: &'static str,
    id: Uuid,
) -> Result<()> {
    let _: Permit = ctx.permit(Action::Delete, Resource::Product { id: None })?;

    // The table name is one of this module's own literals, never a caller's.
    let statement = format!(
        "update {table} set deleted_at = $3
         where scope = $1 and id = $2 and deleted_at is null"
    );

    let deleted = sqlx::query(&statement)
        .bind(ctx.scope.0)
        .bind(id)
        .bind(ctx.now())
        .execute(&mut **tx)
        .await?
        .rows_affected();

    if deleted == 0 {
        return Err(Error::not_found(entity));
    }

    note(tx, ctx, Action::Delete, table, id, serde_json::json!({})).await
}

async fn link(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    table: &'static str,
    column: &'static str,
    product_id: ProductId,
    other: Uuid,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Product {
            id: Some(product_id.as_uuid()),
        },
    )?;

    exists_product(tx, ctx, product_id).await?;

    // Both names are this module's own literals, never a caller's.
    let statement = format!(
        "insert into {table} (id, scope, product_id, {column})
         values ($1, $2, $3, $4)
         on conflict do nothing"
    );

    sqlx::query(&statement)
        .bind(Uuid::now_v7())
        .bind(ctx.scope.0)
        .bind(product_id.as_uuid())
        .bind(other)
        .execute(&mut **tx)
        .await?;

    note(
        tx,
        ctx,
        Action::Write,
        table,
        product_id.as_uuid(),
        serde_json::json!({ column: other.to_string() }),
    )
    .await
}

async fn unlink(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    table: &'static str,
    column: &'static str,
    product_id: ProductId,
    other: Uuid,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Product {
            id: Some(product_id.as_uuid()),
        },
    )?;

    let statement =
        format!("delete from {table} where scope = $1 and product_id = $2 and {column} = $3");

    let gone = sqlx::query(&statement)
        .bind(ctx.scope.0)
        .bind(product_id.as_uuid())
        .bind(other)
        .execute(&mut **tx)
        .await?
        .rows_affected();

    if gone == 0 {
        return Err(Error::not_found("link"));
    }

    note(
        tx,
        ctx,
        Action::Delete,
        table,
        product_id.as_uuid(),
        serde_json::json!({ column: other.to_string() }),
    )
    .await
}

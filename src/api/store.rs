//! What a storefront may ask.
//!
//! Everything here is reachable by somebody who is not signed in, or signed in
//! as a shopper. Two rules follow from that and are not negotiable per-route:
//! a draft product is invisible, and a customer only ever reaches their own
//! cart, their own orders and their own addresses.
//!
//! # Invisible, not forbidden
//!
//! Anything unpublished answers `not_found` rather than `denied`. Saying "you
//! may not see that" tells a stranger the shop has something they were not
//! shown, which is itself worth knowing to whoever is asking.
//!
//! # Ownership is asked, not assumed
//!
//! Nothing here compares a customer id to a row's own. It loads the row, then
//! hands the host's authorizer a [`Resource::Cart`] or [`Resource::Order`]
//! carrying whose it is. Whether that is allowed is the host's decision;
//! putting the right thing in front of it is this module's.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::id::{
    AddressId, CartId, CategoryId, CollectionId, CustomerId, LineItemId, OptionId, OrderId,
    PaymentCollectionId, PaymentSessionId, ProductId, ProductTagId, ProductTypeId, RegionId,
    SalesChannelId, ShippingOptionId, VariantId,
};
use crate::money::{Currency, Money};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, Actor, Ctx, Resource, Tx};
use crate::{
    cart, catalogue, checkout, customer, fulfilment, inventory, order, payment, pricing, store, tax,
};

use super::{Method, Route, Surface, own_cart, own_order, signed_in};

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

/// An amount as it leaves the building: the number and the code it is in,
/// never a bare decimal somebody has to guess the currency of.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoneyView {
    pub amount: Decimal,
    pub currency_code: String,
}

impl From<Money> for MoneyView {
    fn from(money: Money) -> Self {
        MoneyView {
            amount: money.amount,
            currency_code: money.currency.as_str().to_owned(),
        }
    }
}

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
    pub thumbnail_url: Option<String>,
    pub collection_id: Option<CollectionId>,
    pub type_id: Option<ProductTypeId>,
}

impl From<catalogue::Product> for ProductView {
    fn from(row: catalogue::Product) -> Self {
        ProductView {
            id: row.id,
            handle: row.handle,
            title: row.title,
            subtitle: row.subtitle,
            description: row.description,
            thumbnail_url: row.thumbnail_url,
            collection_id: row.product_collection_id,
            type_id: row.product_type_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantView {
    pub id: VariantId,
    pub product_id: ProductId,
    pub title: String,
    pub sku: Option<String>,
    pub barcode: Option<String>,
    pub allows_backorder: bool,
}

impl From<catalogue::ProductVariant> for VariantView {
    fn from(row: catalogue::ProductVariant) -> Self {
        VariantView {
            id: row.id,
            product_id: row.product_id,
            title: row.title,
            sku: row.sku,
            barcode: row.barcode,
            allows_backorder: row.allows_backorder,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionValueView {
    pub id: crate::id::OptionValueId,
    pub value: String,
    pub rank: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionView {
    pub id: OptionId,
    pub product_id: ProductId,
    pub title: String,
    pub rank: i32,
    pub values: Vec<OptionValueView>,
}

impl From<catalogue::OptionWithValues> for OptionView {
    fn from(row: catalogue::OptionWithValues) -> Self {
        OptionView {
            id: row.option.id,
            product_id: row.option.product_id,
            title: row.option.title,
            rank: row.option.rank,
            values: row
                .values
                .into_iter()
                .map(|value| OptionValueView {
                    id: value.id,
                    value: value.value,
                    rank: value.rank,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagView {
    pub id: ProductTagId,
    pub value: String,
}

impl From<catalogue::ProductTag> for TagView {
    fn from(row: catalogue::ProductTag) -> Self {
        TagView {
            id: row.id,
            value: row.value,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeView {
    pub id: ProductTypeId,
    pub value: String,
}

impl From<catalogue::ProductType> for TypeView {
    fn from(row: catalogue::ProductType) -> Self {
        TypeView {
            id: row.id,
            value: row.value,
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
}

impl From<catalogue::ProductCategory> for CategoryView {
    fn from(row: catalogue::ProductCategory) -> Self {
        CategoryView {
            id: row.id,
            parent_id: row.parent_id,
            name: row.name,
            handle: row.handle,
            description: row.description,
            rank: row.rank,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionView {
    pub id: CollectionId,
    pub handle: String,
    pub title: String,
}

impl From<catalogue::ProductCollection> for CollectionView {
    fn from(row: catalogue::ProductCollection) -> Self {
        CollectionView {
            id: row.id,
            handle: row.handle,
            title: row.title,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionView {
    pub id: RegionId,
    pub name: String,
    pub currency_code: String,
    pub is_tax_inclusive: bool,
}

impl From<store::Region> for RegionView {
    fn from(row: store::Region) -> Self {
        RegionView {
            id: row.id,
            name: row.name,
            currency_code: row.currency_code,
            is_tax_inclusive: row.is_tax_inclusive,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrencyView {
    pub code: String,
    pub symbol: String,
    pub name: String,
    pub exponent: i16,
}

impl From<store::CurrencyRow> for CurrencyView {
    fn from(row: store::CurrencyRow) -> Self {
        CurrencyView {
            code: row.code,
            symbol: row.symbol,
            name: row.name,
            exponent: row.exponent,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartView {
    pub id: CartId,
    pub customer_id: Option<CustomerId>,
    pub email: Option<String>,
    pub region_id: Option<RegionId>,
    pub currency_code: String,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<cart::Cart> for CartView {
    fn from(row: cart::Cart) -> Self {
        CartView {
            id: row.id,
            customer_id: row.customer_id,
            email: row.email,
            region_id: row.region_id,
            currency_code: row.currency_code,
            completed_at: row.completed_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineItemView {
    pub id: LineItemId,
    pub variant_id: Option<VariantId>,
    pub product_title: String,
    pub product_handle: Option<String>,
    pub variant_title: Option<String>,
    pub thumbnail: Option<String>,
    pub quantity: i32,
    pub unit_price: MoneyView,
    pub requires_shipping: bool,
}

impl LineItemView {
    fn of(row: cart::LineItem) -> Result<Self> {
        let currency = Currency::parse(&row.currency_code)?;
        Ok(LineItemView {
            id: row.id,
            variant_id: row.variant_id,
            product_title: row.product_title,
            product_handle: row.product_handle,
            variant_title: row.variant_title,
            thumbnail: row.thumbnail,
            quantity: row.quantity,
            unit_price: MoneyView::from(Money {
                amount: row.unit_price,
                currency,
            }),
            requires_shipping: row.requires_shipping,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShippingMethodView {
    pub id: crate::id::ShippingMethodId,
    pub shipping_option_id: Option<ShippingOptionId>,
    pub name: String,
    pub amount: MoneyView,
}

impl ShippingMethodView {
    fn of(row: cart::ShippingMethod) -> Result<Self> {
        let currency = Currency::parse(&row.currency_code)?;
        Ok(ShippingMethodView {
            id: row.id,
            shipping_option_id: row.shipping_option_id,
            name: row.name,
            amount: MoneyView::from(Money {
                amount: row.amount,
                currency,
            }),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TotalsView {
    pub subtotal: MoneyView,
    pub discount: MoneyView,
    pub shipping: MoneyView,
    pub tax: MoneyView,
    pub total: MoneyView,
}

impl From<cart::CartTotals> for TotalsView {
    fn from(row: cart::CartTotals) -> Self {
        TotalsView {
            subtotal: row.subtotal.into(),
            discount: row.discount.into(),
            shipping: row.shipping.into(),
            tax: row.tax.into(),
            total: row.total.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxLineView {
    pub line_id: Uuid,
    pub code: String,
    pub name: String,
    pub rate: Decimal,
    pub amount: MoneyView,
    pub is_tax_inclusive: bool,
}

impl From<tax::TaxLine> for TaxLineView {
    fn from(row: tax::TaxLine) -> Self {
        TaxLineView {
            line_id: row.line_id,
            code: row.code,
            name: row.name,
            rate: row.rate,
            amount: row.amount.into(),
            is_tax_inclusive: row.is_tax_inclusive,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderView {
    pub id: OrderId,
    pub display_id: Option<i64>,
    pub email: Option<String>,
    pub currency_code: String,
    pub status: String,
    pub fulfillment_status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<order::Order> for OrderView {
    fn from(row: order::Order) -> Self {
        OrderView {
            id: row.id,
            display_id: row.display_id,
            email: row.email,
            currency_code: row.currency_code,
            status: row.status,
            fulfillment_status: row.fulfillment_status,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerView {
    pub id: CustomerId,
    pub email: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
    pub company_name: Option<String>,
}

impl From<customer::Customer> for CustomerView {
    fn from(row: customer::Customer) -> Self {
        CustomerView {
            id: row.id,
            email: row.email,
            first_name: row.first_name,
            last_name: row.last_name,
            phone: row.phone,
            company_name: row.company_name,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressView {
    pub id: AddressId,
    pub label: Option<String>,
    pub is_default_shipping: bool,
    pub is_default_billing: bool,
    pub company: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub address_1: Option<String>,
    pub address_2: Option<String>,
    pub city: Option<String>,
    pub province: Option<String>,
    pub postal_code: Option<String>,
    pub country_code: Option<String>,
    pub phone: Option<String>,
}

impl From<customer::CustomerAddress> for AddressView {
    fn from(row: customer::CustomerAddress) -> Self {
        AddressView {
            id: row.id,
            label: row.label,
            is_default_shipping: row.is_default_shipping,
            is_default_billing: row.is_default_billing,
            company: row.company,
            first_name: row.first_name,
            last_name: row.last_name,
            address_1: row.address_1,
            address_2: row.address_2,
            city: row.city,
            province: row.province,
            postal_code: row.postal_code,
            country_code: row.country_code,
            phone: row.phone,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShippingOptionView {
    pub id: ShippingOptionId,
    pub name: String,
    pub price_type: String,
    /// Absent when the option is priced on request rather than from a list.
    pub amount: Option<MoneyView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculatedPriceView {
    pub calculated: MoneyView,
    pub original: MoneyView,
}

impl From<pricing::CalculatedPrice> for CalculatedPriceView {
    fn from(row: pricing::CalculatedPrice) -> Self {
        CalculatedPriceView {
            calculated: row.calculated.into(),
            original: row.original.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReturnReasonView {
    pub id: Uuid,
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

impl From<order::ReturnReason> for ReturnReasonView {
    fn from(row: order::ReturnReason) -> Self {
        ReturnReasonView {
            id: row.id,
            value: row.value,
            label: row.label,
            description: row.description,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReturnView {
    pub id: crate::id::ReturnId,
    pub order_id: OrderId,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<order::Return> for ReturnView {
    fn from(row: order::Return) -> Self {
        ReturnView {
            id: row.id,
            order_id: row.order_id,
            status: row.status,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentCollectionView {
    pub id: PaymentCollectionId,
    pub amount: MoneyView,
    pub status: String,
}

impl PaymentCollectionView {
    fn of(row: payment::PaymentCollection) -> Result<Self> {
        let currency = Currency::parse(&row.currency_code)?;
        Ok(PaymentCollectionView {
            id: row.id,
            amount: MoneyView::from(Money {
                amount: row.amount,
                currency,
            }),
            status: row.status,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentSessionView {
    pub id: PaymentSessionId,
    pub payment_collection_id: PaymentCollectionId,
    pub amount: MoneyView,
    pub status: String,
    /// Whatever the provider needs the shopper's browser to have. It is the
    /// provider's own shape and tezgah does not read it.
    pub data: serde_json::Value,
}

impl PaymentSessionView {
    fn of(row: payment::PaymentSession) -> Result<Self> {
        let currency = Currency::parse(&row.currency_code)?;
        Ok(PaymentSessionView {
            id: row.id,
            payment_collection_id: row.payment_collection_id,
            amount: MoneyView::from(Money {
                amount: row.amount,
                currency,
            }),
            status: row.status,
            data: row.data,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentProviderView {
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedView {
    /// Absent when the provider sent the shopper somewhere else first.
    pub order_id: Option<OrderId>,
    pub requires_more: bool,
}

// ---------------------------------------------------------------------------
// Shared input pieces
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddressInput {
    pub company: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub address_1: Option<String>,
    pub address_2: Option<String>,
    pub city: Option<String>,
    pub province: Option<String>,
    pub postal_code: Option<String>,
    pub country_code: Option<String>,
    pub phone: Option<String>,
}

impl From<AddressInput> for cart::CartAddress {
    fn from(input: AddressInput) -> Self {
        cart::CartAddress {
            company: input.company,
            first_name: input.first_name,
            last_name: input.last_name,
            address_1: input.address_1,
            address_2: input.address_2,
            city: input.city,
            province: input.province,
            postal_code: input.postal_code,
            country_code: input.country_code,
            phone: input.phone,
        }
    }
}

/// Where something is going, as a storefront asks about it before there is a
/// cart address to read.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryInput {
    pub country_code: String,
    pub province_code: Option<String>,
    pub city: Option<String>,
    pub postal_code: Option<String>,
    /// What the host knows about where the buyer is, beside what the cart
    /// says. A quote for a country abroad is refused without it.
    #[serde(default)]
    pub evidence: Vec<EvidenceInput>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn paging(after: Option<&str>, limit: Option<u32>) -> Result<Paging> {
    let limit = limit.unwrap_or(crate::page::DEFAULT_LIMIT);
    match after {
        Some(text) => Ok(Paging::after(Cursor::decode(text)?, limit)),
        None => Ok(Paging::first(limit)),
    }
}

/// The channels a storefront's publishable key may see, as plain ids for a
/// query rather than the rows themselves.
async fn visible_channels(tx: &mut Tx<'_>, ctx: &Ctx<'_>, token: &str) -> Result<Vec<Uuid>> {
    Ok(store::channels_for_token(tx, ctx, token)
        .await?
        .into_iter()
        .map(|channel| channel.id.as_uuid())
        .collect())
}

/// A product that is not published is not here, as far as a storefront is
/// concerned.
fn shown(row: catalogue::Product) -> Result<catalogue::Product> {
    if row.status != catalogue::ProductStatus::Published {
        return Err(Error::not_found("product"));
    }
    Ok(row)
}

async fn published(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductId) -> Result<catalogue::Product> {
    shown(catalogue::product(tx, ctx, id).await?)
}

/// A product this key's channel is allowed to see at all — published, and on
/// one of the channels the token is linked to (or on none, per
/// [`on_channel`]). Every read that takes a product id, a variant id or an
/// option id directly must go through this rather than [`published`] alone,
/// or a channel filter is only a listing filter.
async fn visible(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    token: &str,
    id: ProductId,
) -> Result<catalogue::Product> {
    let channels = visible_channels(tx, ctx, token).await?;
    let row = published(tx, ctx, id).await?;
    on_channel(tx, ctx, row, &channels).await
}

/// A product not linked to any of these channels — and linked to at least one
/// channel that is not among them — is not here either. A product linked to
/// no channel at all is unaffected, the same backward-compatibility rule
/// [`catalogue::ProductFilter::channels`] applies to a list.
async fn on_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    row: catalogue::Product,
    channels: &[Uuid],
) -> Result<catalogue::Product> {
    let linked = catalogue::channels_for_product(tx, ctx, row.id).await?;
    if linked.is_empty() || linked.iter().any(|c| channels.contains(&c.id.as_uuid())) {
        return Ok(row);
    }
    Err(Error::not_found("product"))
}

/// What a line item is worth in total, for tax and for shipping rules.
fn line_total(quantity: i32, unit_price: Decimal, currency: Currency) -> Money {
    Money {
        amount: unit_price * Decimal::from(quantity),
        currency,
    }
}

fn cart_currency(row: &cart::Cart) -> Result<Currency> {
    Currency::parse(&row.currency_code)
}

/// What a shipping option costs here, when it is priced from a list at all.
async fn option_price(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    option: ShippingOptionId,
    at: &pricing::PriceContext,
) -> Result<Option<Money>> {
    let Some(set) = pricing::price_set_for_shipping_option(tx, ctx, option).await? else {
        return Ok(None);
    };
    Ok(pricing::resolve(tx, ctx, set, at)
        .await?
        .map(|price| price.calculated))
}

/// What this crate can say about where the buyer is, each piece naming where
/// it came from.
///
/// Three at most, and never one dressed as two: the two addresses are separate
/// statements, and the region counts only where it covers a single country —
/// a region spanning twelve places nobody. A host that knows more — the
/// country a card was issued in, the country an address resolved to — hands
/// those in and they are kept beside these.
async fn evidence_for(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    holding: &cart::Cart,
    shipping: Option<String>,
    billing: Option<String>,
    host: Vec<tax::TaxEvidence>,
) -> Result<Vec<tax::TaxEvidence>> {
    let mut found = Vec::new();

    if let Some(country) = billing {
        found.push(tax::TaxEvidence::billing_address(country));
    }
    if let Some(country) = shipping {
        found.push(tax::TaxEvidence::shipping_address(country));
    }

    if let Some(region_id) = holding.region_id {
        let covered = store::region_countries(tx, ctx, region_id, Paging::first(2)).await?;
        if let [only] = covered.items.as_slice() {
            found.push(tax::TaxEvidence::region(only.iso_2.clone()));
        }
    }

    found.extend(host);
    Ok(found)
}

/// The buyer, as the tax decision needs them: what they are, what places them,
/// and where the goods leave from when the shop has not registered anywhere.
async fn subject_for(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    holding: &cart::Cart,
    evidence: Vec<tax::TaxEvidence>,
) -> Result<tax::TaxSubject> {
    let origin = inventory::origin_country(tx, ctx).await?;
    // A buyer who put a tax number on file is buying as a business; that is the
    // only signal a storefront has, and it is what the reverse charge turns on.
    let mut subject =
        tax::subject_for(tx, ctx, holding.customer_id, false, evidence, origin).await?;
    subject.is_business = !subject.tax_ids.is_empty();
    Ok(subject)
}

/// Whether the cart's currency is the one that country is served in.
///
/// Asked when an address is set rather than at the till: it is the moment both
/// halves are known and the last one at which the shopper can still start
/// again. A country under no region answers nothing — every shop running
/// today has an empty `region_country`, and a rule that refused there would
/// refuse every sale.
async fn currency_agrees_with(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    holding: &cart::Cart,
    country_code: &str,
) -> Result<()> {
    let Some(region) = store::region_for_country(tx, ctx, country_code).await? else {
        return Ok(());
    };

    if !region
        .currency_code
        .eq_ignore_ascii_case(&holding.currency_code)
    {
        return Err(Error::invalid(
            "that country is served in another currency; start a cart in it",
        ));
    }

    Ok(())
}

fn price_context(row: &cart::Cart, currency: Currency) -> pricing::PriceContext {
    pricing::PriceContext {
        currency,
        quantity: 1,
        region_id: row.region_id,
        customer_group_id: None,
        sales_channel_id: row.sales_channel_id.map(SalesChannelId::as_uuid),
        extra: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Catalogue
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListProducts {
    pub after: Option<String>,
    pub limit: Option<u32>,
    pub collection_id: Option<CollectionId>,
    pub category_id: Option<CategoryId>,
    pub type_id: Option<ProductTypeId>,
    pub tag_id: Option<ProductTagId>,
}

/// Published products only, and never anything else, whatever is asked for.
pub async fn list_products(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    token: &str,
    query: ListProducts,
) -> Result<Page<ProductView>> {
    let channels = visible_channels(tx, ctx, token).await?;

    let filter = catalogue::ProductFilter {
        status: Some(catalogue::ProductStatus::Published),
        collection: query.collection_id,
        category: query.category_id,
        product_type: query.type_id,
        tag: query.tag_id,
        channels: Some(channels),
    };

    let page = catalogue::products(
        tx,
        ctx,
        filter,
        paging(query.after.as_deref(), query.limit)?,
    )
    .await?;

    Ok(Page {
        items: page.items.into_iter().map(ProductView::from).collect(),
        next: page.next,
    })
}

pub async fn get_product(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    token: &str,
    handle: &str,
) -> Result<ProductView> {
    let channels = visible_channels(tx, ctx, token).await?;
    let row = shown(catalogue::product_by_handle(tx, ctx, handle).await?)?;
    Ok(ProductView::from(
        on_channel(tx, ctx, row, &channels).await?,
    ))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListVariants {
    pub product_id: ProductId,
    pub after: Option<String>,
    pub limit: Option<u32>,
}

pub async fn list_variants(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    token: &str,
    query: ListVariants,
) -> Result<Page<VariantView>> {
    visible(tx, ctx, token, query.product_id).await?;

    let page = catalogue::variants(
        tx,
        ctx,
        query.product_id,
        paging(query.after.as_deref(), query.limit)?,
    )
    .await?;

    Ok(Page {
        items: page.items.into_iter().map(VariantView::from).collect(),
        next: page.next,
    })
}

pub async fn get_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    token: &str,
    id: VariantId,
) -> Result<VariantView> {
    let row = catalogue::variant(tx, ctx, id).await?;
    visible(tx, ctx, token, row.product_id).await?;
    Ok(VariantView::from(row))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListOptions {
    pub product_id: ProductId,
}

pub async fn list_product_options(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    token: &str,
    query: ListOptions,
) -> Result<Vec<OptionView>> {
    visible(tx, ctx, token, query.product_id).await?;

    let found = catalogue::option_matrix(tx, ctx, query.product_id).await?;
    Ok(found.into_iter().map(OptionView::from).collect())
}

pub async fn get_product_option(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    token: &str,
    id: OptionId,
) -> Result<OptionView> {
    let found = catalogue::product_option(tx, ctx, id).await?;
    visible(tx, ctx, token, found.option.product_id).await?;
    Ok(OptionView::from(found))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListPage {
    pub after: Option<String>,
    pub limit: Option<u32>,
}

pub async fn list_product_tags(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListPage,
) -> Result<Page<TagView>> {
    let page = catalogue::tags(tx, ctx, paging(query.after.as_deref(), query.limit)?).await?;
    Ok(Page {
        items: page.items.into_iter().map(TagView::from).collect(),
        next: page.next,
    })
}

pub async fn get_product_tag(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ProductTagId) -> Result<TagView> {
    Ok(TagView::from(catalogue::product_tag(tx, ctx, id).await?))
}

pub async fn list_product_types(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListPage,
) -> Result<Page<TypeView>> {
    let page = catalogue::types(tx, ctx, paging(query.after.as_deref(), query.limit)?).await?;
    Ok(Page {
        items: page.items.into_iter().map(TypeView::from).collect(),
        next: page.next,
    })
}

pub async fn get_product_type(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ProductTypeId,
) -> Result<TypeView> {
    Ok(TypeView::from(catalogue::product_type(tx, ctx, id).await?))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListCategories {
    pub parent_id: Option<CategoryId>,
    pub after: Option<String>,
    pub limit: Option<u32>,
}

/// A category a shopper may browse: active, and not one of the ones the back
/// office keeps for itself.
fn browsable(row: &catalogue::ProductCategory) -> bool {
    row.is_active && !row.is_internal
}

pub async fn list_product_categories(
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

    // Filtered after the page is cut, so a page may come back short. The cursor
    // still points at the last row read, so nothing is skipped or repeated.
    Ok(Page {
        items: page
            .items
            .into_iter()
            .filter(browsable)
            .map(CategoryView::from)
            .collect(),
        next: page.next,
    })
}

pub async fn get_product_category(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CategoryId,
) -> Result<CategoryView> {
    let row = catalogue::category(tx, ctx, id).await?;
    if !browsable(&row) {
        return Err(Error::not_found("category"));
    }
    Ok(CategoryView::from(row))
}

pub async fn list_collections(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListPage,
) -> Result<Page<CollectionView>> {
    let page =
        catalogue::collections(tx, ctx, paging(query.after.as_deref(), query.limit)?).await?;
    Ok(Page {
        items: page.items.into_iter().map(CollectionView::from).collect(),
        next: page.next,
    })
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

// ---------------------------------------------------------------------------
// The shop itself
// ---------------------------------------------------------------------------

pub async fn list_regions(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListPage,
) -> Result<Page<RegionView>> {
    let page = store::regions(tx, ctx, paging(query.after.as_deref(), query.limit)?).await?;
    Ok(Page {
        items: page.items.into_iter().map(RegionView::from).collect(),
        next: page.next,
    })
}

pub async fn get_region(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: RegionId) -> Result<RegionView> {
    Ok(RegionView::from(store::region(tx, ctx, id).await?))
}

pub async fn list_currencies(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Vec<CurrencyView>> {
    let rows = store::currencies(tx, ctx).await?;
    Ok(rows.into_iter().map(CurrencyView::from).collect())
}

pub async fn get_currency(tx: &mut Tx<'_>, ctx: &Ctx<'_>, code: &str) -> Result<CurrencyView> {
    let wanted = Currency::parse(code)?;
    Ok(CurrencyView::from(store::currency(tx, ctx, wanted).await?))
}

pub async fn list_locales(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Vec<String>> {
    store::locales(tx, ctx).await
}

// ---------------------------------------------------------------------------
// Carts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCart {
    pub currency_code: String,
    pub region_id: Option<RegionId>,
    pub sales_channel_id: Option<SalesChannelId>,
    pub email: Option<String>,
}

/// A cart belongs to whoever is signed in, and to nobody when nobody is.
pub async fn create_cart(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    token: &str,
    input: CreateCart,
) -> Result<CartView> {
    let mine = match ctx.actor {
        Actor::Customer { id } => Some(CustomerId::from_uuid(id)),
        _ => None,
    };

    if let Some(wanted) = input.sales_channel_id {
        let channels = visible_channels(tx, ctx, token).await?;
        if !channels.contains(&wanted.as_uuid()) {
            return Err(Error::invalid(
                "that sales channel is not one this key may open a cart on",
            ));
        }
    }

    let made = cart::create(
        tx,
        ctx,
        cart::NewCart {
            customer_id: mine,
            email: input.email,
            currency_code: Currency::parse(&input.currency_code)?,
            region_id: input.region_id,
            sales_channel_id: input.sales_channel_id,
            expires_at: None,
            metadata: None,
        },
    )
    .await?;

    Ok(CartView::from(made))
}

pub async fn get_cart(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CartId) -> Result<CartView> {
    Ok(CartView::from(own_cart(tx, ctx, id, Action::View).await?))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateCart {
    pub email: Option<String>,
    pub shipping_address: Option<AddressInput>,
    pub billing_address: Option<AddressInput>,
}

pub async fn update_cart(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CartId,
    input: UpdateCart,
) -> Result<CartView> {
    own_cart(tx, ctx, id, Action::Write).await?;

    if let Some(email) = input.email.as_deref() {
        cart::set_email(tx, ctx, id, email).await?;
    }

    let moved = input.shipping_address.is_some() || input.billing_address.is_some();
    if moved {
        let holding = cart::get(tx, ctx, id).await?;
        let going = input
            .shipping_address
            .as_ref()
            .or(input.billing_address.as_ref())
            .and_then(|address| address.country_code.clone());
        if let Some(country) = going {
            currency_agrees_with(tx, ctx, &holding, &country).await?;
        }
    }

    let changed = if moved {
        cart::set_addresses(
            tx,
            ctx,
            id,
            input.shipping_address.map(cart::CartAddress::from),
            input.billing_address.map(cart::CartAddress::from),
        )
        .await?
    } else {
        cart::get(tx, ctx, id).await?
    };

    // An address is where tax comes from, so a new one is a new answer.
    if moved {
        reprice(tx, ctx, id).await?;
    }

    Ok(CartView::from(changed))
}

/// Hands a guest's cart to the customer who has just signed in.
pub async fn set_cart_customer(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CartId) -> Result<CartView> {
    let me = signed_in(ctx)?;
    own_cart(tx, ctx, id, Action::Write).await?;

    let kept = cart::transfer_to_customer(tx, ctx, id, me).await?;
    reprice(tx, ctx, kept.id).await?;

    Ok(CartView::from(kept))
}

pub async fn list_line_items(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CartId,
) -> Result<Vec<LineItemView>> {
    own_cart(tx, ctx, id, Action::View).await?;
    cart::lines(tx, ctx, id)
        .await?
        .into_iter()
        .map(LineItemView::of)
        .collect()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddLineItem {
    pub variant_id: VariantId,
    pub quantity: i32,
}

/// The price is the shop's, resolved here. A storefront that could send one
/// would be a storefront that could set it.
pub async fn add_line_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CartId,
    input: AddLineItem,
) -> Result<LineItemView> {
    let holding = own_cart(tx, ctx, id, Action::Write).await?;
    let currency = cart_currency(&holding)?;

    let variant = catalogue::variant(tx, ctx, input.variant_id).await?;
    published(tx, ctx, variant.product_id).await?;

    let at = pricing::PriceContext {
        quantity: input.quantity,
        ..price_context(&holding, currency)
    };
    let set = pricing::price_set_for_variant(tx, ctx, input.variant_id)
        .await?
        .ok_or_else(|| Error::invalid("that variant is not for sale here"))?;
    let price = pricing::resolve(tx, ctx, set, &at)
        .await?
        .ok_or_else(|| Error::invalid("that variant has no price in this currency"))?;

    let is_tax_inclusive = pricing::is_tax_inclusive(tx, ctx, &at).await?;

    let item = cart::add_line(
        tx,
        ctx,
        id,
        cart::AddLine {
            variant_id: input.variant_id,
            quantity: input.quantity,
            unit_price: price.calculated,
            is_tax_inclusive,
        },
    )
    .await?;

    reprice(tx, ctx, id).await?;

    LineItemView::of(item)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateLineItem {
    pub quantity: i32,
}

pub async fn update_line_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CartId,
    line_id: LineItemId,
    input: UpdateLineItem,
) -> Result<Option<LineItemView>> {
    own_cart(tx, ctx, id, Action::Write).await?;

    let changed = cart::update_line(tx, ctx, id, line_id, input.quantity).await?;
    reprice(tx, ctx, id).await?;

    match changed {
        Some(item) => LineItemView::of(item).map(Some),
        None => Ok(None),
    }
}

pub async fn remove_line_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CartId,
    line_id: LineItemId,
) -> Result<()> {
    own_cart(tx, ctx, id, Action::Write).await?;
    cart::remove_line(tx, ctx, id, line_id).await?;
    reprice(tx, ctx, id).await
}

/// Works the cart out again from what it now holds: the discounts, then the
/// tax on what is left.
///
/// It lives on this surface rather than in `cart` because it is the one layer
/// allowed to reach for `promotion` and `tax` at once; putting it in `cart`
/// would make the crate's module graph a cycle.
pub async fn reprice(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CartId) -> Result<()> {
    crate::promotion::apply(tx, ctx, id).await?;
    retax(tx, ctx, id, Vec::new()).await
}

/// One thing a host knows about where the buyer is and this crate does not:
/// the country a card was issued in, the country an address resolved to, the
/// country a telephone number belongs to.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceInput {
    pub source: String,
    pub country_code: String,
}

impl From<EvidenceInput> for tax::TaxEvidence {
    fn from(input: EvidenceInput) -> Self {
        tax::TaxEvidence::new(input.source, input.country_code)
    }
}

/// Prices the cart again with what the host knows added to what the cart says.
///
/// A distance sale to a consumer abroad is placed by two agreeing pieces from
/// different sources, and a cart carrying only one address has only one. This
/// is where the second comes from when the host has it.
pub async fn reprice_with_evidence(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CartId,
    evidence: Vec<EvidenceInput>,
) -> Result<TotalsView> {
    own_cart(tx, ctx, id, Action::Write).await?;

    crate::promotion::apply(tx, ctx, id).await?;
    retax(
        tx,
        ctx,
        id,
        evidence.into_iter().map(tax::TaxEvidence::from).collect(),
    )
    .await?;

    Ok(TotalsView::from(cart::totals(tx, ctx, id).await?))
}

fn variants(lines: &[cart::LineItem]) -> Vec<crate::id::VariantId> {
    lines.iter().filter_map(|line| line.variant_id).collect()
}

fn variant_ids(lines: &[cart::LineItem]) -> Vec<Uuid> {
    lines
        .iter()
        .filter_map(|line| line.variant_id.map(|id| id.as_uuid()))
        .collect()
}

/// No address, no jurisdiction: the tax lines go and the cart is untaxed until
/// somebody says where it is going. Checkout is what refuses to take money
/// without an address, not this.
async fn retax(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CartId,
    host: Vec<tax::TaxEvidence>,
) -> Result<()> {
    let holding = cart::get(tx, ctx, id).await?;
    let currency = cart_currency(&holding)?;

    let Some(to) = cart::delivery(tx, ctx, id).await? else {
        return tax::set_cart_tax_lines(tx, ctx, id, &[], &[]).await;
    };

    let address = tax::TaxableAddress {
        country_code: to.country_code,
        province_code: to.province_code,
        postal_code: to.postal_code,
    };

    // Where a line that ships nowhere is supplied. A parcel's destination is
    // not where an electronic service is taxed.
    let supplied = cart::place_of_supply(tx, ctx, id)
        .await?
        .map(|at| tax::TaxableAddress {
            country_code: at.country_code,
            province_code: at.province_code,
            postal_code: at.postal_code,
        });

    let seen = cart::countries(tx, ctx, id).await?;
    let evidence = evidence_for(tx, ctx, &holding, seen.shipping, seen.billing, host).await?;
    let subject = subject_for(tx, ctx, &holding, evidence).await?;

    let held = cart::lines(tx, ctx, id).await?;
    let codes = tax::tax_codes(tx, ctx, &variant_ids(&held)).await?;
    let facts = catalogue::line_facts(tx, ctx, &variants(&held)).await?;
    let items: Vec<tax::TaxableLine> = held
        .into_iter()
        // Selling a gift card is money changing form, not a supply: the tax is
        // due on what the card buys, and a line with no tax line carries none.
        .filter(|line| {
            !line
                .variant_id
                .is_some_and(|id| facts.get(&id).is_some_and(|f| f.is_giftcard))
        })
        .map(|line| tax::TaxableLine {
            id: line.id.as_uuid(),
            amount: line_total(line.quantity, line.unit_price, currency),
            targets: line
                .product_id
                .map(|product| tax::TaxTarget {
                    reference: tax::TaxReference::Product,
                    id: product,
                })
                .into_iter()
                .chain(line.variant_id.map(|variant| tax::TaxTarget {
                    reference: tax::TaxReference::Variant,
                    id: variant.as_uuid(),
                }))
                .collect(),
            tax_code: line
                .variant_id
                .and_then(|variant| codes.get(&variant.as_uuid()).cloned()),
            address: if line.requires_shipping {
                None
            } else {
                supplied.clone()
            },
        })
        .collect();

    let methods: Vec<tax::TaxableLine> = cart::shipping_methods(tx, ctx, id)
        .await?
        .into_iter()
        .map(|method| tax::TaxableLine {
            id: method.id.as_uuid(),
            amount: Money::new(method.amount, currency),
            targets: method
                .shipping_option_id
                .map(|option| {
                    vec![tax::TaxTarget {
                        reference: tax::TaxReference::ShippingOption,
                        id: option.as_uuid(),
                    }]
                })
                .unwrap_or_default(),
            tax_code: None,
            address: None,
        })
        .collect();

    let inclusive = pricing::is_tax_inclusive(tx, ctx, &price_context(&holding, currency)).await?;
    let on_items = tax::calculate(tx, ctx, &items, &address, Some(&subject), inclusive).await?;
    let on_methods = tax::calculate(tx, ctx, &methods, &address, Some(&subject), inclusive).await?;

    tax::set_cart_tax_lines(tx, ctx, id, &on_items, &on_methods).await
}

/// Applies whatever this cart now qualifies for and hands back what it costs.
pub async fn apply_promotions(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CartId) -> Result<TotalsView> {
    own_cart(tx, ctx, id, Action::Write).await?;
    reprice(tx, ctx, id).await?;
    Ok(TotalsView::from(cart::totals(tx, ctx, id).await?))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChooseShippingMethod {
    pub shipping_option_id: ShippingOptionId,
}

pub async fn set_shipping_method(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CartId,
    input: ChooseShippingMethod,
) -> Result<ShippingMethodView> {
    let holding = own_cart(tx, ctx, id, Action::Write).await?;
    let currency = cart_currency(&holding)?;
    let at = price_context(&holding, currency);

    let option = fulfilment::shipping_option(tx, ctx, input.shipping_option_id).await?;
    let amount = option_price(tx, ctx, input.shipping_option_id, &at)
        .await?
        .ok_or_else(|| Error::invalid("that shipping option has no price here"))?;

    let is_tax_inclusive = pricing::is_tax_inclusive(tx, ctx, &at).await?;

    let chosen = cart::set_shipping_method(
        tx,
        ctx,
        id,
        cart::NewShippingMethod {
            shipping_option_id: Some(option.id),
            name: option.name,
            description: None,
            amount,
            is_tax_inclusive,
            data: None,
        },
    )
    .await?;

    reprice(tx, ctx, id).await?;

    ShippingMethodView::of(chosen)
}

/// What tax this cart would carry when it is delivered there.
///
/// A quote rather than a write: nothing is owed until the cart is placed, and
/// a shopper trying three countries should not leave three sets of tax lines.
pub async fn quote_taxes(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CartId,
    input: DeliveryInput,
) -> Result<Vec<TaxLineView>> {
    let holding = own_cart(tx, ctx, id, Action::View).await?;
    let currency = cart_currency(&holding)?;

    let held = cart::lines(tx, ctx, id).await?;
    let codes = tax::tax_codes(tx, ctx, &variant_ids(&held)).await?;
    let lines: Vec<tax::TaxableLine> = held
        .into_iter()
        .map(|line| tax::TaxableLine {
            id: line.id.as_uuid(),
            amount: line_total(line.quantity, line.unit_price, currency),
            targets: line
                .product_id
                .map(|product| tax::TaxTarget {
                    reference: tax::TaxReference::Product,
                    id: product,
                })
                .into_iter()
                .chain(line.variant_id.map(|variant| tax::TaxTarget {
                    reference: tax::TaxReference::Variant,
                    id: variant.as_uuid(),
                }))
                .collect(),
            tax_code: line
                .variant_id
                .and_then(|variant| codes.get(&variant.as_uuid()).cloned()),
            address: None,
        })
        .collect();

    let address = tax::TaxableAddress {
        country_code: input.country_code,
        province_code: input.province_code,
        postal_code: None,
    };

    let seen = cart::countries(tx, ctx, id).await?;
    let evidence = evidence_for(
        tx,
        ctx,
        &holding,
        Some(address.country_code.clone()),
        seen.billing,
        input
            .evidence
            .into_iter()
            .map(tax::TaxEvidence::from)
            .collect(),
    )
    .await?;
    let subject = subject_for(tx, ctx, &holding, evidence).await?;

    let inclusive = pricing::is_tax_inclusive(tx, ctx, &price_context(&holding, currency)).await?;
    let found = tax::calculate(tx, ctx, &lines, &address, Some(&subject), inclusive).await?;

    Ok(found.into_iter().map(TaxLineView::from).collect())
}

/// Turns the cart into an order.
///
/// Takes the pool as well as the transaction because placing a cart is a
/// workflow: it opens a transaction per step so a step that finished stays
/// finished when a later one does not. The transaction here is what the
/// ownership question is asked in, before any of that starts.
pub async fn complete_cart(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CartId,
    how: &checkout::Checkout,
    pool: &PgPool,
) -> Result<CompletedView> {
    own_cart(tx, ctx, id, Action::Settle).await?;

    let placed = how.place(pool, ctx, id).await?;

    Ok(CompletedView {
        order_id: placed.order_id,
        requires_more: placed.requires_more,
    })
}

// ---------------------------------------------------------------------------
// Shipping options
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListShippingOptions {
    pub cart_id: CartId,
    pub country_code: String,
    pub province_code: Option<String>,
    pub city: Option<String>,
    pub postal_code: Option<String>,
}

pub async fn list_shipping_options(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListShippingOptions,
) -> Result<Vec<ShippingOptionView>> {
    let holding = own_cart(tx, ctx, query.cart_id, Action::View).await?;
    let currency = cart_currency(&holding)?;
    let at = price_context(&holding, currency);

    let items: Vec<fulfilment::Shippable> = cart::lines(tx, ctx, query.cart_id)
        .await?
        .into_iter()
        .map(|line| fulfilment::Shippable {
            id: line.id.as_uuid(),
            quantity: line.quantity,
            amount: line_total(line.quantity, line.unit_price, currency),
            shipping_profile_id: None,
            requires_shipping: line.requires_shipping,
        })
        .collect();

    let address = fulfilment::DeliveryAddress {
        country_code: query.country_code,
        province_code: query.province_code,
        city: query.city,
        postal_code: query.postal_code,
    };

    let found =
        fulfilment::options_for(tx, ctx, &address, holding.sales_channel_id, &items).await?;

    let mut out = Vec::with_capacity(found.len());
    for option in found {
        let amount = option_price(tx, ctx, option.id, &at).await?;
        out.push(ShippingOptionView {
            id: option.id,
            name: option.name,
            price_type: option.price_type,
            amount: amount.map(MoneyView::from),
        });
    }

    Ok(out)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculateShipping {
    pub cart_id: CartId,
}

pub async fn calculate_shipping_option(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ShippingOptionId,
    input: CalculateShipping,
) -> Result<MoneyView> {
    let holding = own_cart(tx, ctx, input.cart_id, Action::View).await?;
    let currency = cart_currency(&holding)?;

    option_price(tx, ctx, id, &price_context(&holding, currency))
        .await?
        .map(MoneyView::from)
        .ok_or_else(|| Error::not_found("shipping option price"))
}

// ---------------------------------------------------------------------------
// Customers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCustomer {
    pub email: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
    pub company_name: Option<String>,
}

pub async fn create_customer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: CreateCustomer,
) -> Result<CustomerView> {
    let made = customer::create(
        tx,
        ctx,
        customer::NewCustomer {
            email: Some(input.email),
            first_name: input.first_name,
            last_name: input.last_name,
            phone: input.phone,
            company_name: input.company_name,
            has_account: true,
            metadata: None,
        },
    )
    .await?;

    Ok(CustomerView::from(made))
}

pub async fn me(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<CustomerView> {
    let who = signed_in(ctx)?;
    Ok(CustomerView::from(customer::get(tx, ctx, who).await?))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateMe {
    pub email: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
    pub company_name: Option<String>,
}

pub async fn update_me(tx: &mut Tx<'_>, ctx: &Ctx<'_>, input: UpdateMe) -> Result<CustomerView> {
    let who = signed_in(ctx)?;

    let changed = customer::update(
        tx,
        ctx,
        who,
        customer::CustomerPatch {
            email: input.email,
            first_name: input.first_name,
            last_name: input.last_name,
            phone: input.phone,
            company_name: input.company_name,
            metadata: None,
        },
    )
    .await?;

    Ok(CustomerView::from(changed))
}

pub async fn list_my_addresses(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListPage,
) -> Result<Page<AddressView>> {
    let who = signed_in(ctx)?;

    let page =
        customer::addresses(tx, ctx, who, paging(query.after.as_deref(), query.limit)?).await?;

    Ok(Page {
        items: page.items.into_iter().map(AddressView::from).collect(),
        next: page.next,
    })
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteAddress {
    pub label: Option<String>,
    #[serde(default)]
    pub is_default_shipping: bool,
    #[serde(default)]
    pub is_default_billing: bool,
    pub company: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub address_1: Option<String>,
    pub address_2: Option<String>,
    pub city: Option<String>,
    pub province: Option<String>,
    pub postal_code: Option<String>,
    pub country_code: Option<String>,
    pub phone: Option<String>,
}

impl From<WriteAddress> for customer::NewAddress {
    fn from(input: WriteAddress) -> Self {
        customer::NewAddress {
            label: input.label,
            is_default_shipping: input.is_default_shipping,
            is_default_billing: input.is_default_billing,
            company: input.company,
            first_name: input.first_name,
            last_name: input.last_name,
            address_1: input.address_1,
            address_2: input.address_2,
            city: input.city,
            province: input.province,
            postal_code: input.postal_code,
            country_code: input.country_code,
            phone: input.phone,
            metadata: None,
        }
    }
}

pub async fn add_my_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: WriteAddress,
) -> Result<AddressView> {
    let who = signed_in(ctx)?;
    let made = customer::add_address(tx, ctx, who, input.into()).await?;
    Ok(AddressView::from(made))
}

/// The address is loaded before it is written, so the host is asked about the
/// customer it actually belongs to rather than the one who asked.
async fn my_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: AddressId,
    action: Action,
) -> Result<customer::CustomerAddress> {
    signed_in(ctx)?;

    let found = customer::address(tx, ctx, id).await?;
    ctx.permit(
        action,
        Resource::Customer {
            id: Some(found.customer_id.as_uuid()),
        },
    )?;
    Ok(found)
}

pub async fn update_my_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: AddressId,
    input: WriteAddress,
) -> Result<AddressView> {
    my_address(tx, ctx, id, Action::Write).await?;
    let changed = customer::update_address(tx, ctx, id, input.into()).await?;
    Ok(AddressView::from(changed))
}

pub async fn delete_my_address(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: AddressId) -> Result<()> {
    my_address(tx, ctx, id, Action::Delete).await?;
    customer::delete_address(tx, ctx, id).await
}

// ---------------------------------------------------------------------------
// Orders
// ---------------------------------------------------------------------------

pub async fn list_my_orders(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListPage,
) -> Result<Page<OrderView>> {
    let who = signed_in(ctx)?;

    // No id yet: the question is about the orders of one customer rather than
    // about one order, and the actor's own id is what answers it.
    ctx.permit(
        Action::View,
        Resource::Order {
            id: Uuid::nil(),
            customer: Some(who.as_uuid()),
        },
    )?;

    let page = order::list(
        tx,
        ctx,
        Some(who),
        Some(false),
        paging(query.after.as_deref(), query.limit)?,
    )
    .await?;

    Ok(Page {
        items: page.items.into_iter().map(OrderView::from).collect(),
        next: page.next,
    })
}

pub async fn get_my_order(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderId) -> Result<OrderView> {
    let found = own_order(tx, ctx, id, Action::View).await?;
    if found.is_draft {
        // A draft is the back office's working copy, not a shopper's order.
        return Err(Error::not_found("order"));
    }
    Ok(OrderView::from(found))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferView {
    pub id: crate::id::OrderTransferId,
    pub order_id: OrderId,
    pub to_email: String,
    pub status: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

impl From<order::OrderTransfer> for TransferView {
    fn from(row: order::OrderTransfer) -> Self {
        TransferView {
            id: row.id,
            order_id: row.order_id,
            to_email: row.to_email,
            status: row.status,
            expires_at: row.expires_at,
        }
    }
}

/// The token is here and nowhere else: it is not stored and this response is
/// the only time it can be read.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestedTransferView {
    pub transfer: TransferView,
    pub token: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestTransfer {
    pub to_email: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimTransfer {
    pub token: String,
}

pub async fn request_transfer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    input: RequestTransfer,
) -> Result<RequestedTransferView> {
    own_order(tx, ctx, id, Action::Write).await?;

    let made = order::request_transfer(tx, ctx, id, input.to_email, input.expires_at).await?;
    Ok(RequestedTransferView {
        transfer: TransferView::from(made.transfer),
        token: made.token,
    })
}

/// No `own_order` here: the order is somebody else's until this succeeds, and
/// the token is what says the asker was offered it.
pub async fn accept_transfer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    input: ClaimTransfer,
) -> Result<OrderView> {
    let who = signed_in(ctx)?;
    Ok(OrderView::from(
        order::accept_transfer(tx, ctx, id, &input.token, who).await?,
    ))
}

pub async fn decline_transfer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    input: ClaimTransfer,
) -> Result<TransferView> {
    Ok(TransferView::from(
        order::decline_transfer(tx, ctx, id, &input.token).await?,
    ))
}

pub async fn cancel_transfer(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderId) -> Result<TransferView> {
    own_order(tx, ctx, id, Action::Write).await?;
    Ok(TransferView::from(
        order::cancel_transfer(tx, ctx, id).await?,
    ))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnLineInput {
    pub order_line_item_id: LineItemId,
    pub quantity: i32,
    pub return_reason_id: Option<Uuid>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestReturn {
    pub order_id: OrderId,
    pub lines: Vec<ReturnLineInput>,
}

/// Where the goods come back to is the shop's decision, not the shopper's.
pub async fn request_return(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: RequestReturn,
) -> Result<ReturnView> {
    own_order(tx, ctx, input.order_id, Action::Write).await?;

    let lines = input
        .lines
        .into_iter()
        .map(|line| order::ReturnLine {
            order_line_item_id: line.order_line_item_id,
            quantity: line.quantity,
            return_reason_id: line.return_reason_id,
            note: line.note,
        })
        .collect();

    let made = order::request_return(tx, ctx, input.order_id, None, lines).await?;
    Ok(ReturnView::from(made))
}

pub async fn list_return_reasons(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListPage,
) -> Result<Page<ReturnReasonView>> {
    let page = order::return_reasons(tx, ctx, paging(query.after.as_deref(), query.limit)?).await?;
    Ok(Page {
        items: page.items.into_iter().map(ReturnReasonView::from).collect(),
        next: page.next,
    })
}

pub async fn get_return_reason(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: Uuid,
) -> Result<ReturnReasonView> {
    Ok(ReturnReasonView::from(
        order::return_reason(tx, ctx, id).await?,
    ))
}

// ---------------------------------------------------------------------------
// Payment
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartPayment {
    pub cart_id: CartId,
}

/// The amount is the cart's, never the caller's: a storefront that could name
/// the sum is a storefront that could name a smaller one.
pub async fn create_payment_collection(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: StartPayment,
) -> Result<PaymentCollectionView> {
    own_cart(tx, ctx, input.cart_id, Action::Write).await?;

    let owed = cart::totals(tx, ctx, input.cart_id).await?;
    let made = payment::create_collection(
        tx,
        ctx,
        payment::NewCollection {
            amount: owed.total,
            cart_id: Some(input.cart_id),
            metadata: None,
        },
    )
    .await?;

    PaymentCollectionView::of(made)
}

/// The cart comes with the request because it is what ownership is asked
/// about: a payment collection on its own says nothing about whose it is.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartPaymentSession {
    pub cart_id: CartId,
    pub provider_code: String,
    pub context: Option<serde_json::Value>,
}

pub async fn create_payment_session(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    collection_id: PaymentCollectionId,
    input: StartPaymentSession,
) -> Result<PaymentSessionView> {
    own_cart(tx, ctx, input.cart_id, Action::Write).await?;

    let collection = payment::collection(tx, ctx, collection_id).await?;
    if collection.cart_id != Some(input.cart_id) {
        return Err(Error::not_found("payment collection"));
    }

    let currency = Currency::parse(&collection.currency_code)?;

    let made = payment::create_session(
        tx,
        ctx,
        payment::NewSession {
            collection_id,
            provider_code: input.provider_code,
            amount: Money {
                amount: collection.amount,
                currency,
            },
            context: input.context,
            installment_count: None,
        },
    )
    .await?;

    PaymentSessionView::of(made)
}

/// Only what a shopper may actually pay with.
pub async fn list_payment_providers(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
) -> Result<Vec<PaymentProviderView>> {
    let found = payment::providers(tx, ctx).await?;
    Ok(found
        .into_iter()
        .filter(|provider| provider.is_enabled)
        .map(|provider| PaymentProviderView {
            code: provider.code,
        })
        .collect())
}

// ---------------------------------------------------------------------------
// The table
// ---------------------------------------------------------------------------

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
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/product-variants",
        action: Action::View,
        domain: "catalogue",
        summary: "List the variants of one published product",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/product-variants/{id}",
        action: Action::View,
        domain: "catalogue",
        summary: "Fetch one variant of a published product",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/product-options",
        action: Action::View,
        domain: "catalogue",
        summary: "List the options of one published product",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/product-options/{id}",
        action: Action::View,
        domain: "catalogue",
        summary: "Fetch one option and its values",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/product-tags",
        action: Action::View,
        domain: "catalogue",
        summary: "List product tags",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/product-tags/{id}",
        action: Action::View,
        domain: "catalogue",
        summary: "Fetch one product tag",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/product-types",
        action: Action::View,
        domain: "catalogue",
        summary: "List product types",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/product-types/{id}",
        action: Action::View,
        domain: "catalogue",
        summary: "Fetch one product type",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/product-categories",
        action: Action::View,
        domain: "catalogue",
        summary: "List the categories a shopper may browse",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/product-categories/{id}",
        action: Action::View,
        domain: "catalogue",
        summary: "Fetch one browsable category",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/collections",
        action: Action::View,
        domain: "catalogue",
        summary: "List product collections",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/collections/{id}",
        action: Action::View,
        domain: "catalogue",
        summary: "Fetch one product collection",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/regions",
        action: Action::View,
        domain: "store",
        summary: "List the regions the shop sells into",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/regions/{id}",
        action: Action::View,
        domain: "store",
        summary: "Fetch one region",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/currencies",
        action: Action::View,
        domain: "store",
        summary: "List the currencies the shop trades in",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/currencies/{code}",
        action: Action::View,
        domain: "store",
        summary: "Fetch one currency and its exponent",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/locales",
        action: Action::View,
        domain: "store",
        summary: "List the languages the shop is served in",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/carts",
        action: Action::Write,
        domain: "cart",
        summary: "Start a cart",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/carts/{id}",
        action: Action::View,
        domain: "cart",
        summary: "Fetch one's own cart",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/carts/{id}",
        action: Action::Write,
        domain: "cart",
        summary: "Set a cart's e-mail address or addresses",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/carts/{id}/complete",
        action: Action::Write,
        domain: "cart",
        summary: "Place the cart as an order",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/carts/{id}/customer",
        action: Action::Write,
        domain: "cart",
        summary: "Hand a guest cart to the customer who signed in",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/carts/{id}/line-items",
        action: Action::View,
        domain: "cart",
        summary: "List what is in the cart",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/carts/{id}/line-items",
        action: Action::Write,
        domain: "cart",
        summary: "Put a variant in the cart at the shop's price",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/carts/{id}/line-items/{line_id}",
        action: Action::Write,
        domain: "cart",
        summary: "Change how many of a line are wanted",
    },
    Route {
        surface: Surface::Store,
        method: Method::Delete,
        path: "/store/carts/{id}/line-items/{line_id}",
        action: Action::Delete,
        domain: "cart",
        summary: "Take a line out of the cart",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/carts/{id}/promotions",
        action: Action::Write,
        domain: "promotion",
        summary: "Apply what the cart now qualifies for",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/carts/{id}/shipping-methods",
        action: Action::Write,
        domain: "cart",
        summary: "Choose how the cart is delivered",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/carts/{id}/taxes",
        action: Action::Write,
        domain: "tax",
        summary: "Quote the tax the cart would carry to an address",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/carts/{id}/tax-evidence",
        action: Action::Write,
        domain: "tax",
        summary: "Price the cart again with what the host knows about where the buyer is",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/shipping-options",
        action: Action::View,
        domain: "fulfilment",
        summary: "List what can deliver this cart to an address",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/shipping-options/{id}/calculate",
        action: Action::Write,
        domain: "fulfilment",
        summary: "Price one shipping option for a cart",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/customers",
        action: Action::Write,
        domain: "customer",
        summary: "Register",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/customers/me",
        action: Action::View,
        domain: "customer",
        summary: "Fetch one's own account",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/customers/me",
        action: Action::Write,
        domain: "customer",
        summary: "Change one's own account",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/customers/me/addresses",
        action: Action::View,
        domain: "customer",
        summary: "List one's own addresses",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/customers/me/addresses",
        action: Action::Write,
        domain: "customer",
        summary: "Add an address of one's own",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/customers/me/addresses/{address_id}",
        action: Action::Write,
        domain: "customer",
        summary: "Change an address of one's own",
    },
    Route {
        surface: Surface::Store,
        method: Method::Delete,
        path: "/store/customers/me/addresses/{address_id}",
        action: Action::Delete,
        domain: "customer",
        summary: "Remove an address of one's own",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/orders",
        action: Action::View,
        domain: "order",
        summary: "List one's own orders",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/orders/{id}",
        action: Action::View,
        domain: "order",
        summary: "Fetch one of one's own orders",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/orders/{id}/transfer/request",
        action: Action::Write,
        domain: "order",
        summary: "Offer one of one's own orders to somebody else",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/orders/{id}/transfer/accept",
        action: Action::Write,
        domain: "order",
        summary: "Take over an order one was offered",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/orders/{id}/transfer/decline",
        action: Action::Write,
        domain: "order",
        summary: "Refuse an order one was offered",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/orders/{id}/transfer/cancel",
        action: Action::Write,
        domain: "order",
        summary: "Withdraw an offer of one's own order",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/returns",
        action: Action::Write,
        domain: "order",
        summary: "Ask to send something back",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/return-reasons",
        action: Action::View,
        domain: "order",
        summary: "List the reasons a return may be given",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/return-reasons/{id}",
        action: Action::View,
        domain: "order",
        summary: "Fetch one return reason",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/payment-collections",
        action: Action::Write,
        domain: "payment",
        summary: "Open a payment collection for what a cart owes",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/payment-collections/{id}/payment-sessions",
        action: Action::Write,
        domain: "payment",
        summary: "Start a session with one payment provider",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/payment-providers",
        action: Action::View,
        domain: "payment",
        summary: "List the providers a shopper may pay with",
    },
];

//! The cart: what somebody is about to buy, and what it comes to.
//!
//! A line item is a snapshot rather than a pointer. Retiring a product, or
//! changing its price, must not silently change what a shopper was looking at,
//! so the title, the sku, the options and the price are copied in at the
//! moment of adding.
//!
//! [`totals`] is the only place a cart is added up, and [`compute`] is that
//! sum with the database taken out of it, so the order side can reach the same
//! answer from the same code.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::error::{Error, Result};
use crate::id::{
    CartId, CustomerId, LineItemId, OrderBasketId, RegionId, SalesChannelId, ShippingMethodId,
    ShippingOptionId, VariantId,
};
use crate::money::{Currency, Money};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, AuditEntry, Ctx, Event, Permit, Resource, Tx};

const COLUMNS: &str = "id, customer_id, email, region_id, currency_code, sales_channel_id, \
                       shipping_address_id, billing_address_id, basket_id, completed_at, \
                       expires_at, metadata, created_at, updated_at";

const LINE_COLUMNS: &str = "id, cart_id, variant_id, product_id, product_title, product_handle, \
                            variant_title, variant_sku, variant_barcode, variant_option_values, \
                            thumbnail, quantity, unit_price, compare_at_unit_price, \
                            currency_code, is_tax_inclusive, is_discountable, requires_shipping, \
                            parent_line_item_id, selling_plan_id, metadata, created_at, updated_at";

/// Most line items one cart may be read with. Every caller here totals a
/// whole cart, so this is a ceiling rather than a page: a cursor would hand
/// checkout a partial cart and it would price it as if that were all of it.
pub const MAX_LINES: i64 = 200;

/// Most carts one sweep deletes. A caller runs it again until it comes back
/// short.
pub const MAX_BATCH: i64 = 500;

/// One cart chooses one shipping, and the ceiling is here for the same reason
/// [`MAX_LINES`] is: whoever adds a cart up wants all of it.
pub const MAX_SHIPPING_METHODS: i64 = 50;

const METHOD_COLUMNS: &str = "id, cart_id, shipping_option_id, name, description, amount, \
                              currency_code, is_tax_inclusive, data, metadata, created_at, \
                              updated_at";

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Cart {
    pub id: CartId,
    pub customer_id: Option<CustomerId>,
    pub email: Option<String>,
    pub region_id: Option<RegionId>,
    pub currency_code: String,
    pub sales_channel_id: Option<SalesChannelId>,
    pub shipping_address_id: Option<uuid::Uuid>,
    pub billing_address_id: Option<uuid::Uuid>,
    /// The basket this cart is one seller-scope's leg of, if a checkout
    /// spanning more than one seller joins it under one. `None` for a
    /// single-seller shop, forever.
    pub basket_id: Option<OrderBasketId>,
    /// Set when the cart became an order; every write after that is refused.
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Cart {
    pub fn currency(&self) -> Result<Currency> {
        Currency::parse(&self.currency_code)
    }

    pub fn is_complete(&self) -> bool {
        self.completed_at.is_some()
    }
}

#[derive(Debug, Clone)]
pub struct NewCart {
    pub customer_id: Option<CustomerId>,
    pub email: Option<String>,
    pub currency_code: Currency,
    pub region_id: Option<RegionId>,
    pub sales_channel_id: Option<SalesChannelId>,
    /// The marketplace basket this cart is one seller-scope's leg of. Set by
    /// a host that already opened the basket under its own scope's `Ctx` —
    /// tezgah never opens one on a cart's behalf, the same way it never picks
    /// which scope a checkout runs under.
    pub basket_id: Option<OrderBasketId>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: Option<serde_json::Value>,
}

impl NewCart {
    pub fn guest(currency_code: Currency) -> Self {
        NewCart {
            customer_id: None,
            email: None,
            currency_code,
            region_id: None,
            sales_channel_id: None,
            basket_id: None,
            expires_at: None,
            metadata: None,
        }
    }

    pub fn of(customer_id: CustomerId, currency_code: Currency) -> Self {
        NewCart {
            customer_id: Some(customer_id),
            ..NewCart::guest(currency_code)
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct LineItem {
    pub id: LineItemId,
    pub cart_id: CartId,
    pub variant_id: Option<VariantId>,
    pub product_id: Option<uuid::Uuid>,
    pub product_title: String,
    pub product_handle: Option<String>,
    pub variant_title: Option<String>,
    pub variant_sku: Option<String>,
    pub variant_barcode: Option<String>,
    pub variant_option_values: Option<serde_json::Value>,
    pub thumbnail: Option<String>,
    pub quantity: i32,
    pub unit_price: Decimal,
    pub compare_at_unit_price: Option<Decimal>,
    pub currency_code: String,
    pub is_tax_inclusive: bool,
    pub is_discountable: bool,
    pub requires_shipping: bool,
    /// The bundle line this one is a component of, when it is one. `cart::
    /// add_bundle` is what sets it; an ordinary line never has one.
    pub parent_line_item_id: Option<LineItemId>,
    /// The plan this line is sold on, when it is a subscription's. `checkout`'s
    /// `create_subscriptions` step is what a contract is opened from.
    pub selling_plan_id: Option<crate::id::SellingPlanId>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// What to put in the cart. The price is passed in rather than looked up:
/// resolving one needs a region, a customer's groups and a price list, and
/// that answer belongs to pricing rather than to the cart.
#[derive(Debug, Clone)]
pub struct AddLine {
    pub variant_id: VariantId,
    pub quantity: i32,
    pub unit_price: Money,
    pub is_tax_inclusive: bool,
    /// Bought on a plan rather than once. `selling_plan::attach_variant` is
    /// what makes a variant eligible; this does not check it, the same as
    /// `unit_price` above is not looked up here.
    pub selling_plan_id: Option<crate::id::SellingPlanId>,
}

/// One component of a bundle being added, already priced: `unit_price` is the
/// share of the bundle's price `pricing::resolve_bundle` allocated to it, not
/// the component's own list price.
#[derive(Debug, Clone)]
pub struct AddBundleComponent {
    pub variant_id: VariantId,
    pub quantity: i32,
    pub unit_price: Money,
}

/// A bundle being added: one parent line and its components, priced the same
/// way — resolved by `pricing::resolve_bundle` before the cart is touched.
#[derive(Debug, Clone)]
pub struct AddBundle {
    pub variant_id: VariantId,
    pub quantity: i32,
    pub is_tax_inclusive: bool,
    pub components: Vec<AddBundleComponent>,
}

#[derive(Debug, Clone, Default)]
pub struct CartAddress {
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

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ShippingMethod {
    pub id: ShippingMethodId,
    pub cart_id: CartId,
    pub shipping_option_id: Option<ShippingOptionId>,
    pub name: String,
    pub description: Option<String>,
    pub amount: Decimal,
    pub currency_code: String,
    pub is_tax_inclusive: bool,
    pub data: Option<serde_json::Value>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct NewShippingMethod {
    pub shipping_option_id: Option<ShippingOptionId>,
    pub name: String,
    pub description: Option<String>,
    pub amount: Money,
    pub is_tax_inclusive: bool,
    pub data: Option<serde_json::Value>,
}

/// One line, flattened to what adding up needs.
#[derive(Debug, Clone, FromRow)]
pub struct TotalsLine {
    pub quantity: i32,
    pub unit_price: Decimal,
    pub is_tax_inclusive: bool,
    /// Everything the promotions took off this line, already summed.
    pub discount: Decimal,
    /// The sum of the line's tax rates, as a percentage: 18 is eighteen percent.
    pub tax_rate: Decimal,
}

#[derive(Debug, Clone, FromRow)]
pub struct TotalsShipping {
    pub amount: Decimal,
    pub is_tax_inclusive: bool,
    pub discount: Decimal,
    pub tax_rate: Decimal,
}

/// What a cart comes to. `total` is `subtotal - discount + shipping + tax`,
/// each part rounded once, so the parts a customer is shown add up to what the
/// provider is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CartTotals {
    pub subtotal: Money,
    pub discount: Money,
    pub shipping: Money,
    pub tax: Money,
    pub total: Money,
}

const HUNDRED: Decimal = Decimal::ONE_HUNDRED;

/// The whole of the arithmetic, with no database in it.
///
/// A tax-inclusive amount carries its tax inside it, so the net is recovered
/// before anything is added: mixing the two is how a total stops matching the
/// sum of its lines.
pub fn compute(
    lines: &[TotalsLine],
    shipping: &[TotalsShipping],
    currency: Currency,
    exponent: u32,
) -> Result<CartTotals> {
    let mut subtotal = Decimal::ZERO;
    let mut discount = Decimal::ZERO;
    let mut shipping_total = Decimal::ZERO;
    let mut tax = Decimal::ZERO;

    for line in lines {
        if line.quantity < 0 {
            return Err(Error::bug("a line with a negative quantity was added up"));
        }
        let gross = line.unit_price * Decimal::from(line.quantity);
        let (net, off) = net_of(gross, line.discount, line.is_tax_inclusive, line.tax_rate);
        subtotal += net;
        discount += off;
        tax += taxable(net, off) * line.tax_rate / HUNDRED;
    }

    for method in shipping {
        let (net, off) = net_of(
            method.amount,
            method.discount,
            method.is_tax_inclusive,
            method.tax_rate,
        );
        shipping_total += net;
        discount += off;
        tax += taxable(net, off) * method.tax_rate / HUNDRED;
    }

    let subtotal = subtotal.round_dp(exponent);
    let discount = discount.round_dp(exponent);
    let shipping_total = shipping_total.round_dp(exponent);
    let tax = tax.round_dp(exponent);
    let total = subtotal - discount + shipping_total + tax;

    Ok(CartTotals {
        subtotal: Money::new(subtotal, currency),
        discount: Money::new(discount, currency),
        shipping: Money::new(shipping_total, currency),
        tax: Money::new(tax, currency),
        total: Money::new(total, currency),
    })
}

fn net_of(gross: Decimal, discount: Decimal, inclusive: bool, rate: Decimal) -> (Decimal, Decimal) {
    if !inclusive {
        return (gross, discount);
    }
    let divisor = Decimal::ONE + rate / HUNDRED;
    if divisor.is_zero() {
        return (gross, discount);
    }
    (gross / divisor, discount / divisor)
}

fn taxable(net: Decimal, discount: Decimal) -> Decimal {
    let base = net - discount;
    if base.is_sign_negative() {
        Decimal::ZERO
    } else {
        base
    }
}

pub async fn create(tx: &mut Tx<'_>, ctx: &Ctx<'_>, new: NewCart) -> Result<Cart> {
    let id = CartId::new();
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Cart {
            id: id.as_uuid(),
            customer: new.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    let cart = sqlx::query_as::<_, Cart>(&format!(
        "insert into cart
             (id, scope, customer_id, email, region_id, currency_code, sales_channel_id,
              basket_id, expires_at, metadata)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         returning {COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.customer_id.map(CustomerId::as_uuid))
    .bind(new.email.map(|value| value.trim().to_lowercase()))
    .bind(new.region_id.map(RegionId::as_uuid))
    .bind(new.currency_code.as_str())
    .bind(new.sales_channel_id.map(SalesChannelId::as_uuid))
    .bind(new.basket_id.map(OrderBasketId::as_uuid))
    .bind(new.expires_at)
    .bind(new.metadata)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "cart",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "currency": cart.currency_code }),
        },
    )
    .await?;

    Ok(cart)
}

pub async fn get(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CartId) -> Result<Cart> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Cart {
            id: id.as_uuid(),
            customer: None,
        },
    )?;

    load(tx, ctx, id).await
}

pub async fn list(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: Option<CustomerId>,
    paging: Paging,
) -> Result<Page<Cart>> {
    let _: Permit = ctx.permit(Action::View, Resource::Customer { id: None })?;

    let rows = sqlx::query_as::<_, Cart>(&format!(
        "select {COLUMNS} from cart
         where scope = $1
           and ($2::uuid is null or customer_id = $2)
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(customer_id.map(CustomerId::as_uuid))
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

/// This scope's own carts under one basket — the entry point a host's own
/// multi-scope checkout orchestration calls, once per seller-scope it knows
/// about, to find that scope's leg of a shared checkout. Not the basket's
/// whole cross-seller cart: a connection scoped to one seller cannot read
/// another's, by the same row-level security [`crate::order_basket::orders`]
/// answers the same question for orders rather than carts.
pub async fn for_basket(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    basket_id: OrderBasketId,
    paging: Paging,
) -> Result<Page<Cart>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Cart {
            id: basket_id.as_uuid(),
            customer: None,
        },
    )?;

    let rows = sqlx::query_as::<_, Cart>(&format!(
        "select {COLUMNS} from cart
         where scope = $1
           and basket_id = $2
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(basket_id.as_uuid())
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

pub async fn lines(tx: &mut Tx<'_>, ctx: &Ctx<'_>, cart_id: CartId) -> Result<Vec<LineItem>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Cart {
            id: cart_id.as_uuid(),
            customer: None,
        },
    )?;

    let rows = sqlx::query_as::<_, LineItem>(&format!(
        "select {LINE_COLUMNS} from cart_line_item
         where scope = $1 and cart_id = $2
         order by created_at, id
         limit $3"
    ))
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .bind(MAX_LINES)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

/// The chosen shipping, at most [`MAX_SHIPPING_METHODS`] of it.
pub async fn shipping_methods(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
) -> Result<Vec<ShippingMethod>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Cart {
            id: cart_id.as_uuid(),
            customer: None,
        },
    )?;

    let rows = sqlx::query_as::<_, ShippingMethod>(&format!(
        "select {METHOD_COLUMNS} from cart_shipping_method
         where scope = $1 and cart_id = $2
         order by created_at, id
         limit $3"
    ))
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .bind(MAX_SHIPPING_METHODS)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

/// Where this cart is going, as far as anything outside the cart cares.
#[derive(Debug, Clone)]
pub struct Delivery {
    pub country_code: String,
    pub province_code: Option<String>,
    /// A United States rate is decided below the state, and the postcode is
    /// what a provider is given to find the county and the city.
    pub postal_code: Option<String>,
}

/// The address a cart is judged by: the shipping one, or the billing one for a
/// cart that ships nothing. `None` when neither names a country.
pub async fn delivery(tx: &mut Tx<'_>, ctx: &Ctx<'_>, cart_id: CartId) -> Result<Option<Delivery>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Cart {
            id: cart_id.as_uuid(),
            customer: None,
        },
    )?;

    address_of(
        tx,
        ctx,
        cart_id,
        "coalesce(c.shipping_address_id, c.billing_address_id)",
    )
    .await
}

/// Where a line that ships nowhere is supplied: the billing address, or the
/// shipping one where there is no billing address. An electronic service is
/// taxed where the buyer is, and the parcel's destination is not that.
pub async fn place_of_supply(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
) -> Result<Option<Delivery>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Cart {
            id: cart_id.as_uuid(),
            customer: None,
        },
    )?;

    address_of(
        tx,
        ctx,
        cart_id,
        "coalesce(c.billing_address_id, c.shipping_address_id)",
    )
    .await
}

/// What each of the cart's two addresses says its country is. Two answers
/// rather than one because they are two statements: agreeing, they are two
/// pieces of evidence for where the buyer is, and one of them alone is not.
#[derive(Debug, Clone, Default)]
pub struct CartCountries {
    pub shipping: Option<String>,
    pub billing: Option<String>,
}

pub async fn countries(tx: &mut Tx<'_>, ctx: &Ctx<'_>, cart_id: CartId) -> Result<CartCountries> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Cart {
            id: cart_id.as_uuid(),
            customer: None,
        },
    )?;

    #[derive(FromRow)]
    struct Row {
        shipping: Option<String>,
        billing: Option<String>,
    }

    let row = sqlx::query_as::<_, Row>(
        "select s.country_code as shipping, b.country_code as billing
         from cart c
         left join cart_address s on s.scope = c.scope and s.id = c.shipping_address_id
         left join cart_address b on b.scope = c.scope and b.id = c.billing_address_id
         where c.scope = $1 and c.id = $2",
    )
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    Ok(row
        .map(|row| CartCountries {
            shipping: row.shipping,
            billing: row.billing,
        })
        .unwrap_or_default())
}

async fn address_of(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    which: &str,
) -> Result<Option<Delivery>> {
    #[derive(FromRow)]
    struct Row {
        country_code: Option<String>,
        province: Option<String>,
        postal_code: Option<String>,
    }

    let row = sqlx::query_as::<_, Row>(&format!(
        "select a.country_code, a.province, a.postal_code
         from cart c
         join cart_address a
           on a.scope = c.scope
          and a.id = {which}
         where c.scope = $1 and c.id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    Ok(row.and_then(|row| {
        row.country_code.map(|country_code| Delivery {
            country_code,
            province_code: row.province,
            postal_code: row.postal_code,
        })
    }))
}

/// Adds a variant, or adds to it: the second add of the same variant is a
/// quantity change rather than a second line, settled by the database so two
/// concurrent adds cannot each insert.
pub async fn add_line(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    line: AddLine,
) -> Result<LineItem> {
    insert_line(tx, ctx, cart_id, line, None).await
}

/// What [`add_line`] and [`add_bundle`] share. A bundle's child line merges
/// only with another child of the *same* parent — `cart_line_item_bundle_
/// component_key` is unique on `(cart_id, variant_id, parent_line_item_id)` —
/// a plan line only with another line on the *same* plan —
/// `cart_line_item_plan_key` is unique on `(cart_id, variant_id,
/// selling_plan_id)` — while an ordinary line still merges on
/// `(cart_id, variant_id)` alone, so none of the three ever collide with
/// each other no matter which is added first.
async fn insert_line(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    line: AddLine,
    parent_line_item_id: Option<LineItemId>,
) -> Result<LineItem> {
    let cart = open(tx, ctx, cart_id, Action::Write).await?;

    if line.quantity <= 0 {
        return Err(Error::invalid("a line needs a quantity of at least one"));
    }
    if line.unit_price.currency.as_str() != cart.currency_code {
        return Err(Error::invalid("that price is in another currency"));
    }
    if line.unit_price.is_negative() {
        return Err(Error::invalid("a price cannot be negative"));
    }

    let on_conflict = if parent_line_item_id.is_some() {
        "on conflict (scope, cart_id, variant_id, parent_line_item_id)
             where variant_id is not null and parent_line_item_id is not null"
    } else if line.selling_plan_id.is_some() {
        "on conflict (scope, cart_id, variant_id, selling_plan_id)
             where variant_id is not null and parent_line_item_id is null
               and selling_plan_id is not null"
    } else {
        "on conflict (scope, cart_id, variant_id)
             where variant_id is not null and parent_line_item_id is null
               and selling_plan_id is null"
    };

    let id = LineItemId::new();
    let item = sqlx::query_as::<_, LineItem>(&format!(
        "insert into cart_line_item
             (id, scope, cart_id, variant_id, product_id, product_title, product_handle,
              variant_title, variant_sku, variant_barcode, variant_option_values, thumbnail,
              quantity, unit_price, currency_code, is_tax_inclusive, is_discountable,
              requires_shipping, parent_line_item_id, selling_plan_id)
         select $1::uuid, $2::uuid, $3::uuid, v.id, p.id, p.title, p.handle, v.title, v.sku,
                v.barcode,
                (select jsonb_object_agg(o.title, ov.value)
                   from product_variant_option_value vov
                   join product_option o
                     on o.scope = vov.scope and o.id = vov.option_id
                   join product_option_value ov
                     on ov.scope = vov.scope and ov.id = vov.option_value_id
                  where vov.scope = v.scope and vov.variant_id = v.id),
                p.thumbnail_url, $5::integer, $6::numeric, $7::char(3), $8::boolean,
                p.is_discountable,
                -- Whether a variant is physical is a catalogue fact first: a
                -- shop that does not track stock links no `inventory_item` to
                -- anything, and that is not the same as selling nothing that
                -- ships. A linked inventory item that itself ships nothing
                -- still overrides a variant the catalogue calls physical.
                v.requires_shipping
                and coalesce((select bool_or(i.requires_shipping)
                                from variant_inventory_item vi
                                join inventory_item i
                                  on i.scope = vi.scope
                                 and i.id = vi.inventory_item_id
                                 and i.deleted_at is null
                               where vi.scope = v.scope and vi.variant_id = v.id), true),
                $9::uuid, $10::uuid
         from product_variant v
         join product p on p.scope = v.scope and p.id = v.product_id
         where v.scope = $2 and v.id = $4 and v.deleted_at is null and p.deleted_at is null
         {on_conflict}
         do update set quantity = cart_line_item.quantity + excluded.quantity
         returning {LINE_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .bind(line.variant_id.as_uuid())
    .bind(line.quantity)
    .bind(line.unit_price.amount)
    .bind(line.unit_price.currency.as_str())
    .bind(line.is_tax_inclusive)
    .bind(parent_line_item_id.map(|id| id.as_uuid()))
    .bind(line.selling_plan_id.map(|id| id.as_uuid()))
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("variant"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "cart_line_item",
            entity_id: item.id.as_uuid(),
            summary: serde_json::json!({ "cart": cart_id, "quantity": item.quantity }),
        },
    )
    .await?;

    Ok(item)
}

/// Adds a bundle as one parent line carrying no price of its own and one
/// child line per component, each already priced by `pricing::resolve_bundle`
/// before this is called — cart.rs prices nothing, the same as [`add_line`].
///
/// The parent's price is zero on purpose: the money is on the children, so
/// [`totals`] and [`compute`] sum a cart with a bundle in it exactly the way
/// they sum any other, without learning a second way to add a cart up. A
/// return values one component by returning its own line, which is the point
/// of giving it one.
///
/// A component shared with another bundle, or with the same variant bought
/// on its own, gets its own line: `cart_line_item_bundle_component_key` is
/// unique on the parent as well as the variant, so this bundle's child never
/// merges into one that belongs to a different parent.
pub async fn add_bundle(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    bundle: AddBundle,
) -> Result<Vec<LineItem>> {
    let cart = open(tx, ctx, cart_id, Action::Write).await?;
    let currency = cart.currency()?;

    if bundle.quantity <= 0 {
        return Err(Error::invalid("a line needs a quantity of at least one"));
    }
    if bundle.components.is_empty() {
        return Err(Error::invalid("a bundle needs at least one component"));
    }
    if bundle.components.len() as i64 >= MAX_LINES {
        return Err(Error::invalid("a bundle has too many components"));
    }

    let parent = add_line(
        tx,
        ctx,
        cart_id,
        AddLine {
            variant_id: bundle.variant_id,
            quantity: bundle.quantity,
            unit_price: Money::new(Decimal::ZERO, currency),
            is_tax_inclusive: bundle.is_tax_inclusive,
            selling_plan_id: None,
        },
    )
    .await?;

    let mut lines = vec![parent.clone()];

    for component in bundle.components {
        if component.unit_price.currency != currency {
            return Err(Error::invalid("that price is in another currency"));
        }

        let child = insert_line(
            tx,
            ctx,
            cart_id,
            AddLine {
                variant_id: component.variant_id,
                quantity: bundle.quantity,
                unit_price: component.unit_price,
                is_tax_inclusive: bundle.is_tax_inclusive,
                selling_plan_id: None,
            },
            Some(parent.id),
        )
        .await?;

        lines.push(child);
    }

    Ok(lines)
}

/// A quantity of zero is a removal, because the schema has no zero line.
pub async fn update_line(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    line_id: LineItemId,
    quantity: i32,
) -> Result<Option<LineItem>> {
    open(tx, ctx, cart_id, Action::Write).await?;

    if quantity < 0 {
        return Err(Error::invalid("a quantity cannot be negative"));
    }
    if quantity == 0 {
        remove_line(tx, ctx, cart_id, line_id).await?;
        return Ok(None);
    }

    let item = sqlx::query_as::<_, LineItem>(&format!(
        "update cart_line_item set quantity = $4
         where scope = $1 and cart_id = $2 and id = $3
         returning {LINE_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .bind(line_id.as_uuid())
    .bind(quantity)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("line item"))?;

    Ok(Some(item))
}

pub async fn remove_line(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    line_id: LineItemId,
) -> Result<()> {
    open(tx, ctx, cart_id, Action::Write).await?;

    let done =
        sqlx::query("delete from cart_line_item where scope = $1 and cart_id = $2 and id = $3")
            .bind(ctx.scope.0)
            .bind(cart_id.as_uuid())
            .bind(line_id.as_uuid())
            .execute(&mut **tx)
            .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("line item"));
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "cart_line_item",
            entity_id: line_id.as_uuid(),
            summary: serde_json::json!({ "cart": cart_id }),
        },
    )
    .await?;

    Ok(())
}

/// Copies the addresses into the cart rather than pointing at the customer's
/// address book, so editing a saved address never moves an in-flight order.
pub async fn set_addresses(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    shipping: Option<CartAddress>,
    billing: Option<CartAddress>,
) -> Result<Cart> {
    let cart = open(tx, ctx, cart_id, Action::Write).await?;

    let shipping_id = match shipping {
        Some(address) => Some(write_address(tx, ctx, cart.customer_id, address).await?),
        None => None,
    };
    let billing_id = match billing {
        Some(address) => Some(write_address(tx, ctx, cart.customer_id, address).await?),
        None => None,
    };

    let cart = sqlx::query_as::<_, Cart>(&format!(
        "update cart set
             shipping_address_id = coalesce($3, shipping_address_id),
             billing_address_id = coalesce($4, billing_address_id)
         where scope = $1 and id = $2 and completed_at is null
         returning {COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .bind(shipping_id)
    .bind(billing_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("cart"))?;

    Ok(cart)
}

async fn write_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: Option<CustomerId>,
    address: CartAddress,
) -> Result<uuid::Uuid> {
    let country = match address.country_code {
        Some(code) => {
            let code = code.trim().to_uppercase();
            if code.len() != 2 || !code.chars().all(|c| c.is_ascii_alphabetic()) {
                return Err(Error::invalid("a country code is two letters"));
            }
            Some(code)
        }
        None => None,
    };

    let id = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into cart_address
             (id, scope, customer_id, company, first_name, last_name, address_1, address_2,
              city, province, postal_code, country_code, phone)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    )
    .bind(id)
    .bind(ctx.scope.0)
    .bind(customer_id.map(CustomerId::as_uuid))
    .bind(address.company)
    .bind(address.first_name)
    .bind(address.last_name)
    .bind(address.address_1)
    .bind(address.address_2)
    .bind(address.city)
    .bind(address.province)
    .bind(address.postal_code)
    .bind(country)
    .bind(address.phone)
    .execute(&mut **tx)
    .await?;

    Ok(id)
}

/// One chosen shipping per cart: the previous choice goes rather than being
/// added to.
pub async fn set_shipping_method(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    method: NewShippingMethod,
) -> Result<ShippingMethod> {
    let cart = open(tx, ctx, cart_id, Action::Write).await?;

    if method.amount.currency.as_str() != cart.currency_code {
        return Err(Error::invalid("that price is in another currency"));
    }
    if method.amount.is_negative() {
        return Err(Error::invalid("a shipping price cannot be negative"));
    }
    if method.name.trim().is_empty() {
        return Err(Error::invalid("a shipping method needs a name"));
    }

    sqlx::query("delete from cart_shipping_method where scope = $1 and cart_id = $2")
        .bind(ctx.scope.0)
        .bind(cart_id.as_uuid())
        .execute(&mut **tx)
        .await?;

    let id = ShippingMethodId::new();
    let chosen = sqlx::query_as::<_, ShippingMethod>(&format!(
        "insert into cart_shipping_method
             (id, scope, cart_id, shipping_option_id, name, description, amount, currency_code,
              is_tax_inclusive, data)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         returning {METHOD_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .bind(method.shipping_option_id.map(ShippingOptionId::as_uuid))
    .bind(method.name.trim())
    .bind(method.description)
    .bind(method.amount.amount)
    .bind(method.amount.currency.as_str())
    .bind(method.is_tax_inclusive)
    .bind(method.data)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "cart_shipping_method",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "cart": cart_id, "name": chosen.name }),
        },
    )
    .await?;

    Ok(chosen)
}

pub async fn set_email(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    email: &str,
) -> Result<Cart> {
    open(tx, ctx, cart_id, Action::Write).await?;

    let email = email.trim().to_lowercase();
    if !email.contains('@') {
        return Err(Error::invalid("that is not an e-mail address"));
    }

    sqlx::query_as::<_, Cart>(&format!(
        "update cart set email = $3
         where scope = $1 and id = $2 and completed_at is null
         returning {COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .bind(email)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("cart"))
}

pub async fn set_customer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    customer_id: CustomerId,
) -> Result<Cart> {
    open(tx, ctx, cart_id, Action::Write).await?;

    sqlx::query_as::<_, Cart>(&format!(
        "update cart set
             customer_id = c.id,
             email = coalesce(cart.email, c.email)
         from customer c
         where cart.scope = $1
           and cart.id = $2
           and cart.completed_at is null
           and c.scope = $1
           and c.id = $3
           and c.deleted_at is null
         returning {}",
        prefixed(COLUMNS, "cart")
    ))
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .bind(customer_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("cart"))
}

/// A guest cart becomes the customer's. If they already have one, the two are
/// merged: the same variant in both is one line with the quantities added.
pub async fn transfer_to_customer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    customer_id: CustomerId,
) -> Result<Cart> {
    let guest = open(tx, ctx, cart_id, Action::Write).await?;

    let existing = sqlx::query_as::<_, Cart>(&format!(
        "select {COLUMNS} from cart
         where scope = $1
           and customer_id = $2
           and id <> $3
           and completed_at is null
           and currency_code = $4
         order by created_at desc, id desc
         limit 1"
    ))
    .bind(ctx.scope.0)
    .bind(customer_id.as_uuid())
    .bind(cart_id.as_uuid())
    .bind(&guest.currency_code)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(target) = existing else {
        return set_customer(tx, ctx, cart_id, customer_id).await;
    };

    // Parents first, so a child's `parent_line_item_id` can always be remapped
    // to its new (or merged-into) row before the child itself is inserted.
    let sources: Vec<(uuid::Uuid, Option<uuid::Uuid>, Option<uuid::Uuid>)> = sqlx::query_as(
        "select id, parent_line_item_id, selling_plan_id from cart_line_item
         where scope = $1 and cart_id = $2
         order by parent_line_item_id is not null, created_at, id",
    )
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .fetch_all(&mut **tx)
    .await?;

    // Every other column travels as it is: read from the catalogue rather
    // than named by hand, so a column added to `cart_line_item` tomorrow is
    // carried across a merge the day it is added.
    let carried: Vec<String> = sqlx::query_scalar(
        "select column_name from information_schema.columns
         where table_schema = 'public' and table_name = 'cart_line_item'
           and column_name not in
               ('id', 'scope', 'cart_id', 'parent_line_item_id', 'created_at', 'updated_at')
         order by column_name",
    )
    .fetch_all(&mut **tx)
    .await?;
    let carried = carried.join(", ");

    let mut remap: std::collections::HashMap<uuid::Uuid, uuid::Uuid> =
        std::collections::HashMap::new();

    for (from, old_parent, selling_plan_id) in sources {
        let parent_id = old_parent.map(|old_parent| {
            *remap
                .get(&old_parent)
                .expect("a bundle's parent line merges before its children")
        });

        let on_conflict = if parent_id.is_some() {
            "on conflict (scope, cart_id, variant_id, parent_line_item_id)
                 where variant_id is not null and parent_line_item_id is not null"
        } else if selling_plan_id.is_some() {
            "on conflict (scope, cart_id, variant_id, selling_plan_id)
                 where variant_id is not null and parent_line_item_id is null
                   and selling_plan_id is not null"
        } else {
            "on conflict (scope, cart_id, variant_id)
                 where variant_id is not null and parent_line_item_id is null
                   and selling_plan_id is null"
        };

        let new_id: uuid::Uuid = sqlx::query_scalar(&format!(
            "insert into cart_line_item (id, scope, cart_id, parent_line_item_id, {carried})
             select $1::uuid, scope, $2::uuid, $5::uuid, {carried}
             from cart_line_item
             where scope = $3 and id = $4
             {on_conflict}
             do update set quantity = cart_line_item.quantity + excluded.quantity
             returning id"
        ))
        .bind(LineItemId::new().as_uuid())
        .bind(target.id.as_uuid())
        .bind(ctx.scope.0)
        .bind(from)
        .bind(parent_id)
        .fetch_one(&mut **tx)
        .await?;

        remap.insert(from, new_id);
    }

    sqlx::query("delete from cart where scope = $1 and id = $2")
        .bind(ctx.scope.0)
        .bind(cart_id.as_uuid())
        .execute(&mut **tx)
        .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "cart",
            entity_id: target.id.as_uuid(),
            summary: serde_json::json!({ "merged": cart_id, "customer": customer_id }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "cart.merged",
            entity_id: target.id.as_uuid(),
            payload: serde_json::json!({ "from": cart_id, "customer": customer_id }),
        },
    )
    .await?;

    load(tx, ctx, target.id).await
}

/// The one place a cart is added up.
pub async fn totals(tx: &mut Tx<'_>, ctx: &Ctx<'_>, cart_id: CartId) -> Result<CartTotals> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Cart {
            id: cart_id.as_uuid(),
            customer: None,
        },
    )?;

    let cart = load(tx, ctx, cart_id).await?;
    let currency = cart.currency()?;

    let exponent = crate::store::exponent(tx, ctx, currency).await?;

    let lines = sqlx::query_as::<_, TotalsLine>(
        "select l.quantity, l.unit_price, l.is_tax_inclusive,
                coalesce((select sum(a.amount) from cart_line_item_adjustment a
                          where a.scope = l.scope and a.cart_line_item_id = l.id), 0) as discount,
                coalesce((select sum(t.rate) from cart_line_item_tax_line t
                          where t.scope = l.scope and t.cart_line_item_id = l.id), 0) as tax_rate
         from cart_line_item l
         where l.scope = $1 and l.cart_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .fetch_all(&mut **tx)
    .await?;

    let shipping = sqlx::query_as::<_, TotalsShipping>(
        "select s.amount, s.is_tax_inclusive,
                coalesce((select sum(a.amount) from cart_shipping_method_adjustment a
                          where a.scope = s.scope and a.cart_shipping_method_id = s.id),
                         0) as discount,
                coalesce((select sum(t.rate) from cart_shipping_method_tax_line t
                          where t.scope = s.scope and t.cart_shipping_method_id = s.id),
                         0) as tax_rate
         from cart_shipping_method s
         where s.scope = $1 and s.cart_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .fetch_all(&mut **tx)
    .await?;

    compute(&lines, &shipping, currency, exponent)
}

/// Clears out the carts nobody came back to. Completed carts are never touched:
/// one of those is an order's history. At most [`MAX_BATCH`] a call: a sweep
/// that came back full has more to take, so call it again until it does not.
///
/// Every hold an expiring cart's lines took is released before the cart goes
/// — `reservation_item.cart_line_item_id` is `restrict`, so a cart with an
/// unreleased hold cannot be deleted at all rather than deleted with the hold
/// left dangling.
pub async fn expire(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<CartId>> {
    let _: Permit = ctx.permit(Action::Delete, Resource::Customer { id: None })?;

    let candidates: Vec<CartId> = sqlx::query_scalar(
        "select id from cart
         where scope = $1
           and completed_at is null
           and expires_at is not null
           and expires_at <= $2
         order by expires_at, id
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(now)
    .bind(MAX_BATCH)
    .fetch_all(&mut **tx)
    .await?;

    for id in &candidates {
        crate::inventory::release_cart(tx, ctx, *id).await?;
    }

    let gone: Vec<CartId> =
        sqlx::query_scalar("delete from cart where scope = $1 and id = any($2) returning id")
            .bind(ctx.scope.0)
            .bind(candidates.iter().map(CartId::as_uuid).collect::<Vec<_>>())
            .fetch_all(&mut **tx)
            .await?;

    for id in &gone {
        ctx.emit(
            tx,
            Event {
                name: "cart.expired",
                entity_id: id.as_uuid(),
                payload: serde_json::json!({ "at": now }),
            },
        )
        .await?;
    }

    Ok(gone)
}

async fn load(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CartId) -> Result<Cart> {
    sqlx::query_as::<_, Cart>(&format!(
        "select {COLUMNS} from cart where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("cart"))
}

/// A cart that has become an order is closed to every write, and saying so
/// once is why nothing below repeats the check.
async fn open(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CartId, action: Action) -> Result<Cart> {
    // Read before the permit because an authorizer is asked whose cart it is,
    // and only the row knows.
    let cart = sqlx::query_as::<_, Cart>(&format!(
        "select {COLUMNS} from cart where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("cart"))?;

    let _: Permit = ctx.permit(
        action,
        Resource::Cart {
            id: id.as_uuid(),
            customer: cart.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    if cart.is_complete() {
        return Err(Error::conflict("that cart has already been ordered"));
    }

    Ok(cart)
}

fn prefixed(columns: &str, alias: &str) -> String {
    columns
        .split(',')
        .map(|column| format!("{alias}.{}", column.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn line(quantity: i32, price: Decimal, discount: Decimal, rate: Decimal) -> TotalsLine {
        TotalsLine {
            quantity,
            unit_price: price,
            is_tax_inclusive: false,
            discount,
            tax_rate: rate,
        }
    }

    #[test]
    fn the_parts_add_up_to_the_total() -> Result<()> {
        let currency = Currency::parse("TRY")?;
        let cases = [
            vec![line(1, dec!(10), dec!(0), dec!(0))],
            vec![
                line(3, dec!(19.99), dec!(5), dec!(18)),
                line(2, dec!(0.05), dec!(0), dec!(8)),
            ],
            vec![line(7, dec!(3.33), dec!(1.11), dec!(20))],
        ];

        for lines in cases {
            let totals = compute(&lines, &[], currency, 2)?;
            assert_eq!(
                totals.total.amount,
                totals.subtotal.amount - totals.discount.amount
                    + totals.shipping.amount
                    + totals.tax.amount
            );
        }

        Ok(())
    }

    #[test]
    fn a_tax_inclusive_line_does_not_pay_its_tax_twice() -> Result<()> {
        let currency = Currency::parse("TRY")?;
        let inclusive = TotalsLine {
            is_tax_inclusive: true,
            ..line(1, dec!(118), dec!(0), dec!(18))
        };
        let totals = compute(&[inclusive], &[], currency, 2)?;
        assert_eq!(totals.subtotal.amount, dec!(100.00));
        assert_eq!(totals.tax.amount, dec!(18.00));
        assert_eq!(totals.total.amount, dec!(118.00));
        Ok(())
    }

    #[test]
    fn a_discount_larger_than_the_line_does_not_make_tax_negative() -> Result<()> {
        let currency = Currency::parse("TRY")?;
        let totals = compute(&[line(1, dec!(10), dec!(50), dec!(20))], &[], currency, 2)?;
        assert!(!totals.tax.is_negative());
        Ok(())
    }
}

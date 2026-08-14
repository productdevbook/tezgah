//! Many rows at once: importing a catalogue, exporting it, and moving prices
//! and stock in bulk.
//!
//! A shop with ten thousand products does not create them one POST at a time,
//! and the alternative it reaches for when a library has no answer is raw SQL
//! against the tables — which is the one thing `GOAL.md` says it should never
//! have to do.
//!
//! # No files
//!
//! A CSV is a file, and file storage is the host's ([`GOAL.md`], "file and image
//! storage → a URL on the record"). So nothing here reads a URL or writes one:
//! an import is handed its rows, and an export hands back a page of flat rows
//! the caller renders into whatever it is uploading to.
//!
//! # Partial success
//!
//! An import applies row by row inside a savepoint. Three bad rows out of a
//! thousand come back as [`Rejection`]s naming the row and the reason, and the
//! other nine hundred and ninety-seven are in. Refusing the whole file would
//! make a large import impossible to land: the operator fixes three lines and
//! is told to send all thousand again, and the next run finds three more.
//!
//! The batches are the other way about. [`update_prices`] refuses outright when
//! one call carries two currencies, because that is not three bad rows — it is
//! a caller that assembled the wrong file, and applying half of it leaves a
//! price list nobody can reason about.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{Acquire, FromRow};
use uuid::Uuid;

use crate::catalogue;
use crate::error::{Error, Result};
use crate::id::{InventoryItemId, PriceId, PriceSetId, ProductId, StockLocationId, VariantId};
use crate::inventory;
use crate::money::{Currency, Money};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, AuditEntry, Ctx, Permit, Resource, Tx};
use crate::pricing;
use crate::workflow::{Failure, Outcome, Step, Workflow};

/// Most rows one call may carry. A refusal rather than a clamp: a caller
/// sending more has a file it has not split, and silently dropping the tail
/// would be worse than saying so.
pub const MAX_BATCH: usize = 1_000;

// ---------------------------------------------------------------------------
// Shapes
// ---------------------------------------------------------------------------

/// One line of an import: a product, one of its variants, and that variant's
/// base price. Flat on purpose — this is what a spreadsheet row holds.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProductRow {
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportProducts {
    pub rows: Vec<ProductRow>,
    /// Products to remove in the same call, which is what Medusa's
    /// `products/batch` carries beside its writes.
    #[serde(default)]
    pub delete: Vec<ProductId>,
}

/// Which row was refused and why. The row number is the caller's index into
/// what it sent, so it can point at the line in the file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rejection {
    pub row: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportResult {
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
    pub rejected: Vec<Rejection>,
    /// The products this call brought into being, so a compensating step can
    /// take back exactly what it added and nothing that was already here.
    pub created_ids: Vec<ProductId>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchResult {
    pub applied: usize,
    pub rejected: Vec<Rejection>,
}

/// One exported line: a variant, the product above it, and its base price.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ProductExport {
    pub product_id: ProductId,
    pub handle: String,
    pub product_title: String,
    pub status: catalogue::ProductStatus,
    pub variant_id: VariantId,
    pub variant_title: String,
    pub sku: Option<String>,
    pub price_amount: Option<Decimal>,
    pub price_currency: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct PriceChange {
    pub price_id: PriceId,
    pub amount: Money,
}

#[derive(Debug, Clone)]
pub struct StockLevelRow {
    pub inventory_item_id: InventoryItemId,
    pub location_id: StockLocationId,
    pub stocked_quantity: i32,
    pub incoming_quantity: Option<i32>,
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// A page of variants, flattened. `currency` picks which base price is quoted;
/// without one, whichever sorts first, so a single-currency shop needs no
/// argument.
pub async fn export_products(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    currency: Option<Currency>,
    paging: Paging,
) -> Result<Page<ProductExport>> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    let rows = sqlx::query_as::<_, ProductExport>(
        "select v.product_id      as product_id,
                p.handle          as handle,
                p.title           as product_title,
                p.status          as status,
                v.id              as variant_id,
                v.title           as variant_title,
                v.sku             as sku,
                base.amount       as price_amount,
                base.currency_code as price_currency,
                v.created_at      as created_at
           from product_variant v
           join product p
             on p.id = v.product_id and p.scope = v.scope and p.deleted_at is null
           left join lateral (
                select pr.amount, pr.currency_code
                  from product_variant_price_set link
                  join price pr
                    on pr.price_set_id = link.price_set_id and pr.scope = link.scope
                 where link.scope = v.scope
                   and link.variant_id = v.id
                   and pr.deleted_at is null
                   and pr.price_list_id is null
                   and pr.rules_count = 0
                   and ($2::text is null or pr.currency_code = $2)
                 order by pr.currency_code
                 limit 1
           ) base on true
          where v.scope = $1
            and v.deleted_at is null
            and ($3::timestamptz is null or (v.created_at, v.id) > ($3, $4))
          order by v.created_at, v.id
          limit $5",
    )
    .bind(ctx.scope.0)
    .bind(currency.map(|c| c.as_str().to_owned()))
    .bind(paging.after.map(|c| c.at))
    .bind(paging.after.map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| Cursor {
        at: row.created_at,
        id: row.variant_id.as_uuid(),
    }))
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

fn check_row(row: &ProductRow) -> Result<()> {
    if row.handle.trim().is_empty() {
        return Err(Error::invalid("a row needs a handle"));
    }
    if row.title.trim().is_empty() {
        return Err(Error::invalid("a row needs a title"));
    }
    match (row.price_amount, row.price_currency.as_deref()) {
        (None, None) => {}
        (Some(amount), Some(code)) => {
            Currency::parse(code)?;
            if amount.is_sign_negative() {
                return Err(Error::invalid("a price cannot be negative"));
            }
        }
        _ => {
            return Err(Error::invalid(
                "a price needs both an amount and a currency",
            ));
        }
    }
    Ok(())
}

fn check_batch<T>(rows: &[T]) -> Result<()> {
    if rows.len() > MAX_BATCH {
        return Err(Error::invalid(format!(
            "that is {} rows; {MAX_BATCH} is the most one call may carry",
            rows.len()
        )));
    }
    Ok(())
}

/// Whether a row made a product or found one.
enum Applied {
    Created(ProductId),
    Updated,
}

async fn apply_row(tx: &mut Tx<'_>, ctx: &Ctx<'_>, row: &ProductRow) -> Result<Applied> {
    check_row(row)?;

    let existing = match catalogue::product_by_handle(tx, ctx, &row.handle).await {
        Ok(product) => Some(product),
        Err(err) if err.is_not_found() => None,
        Err(err) => return Err(err),
    };

    let (product_id, applied) = match existing {
        Some(product) => {
            catalogue::update_product(
                tx,
                ctx,
                product.id,
                catalogue::ProductPatch {
                    title: Some(row.title.clone()),
                    subtitle: row.subtitle.clone(),
                    description: row.description.clone(),
                    ..catalogue::ProductPatch::default()
                },
            )
            .await?;
            if let Some(status) = row.status {
                catalogue::set_product_status(tx, ctx, product.id, status).await?;
            }
            (product.id, Applied::Updated)
        }
        None => {
            let product = catalogue::create_product(
                tx,
                ctx,
                catalogue::NewProduct {
                    handle: row.handle.clone(),
                    title: row.title.clone(),
                    subtitle: row.subtitle.clone(),
                    description: row.description.clone(),
                    status: row.status,
                    ..catalogue::NewProduct::default()
                },
            )
            .await?;
            (product.id, Applied::Created(product.id))
        }
    };

    let variant = match (row.variant_title.as_deref(), row.sku.as_deref()) {
        (None, None) => None,
        (title, sku) => Some(upsert_variant(tx, ctx, product_id, title, sku).await?),
    };

    if let (Some(variant), Some(amount), Some(code)) =
        (variant, row.price_amount, row.price_currency.as_deref())
    {
        let money = Money::new(amount, Currency::parse(code)?);
        set_base_price(tx, ctx, variant, money).await?;
    }

    Ok(applied)
}

/// Matches on sku when there is one and on title otherwise: a sku is the
/// identifier a spreadsheet is keyed by, and a title is what is left when the
/// shop does not keep them.
async fn upsert_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    product_id: ProductId,
    title: Option<&str>,
    sku: Option<&str>,
) -> Result<VariantId> {
    let found: Option<VariantId> = sqlx::query_scalar(
        "select id from product_variant
          where scope = $1
            and product_id = $2
            and deleted_at is null
            and (($3::text is not null and sku = $3) or ($3::text is null and title = $4))
          order by created_at
          limit 1",
    )
    .bind(ctx.scope.0)
    .bind(product_id.as_uuid())
    .bind(sku)
    .bind(title.unwrap_or_default())
    .fetch_optional(&mut **tx)
    .await?;

    let title = title.unwrap_or("Default").to_owned();

    match found {
        Some(id) => {
            catalogue::update_variant(
                tx,
                ctx,
                id,
                catalogue::VariantPatch {
                    title: Some(title),
                    ..catalogue::VariantPatch::default()
                },
            )
            .await?;
            Ok(id)
        }
        None => {
            let variant = catalogue::create_variant(
                tx,
                ctx,
                product_id,
                catalogue::NewVariant {
                    title,
                    sku: sku.map(str::to_owned),
                    ..catalogue::NewVariant::default()
                },
            )
            .await?;
            Ok(variant.id)
        }
    }
}

/// The variant's price with no rules and no price list on it — the one a
/// spreadsheet means by "price".
async fn set_base_price(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant: VariantId,
    amount: Money,
) -> Result<()> {
    let set: PriceSetId = match pricing::price_set_for_variant(tx, ctx, variant).await? {
        Some(set) => set,
        None => {
            let set = pricing::create_price_set(tx, ctx).await?;
            pricing::link_variant(tx, ctx, variant, set.id).await?;
            set.id
        }
    };

    let existing: Option<PriceId> = sqlx::query_scalar(
        "select id from price
          where scope = $1
            and price_set_id = $2
            and price_list_id is null
            and rules_count = 0
            and currency_code = $3
            and deleted_at is null
          order by created_at
          limit 1",
    )
    .bind(ctx.scope.0)
    .bind(set.as_uuid())
    .bind(amount.currency.as_str())
    .fetch_optional(&mut **tx)
    .await?;

    match existing {
        Some(id) => {
            pricing::update_price(
                tx,
                ctx,
                id,
                pricing::PriceUpdate {
                    amount: Some(amount),
                    ..pricing::PriceUpdate::default()
                },
            )
            .await?;
        }
        None => {
            pricing::add_price(
                tx,
                ctx,
                pricing::NewPrice {
                    price_set_id: set,
                    price_list_id: None,
                    title: None,
                    amount,
                    min_quantity: None,
                    max_quantity: None,
                    rules: Vec::new(),
                },
            )
            .await?;
        }
    }

    Ok(())
}

/// Applies an import, one savepoint per row.
///
/// The savepoint is what makes partial success possible at all: a duplicate
/// handle comes back from Postgres as an error that aborts the transaction, and
/// without a savepoint to roll back to, row four would take the three before it
/// with it.
pub async fn import_products(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: ImportProducts,
) -> Result<ImportResult> {
    let _: Permit = ctx.permit(Action::Write, Resource::Product { id: None })?;

    check_batch(&input.rows)?;
    check_batch(&input.delete)?;

    let mut result = ImportResult::default();

    for (at, row) in input.rows.iter().enumerate() {
        let mut savepoint = (&mut *tx).begin().await?;
        match apply_row(&mut savepoint, ctx, row).await {
            Ok(applied) => {
                savepoint.commit().await?;
                match applied {
                    Applied::Created(id) => {
                        result.created += 1;
                        result.created_ids.push(id);
                    }
                    Applied::Updated => result.updated += 1,
                }
            }
            Err(err) => {
                savepoint.rollback().await?;
                result.rejected.push(Rejection {
                    row: at,
                    reason: err.to_string(),
                });
            }
        }
    }

    for (at, id) in input.delete.iter().enumerate() {
        let mut savepoint = (&mut *tx).begin().await?;
        match catalogue::delete_product(&mut savepoint, ctx, *id).await {
            Ok(()) => {
                savepoint.commit().await?;
                result.deleted += 1;
            }
            Err(err) => {
                savepoint.rollback().await?;
                result.rejected.push(Rejection {
                    row: input.rows.len() + at,
                    reason: err.to_string(),
                });
            }
        }
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "product_import",
            entity_id: Uuid::now_v7(),
            summary: serde_json::json!({
                "created": result.created,
                "updated": result.updated,
                "deleted": result.deleted,
                "rejected": result.rejected.len(),
            }),
        },
    )
    .await?;

    Ok(result)
}

// ---------------------------------------------------------------------------
// Price and stock batches
// ---------------------------------------------------------------------------

/// Moves a set of prices. One currency per call: two in one batch is a caller
/// that built the wrong file, so none of it is applied.
pub async fn update_prices(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    changes: Vec<PriceChange>,
) -> Result<BatchResult> {
    let _: Permit = ctx.permit(Action::Write, Resource::Pricing)?;

    check_batch(&changes)?;

    if let Some(first) = changes.first() {
        if changes
            .iter()
            .any(|change| change.amount.currency != first.amount.currency)
        {
            return Err(Error::invalid(
                "one batch of prices carries one currency; split it",
            ));
        }
    }

    let mut result = BatchResult::default();

    for (at, change) in changes.iter().enumerate() {
        let mut savepoint = (&mut *tx).begin().await?;
        let done = pricing::update_price(
            &mut savepoint,
            ctx,
            change.price_id,
            pricing::PriceUpdate {
                amount: Some(change.amount),
                ..pricing::PriceUpdate::default()
            },
        )
        .await;

        match done {
            Ok(_) => {
                savepoint.commit().await?;
                result.applied += 1;
            }
            Err(err) => {
                savepoint.rollback().await?;
                result.rejected.push(Rejection {
                    row: at,
                    reason: err.to_string(),
                });
            }
        }
    }

    Ok(result)
}

/// Sets the counted stock of many items at many locations at once — a stock
/// take, which is the read a shop does with a scanner and a spreadsheet.
pub async fn set_stock_levels(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    rows: Vec<StockLevelRow>,
) -> Result<BatchResult> {
    let _: Permit = ctx.permit(Action::Write, Resource::Inventory { id: None })?;

    check_batch(&rows)?;

    let mut result = BatchResult::default();

    for (at, row) in rows.iter().enumerate() {
        let mut savepoint = (&mut *tx).begin().await?;
        let done = inventory::set_stock(
            &mut savepoint,
            ctx,
            row.inventory_item_id,
            row.location_id,
            row.stocked_quantity,
            row.incoming_quantity.unwrap_or(0),
        )
        .await;

        match done {
            Ok(_) => {
                savepoint.commit().await?;
                result.applied += 1;
            }
            Err(err) => {
                savepoint.rollback().await?;
                result.rejected.push(Rejection {
                    row: at,
                    reason: err.to_string(),
                });
            }
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// As a workflow
// ---------------------------------------------------------------------------

/// Checks the shape of every row without writing anything, so a file that is
/// wrong throughout is refused before half of it is in.
#[derive(Debug)]
pub struct Validate;

#[async_trait::async_trait]
impl Step for Validate {
    fn name(&self) -> &'static str {
        "import.validate"
    }

    async fn invoke(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        input: &serde_json::Value,
    ) -> std::result::Result<Outcome, Failure> {
        let parsed: ImportProducts = serde_json::from_value(input.clone())
            .map_err(|err| Failure::Final(Error::invalid(err.to_string())))?;
        check_batch(&parsed.rows).map_err(Failure::Final)?;
        check_batch(&parsed.delete).map_err(Failure::Final)?;

        Ok(Outcome::new(input.clone(), serde_json::Value::Null))
    }
}

/// Applies the rows. Compensating takes back the products this run created and
/// leaves the ones it merely edited: there is no before-image to restore them
/// to, and deleting a product that existed before the import would lose more
/// than the import added.
#[derive(Debug)]
pub struct Apply;

#[async_trait::async_trait]
impl Step for Apply {
    fn name(&self) -> &'static str {
        "import.apply"
    }

    async fn invoke(
        &self,
        tx: &mut Tx<'_>,
        ctx: &Ctx<'_>,
        input: &serde_json::Value,
    ) -> std::result::Result<Outcome, Failure> {
        let parsed: ImportProducts = serde_json::from_value(input.clone())
            .map_err(|err| Failure::Final(Error::invalid(err.to_string())))?;

        let result = import_products(tx, ctx, parsed)
            .await
            .map_err(Failure::Retry)?;

        let kept = serde_json::json!({ "created_ids": result.created_ids });
        let output = serde_json::to_value(&result)
            .map_err(|err| Failure::Final(Error::invalid(err.to_string())))?;

        Ok(Outcome::new(output, kept))
    }

    async fn compensate(
        &self,
        tx: &mut Tx<'_>,
        ctx: &Ctx<'_>,
        kept: &serde_json::Value,
    ) -> Result<()> {
        let ids: Vec<ProductId> = serde_json::from_value(kept["created_ids"].clone())
            .map_err(|err| Error::invalid(err.to_string()))?;
        for id in ids {
            catalogue::delete_product(tx, ctx, id).await?;
        }
        Ok(())
    }
}

/// Validate, then apply. A host runs it through
/// [`workflow::run`](crate::workflow::run) when the file is large enough to
/// want checkpoints and a resume.
pub fn import_workflow() -> Workflow {
    Workflow::new("product.import").then(Validate).then(Apply)
}

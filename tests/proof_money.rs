//! Money invariants, under sequences nobody wrote by hand.
//!
//! Two halves, because the two halves have different costs.
//!
//! The arithmetic — `money::allocate` and `cart::compute` — touches no database
//! and is checked with `proptest` over several hundred cases each, shrinking a
//! failure down to the smallest input that still breaks it.
//!
//! Everything with a table behind it is checked by a deterministic walk instead:
//! a seeded generator draws an operation, the operation runs in its own
//! transaction, and every invariant is re-read from the database afterwards.
//! A shrinker would be worth little here — a failing sequence is only meaningful
//! whole — and a property runner's hundreds of cases would be hours of Postgres.
//! The seed and the step are printed on failure, and re-running that seed
//! replays the same sequence.
//!
//! `TEZGAH_PROOF_RUNS` and `TEZGAH_PROOF_STEPS` widen the walk without a
//! recompile: the default is what fits inside nextest's slow-test timeout, and
//! a nightly with longer has only to raise them.

mod common;

use common::Shop;
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::json;
use tezgah::Result;
use tezgah::cart::{self, AddLine, NewCart, TotalsLine, TotalsShipping};
use tezgah::catalogue::{self, NewProduct, NewVariant};
use tezgah::id::{
    CartId, InventoryItemId, LineItemId, OrderId, PaymentId, PromotionId, StockLocationId,
    VariantId,
};
use tezgah::inventory::{self, NewInventoryItem, NewStockLocation};
use tezgah::money::{self, Currency, Money};
use tezgah::order::{self, NewOrder, NewOrderLine, ReturnLine};
use tezgah::payment::{self, Authorization, AuthorizationStatus, NewCollection, NewSession};
use tezgah::ports::{Ctx, Tx};
use tezgah::promotion::{self, NewPromotion, PromotionKind, Status};

fn lira() -> Currency {
    Currency::parse("TRY").expect("a currency code")
}

fn try_(amount: Decimal) -> Money {
    Money::new(amount, lira())
}

// ---------------------------------------------------------------------------
// The arithmetic, over generated inputs
// ---------------------------------------------------------------------------

/// Every exponent tezgah has to survive: JPY, most of the world, and KWD.
const EXPONENTS: [u32; 3] = [0, 2, 3];

fn a_line(inclusive: bool) -> impl Strategy<Value = TotalsLine> {
    (0i32..20, 1i64..500_000, 0i64..100_000, 0i64..3_000).prop_map(
        move |(quantity, unit, discount, rate)| TotalsLine {
            quantity,
            unit_price: Decimal::new(unit, 2),
            is_tax_inclusive: inclusive,
            discount: Decimal::new(discount, 2),
            tax_rate: Decimal::new(rate, 2),
        },
    )
}

fn a_shipping(inclusive: bool) -> impl Strategy<Value = TotalsShipping> {
    (0i64..100_000, 0i64..50_000, 0i64..3_000).prop_map(move |(amount, discount, rate)| {
        TotalsShipping {
            amount: Decimal::new(amount, 2),
            is_tax_inclusive: inclusive,
            discount: Decimal::new(discount, 2),
            tax_rate: Decimal::new(rate, 2),
        }
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn the_parts_of_an_allocation_always_add_back_up_to_the_whole(
        minor in -100_000_000i64..100_000_000,
        weights in prop::collection::vec(1i64..1_000_000, 1..12),
        which in 0usize..EXPONENTS.len(),
    ) {
        let exponent = EXPONENTS[which];
        let total = try_(Decimal::new(minor, exponent));
        let weights: Vec<Decimal> = weights.into_iter().map(|w| Decimal::new(w, 3)).collect();

        let parts = money::allocate(total, &weights, exponent).expect("an allocation");

        prop_assert_eq!(parts.len(), weights.len());
        let back: Decimal = parts.iter().map(|part| part.amount).sum();
        prop_assert_eq!(back, total.amount, "the parts do not add back up");
        prop_assert!(parts.iter().all(|part| part.currency == total.currency));
    }

    #[test]
    fn an_allocation_across_nothing_is_refused(
        minor in -1_000_000i64..1_000_000,
        weights in prop::collection::vec(Just(Decimal::ZERO), 1..6),
    ) {
        let refused = money::allocate(try_(Decimal::new(minor, 2)), &weights, 2);
        prop_assert!(refused.is_err());
    }

    #[test]
    fn a_total_is_always_its_parts_composed(
        lines in prop::collection::vec(prop_oneof![a_line(false), a_line(true)], 0..12),
        shipping in prop::collection::vec(prop_oneof![a_shipping(false), a_shipping(true)], 0..3),
        which in 0usize..EXPONENTS.len(),
    ) {
        let exponent = EXPONENTS[which];
        let totals = cart::compute(&lines, &shipping, lira(), exponent).expect("a total");

        prop_assert_eq!(
            totals.total.amount,
            totals.subtotal.amount - totals.discount.amount
                + totals.shipping.amount + totals.tax.amount,
            "the total is not what its parts come to"
        );
        prop_assert!(!totals.tax.amount.is_sign_negative(), "tax below nothing");
        prop_assert!(!totals.subtotal.amount.is_sign_negative(), "a subtotal below nothing");
    }

    #[test]
    fn a_total_is_always_the_sum_of_its_lines(
        lines in prop::collection::vec((0i32..20, 1i64..500_000), 0..12),
        which in 0usize..EXPONENTS.len(),
    ) {
        let exponent = EXPONENTS[which];
        let lines: Vec<TotalsLine> = lines
            .into_iter()
            .map(|(quantity, unit)| TotalsLine {
                quantity,
                unit_price: Decimal::new(unit, 2),
                is_tax_inclusive: false,
                discount: Decimal::ZERO,
                tax_rate: Decimal::ZERO,
            })
            .collect();

        let by_hand: Decimal = lines
            .iter()
            .map(|line| line.unit_price * Decimal::from(line.quantity))
            .sum();
        let totals = cart::compute(&lines, &[], lira(), exponent).expect("a total");

        prop_assert_eq!(totals.subtotal.amount, by_hand.round_dp(exponent));
        prop_assert_eq!(totals.total.amount, totals.subtotal.amount);
    }

    #[test]
    fn a_line_of_negative_quantity_is_never_added_up(
        quantity in -100i32..0,
        unit in 1i64..500_000,
    ) {
        let line = TotalsLine {
            quantity,
            unit_price: Decimal::new(unit, 2),
            is_tax_inclusive: false,
            discount: Decimal::ZERO,
            tax_rate: Decimal::ZERO,
        };
        prop_assert!(cart::compute(&[line], &[], lira(), 2).is_err());
    }
}

// ---------------------------------------------------------------------------
// The same invariants, against a database, under a generated sequence
// ---------------------------------------------------------------------------

/// SplitMix64. A generator rather than a dependency, because all that is wanted
/// of it is that the same seed draws the same sequence twice.
struct Seeded {
    seed: u64,
    state: u64,
}

impl Seeded {
    fn at(seed: u64) -> Seeded {
        Seeded { seed, state: seed }
    }

    fn draw(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        (self.draw() % bound as u64) as usize
    }

    fn between(&mut self, low: i32, high: i32) -> i32 {
        low + self.below((high - low + 1) as usize) as i32
    }

    /// An amount with two decimal places, in `low..=high` whole units.
    fn money(&mut self, low: i32, high: i32) -> Money {
        let whole = self.between(low, high);
        let cents = self.between(0, 99);
        try_(Decimal::new(i64::from(whole) * 100 + i64::from(cents), 2))
    }
}

const STOCK: i32 = 200;
const USAGE_LIMIT: i32 = 5;

struct Shelf {
    item: InventoryItemId,
    location: StockLocationId,
    promotion: PromotionId,
    variant: VariantId,
}

/// A product that can be counted, and a promotion with a limit worth exceeding.
async fn stock_the_shelf(shop: &Shop, run: u64) -> Shelf {
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let product = catalogue::create_product(
        &mut tx,
        &ctx,
        NewProduct {
            handle: format!("kettle-{run}"),
            title: "A kettle".into(),
            ..NewProduct::default()
        },
    )
    .await
    .expect("a product");

    let variant = catalogue::create_variant(
        &mut tx,
        &ctx,
        product.id,
        NewVariant {
            title: "One size".into(),
            sku: Some(format!("kettle-{run}-1")),
            ..NewVariant::default()
        },
    )
    .await
    .expect("a variant");

    let location = inventory::create_stock_location(
        &mut tx,
        &ctx,
        NewStockLocation {
            name: format!("shelf-{run}"),
        },
    )
    .await
    .expect("a location");

    let item = inventory::create_inventory_item(
        &mut tx,
        &ctx,
        NewInventoryItem {
            sku: Some(format!("kettle-{run}-1")),
            title: Some("A kettle".into()),
            requires_shipping: true,
        },
    )
    .await
    .expect("an inventory item");

    inventory::attach_inventory_item(&mut tx, &ctx, variant.id, item.id, 1)
        .await
        .expect("the item behind the variant");

    inventory::set_stock(&mut tx, &ctx, item.id, location.id, STOCK, 0)
        .await
        .expect("stock on the shelf");

    let promotion = promotion::create_promotion(
        &mut tx,
        &ctx,
        NewPromotion {
            code: format!("TEN-{run}"),
            kind: PromotionKind::Standard,
            status: Status::Active,
            is_automatic: false,
            campaign_id: None,
            usage_limit: Some(USAGE_LIMIT),
            customer_usage_limit: None,
        },
    )
    .await
    .expect("a promotion");

    tx.commit().await.expect("to commit the fixture");

    Shelf {
        item: item.id,
        location: location.id,
        promotion: promotion.id,
        variant: variant.id,
    }
}

/// What the walk is holding at any moment.
struct Walk {
    cart: CartId,
    order: Option<OrderId>,
    payment: Option<PaymentId>,
    /// A hand-counted shelf may leave less on it than is already reserved, so
    /// `available` is only an assertion about reserving until one happens.
    counted: bool,
}

#[tokio::test]
async fn money_invariants_hold_under_a_generated_sequence_of_operations() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let runs: u64 = env_or("TEZGAH_PROOF_RUNS", 3);
    let steps: u32 = env_or("TEZGAH_PROOF_STEPS", 40);

    {
        let mut tx = shop.begin().await;
        payment::register_provider(&mut tx, &ctx, "fake")
            .await
            .expect("a provider");
        tx.commit().await.expect("to commit");
    }

    for run in 0..runs {
        // The run number is the seed, so a failure names something to re-run.
        let seed = 0x7e26_a400 + run;
        let mut rng = Seeded::at(seed);
        let shelf = stock_the_shelf(&shop, run).await;

        let cart = {
            let mut tx = shop.begin().await;
            let cart = cart::create(&mut tx, &ctx, NewCart::guest(lira()))
                .await
                .expect("a cart");
            tx.commit().await.expect("to commit");
            cart.id
        };

        let mut walk = Walk {
            cart,
            order: None,
            payment: None,
            counted: false,
        };

        for step in 0..steps {
            let choice = rng.below(9);
            let mut tx = shop.begin().await;
            let outcome = one_step(&mut tx, &ctx, &mut rng, &shelf, &mut walk, choice).await;

            match outcome {
                Ok(()) => tx.commit().await.expect("to commit the step"),
                Err(err) => {
                    tx.rollback().await.expect("to roll back a refused step");
                    assert!(
                        err.detail().is_none() || !err.is_internal(),
                        "seed {}, step {step}: an operation failed as a bug: {}",
                        rng.seed,
                        err.report()
                    );
                }
            }

            hold(&shop, &ctx, &shelf, &walk, rng.seed, step).await;
        }
    }

    shop.close().await;
}

fn env_or<T: std::str::FromStr>(name: &str, fallback: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(fallback)
}

/// One draw from the alphabet. Refusing is an answer: what must not happen is
/// an invariant broken, not an operation declined.
async fn one_step(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    rng: &mut Seeded,
    shelf: &Shelf,
    walk: &mut Walk,
    choice: usize,
) -> Result<()> {
    match choice {
        0 => {
            cart::add_line(
                tx,
                ctx,
                walk.cart,
                AddLine {
                    variant_id: shelf.variant,
                    quantity: rng.between(1, 4),
                    unit_price: rng.money(1, 90),
                    is_tax_inclusive: false,
                },
            )
            .await?;
            Ok(())
        }
        1 => {
            let lines = cart::lines(tx, ctx, walk.cart).await?;
            let Some(line) = pick(rng, &lines) else {
                return Ok(());
            };
            cart::update_line(tx, ctx, walk.cart, line, rng.between(0, 5)).await?;
            Ok(())
        }
        2 => {
            let lines = cart::lines(tx, ctx, walk.cart).await?;
            let Some(line) = pick(rng, &lines) else {
                return Ok(());
            };
            cart::remove_line(tx, ctx, walk.cart, line).await
        }
        3 => promotion::claim(tx, ctx, shelf.promotion, None).await,
        4 => {
            if walk.order.is_some() {
                return Ok(());
            }
            let lines = cart::lines(tx, ctx, walk.cart).await?;
            if lines.is_empty() {
                return Ok(());
            }

            let mut new = NewOrder::of(lira());
            let mut wanted = 0;
            for line in &lines {
                wanted += line.quantity;
                let mut ordered = NewOrderLine::of(
                    line.product_title.clone(),
                    line.quantity,
                    try_(line.unit_price),
                );
                ordered.variant_id = line.variant_id;
                new.lines.push(ordered);
            }

            let order = order::create(tx, ctx, new).await?;
            inventory::reserve(
                tx,
                ctx,
                shelf.item,
                shelf.location,
                wanted,
                None,
                false,
                None,
            )
            .await?;

            let total = order::totals(tx, ctx, order.id, order.version).await?.total;
            let collection = payment::create_collection(
                tx,
                ctx,
                NewCollection {
                    amount: total,
                    metadata: None,
                },
            )
            .await?;
            let session = payment::create_session(
                tx,
                ctx,
                NewSession {
                    collection_id: collection.id,
                    provider_code: "fake".into(),
                    amount: total,
                    context: None,
                },
            )
            .await?;
            let held = payment::authorize(
                tx,
                ctx,
                session.id,
                Authorization {
                    status: AuthorizationStatus::Authorized,
                    amount: Some(total),
                    data: json!({}),
                    redirect: None,
                    message: None,
                },
            )
            .await?
            .payment()?;

            order::record_transaction(tx, ctx, order.id, total, "payment", held.id.as_uuid())
                .await?;

            walk.order = Some(order.id);
            walk.payment = Some(held.id);
            Ok(())
        }
        5 => {
            let (Some(order), Some(paid)) = (walk.order, walk.payment) else {
                return Ok(());
            };
            let amount = rng.money(1, 60);
            let capture = payment::capture(tx, ctx, paid, amount, None).await?;
            order::record_transaction(tx, ctx, order, amount, "capture", capture.id.as_uuid())
                .await?;
            Ok(())
        }
        6 => {
            let (Some(order), Some(paid)) = (walk.order, walk.payment) else {
                return Ok(());
            };
            let amount = rng.money(1, 40);
            let refund = payment::refund(tx, ctx, paid, amount, None, None).await?;
            // The ledger reads a refund as money leaving, so it is signed.
            order::record_transaction(
                tx,
                ctx,
                order,
                try_(-amount.amount),
                "refund",
                refund.id.as_uuid(),
            )
            .await?;
            Ok(())
        }
        7 => {
            let Some(order) = walk.order else {
                return Ok(());
            };
            let items = order::line_items(tx, ctx, order).await?;
            let Some(line) = items.first().map(|line| line.id) else {
                return Ok(());
            };
            order::request_return(
                tx,
                ctx,
                order,
                Some(shelf.location),
                vec![ReturnLine {
                    order_line_item_id: line,
                    quantity: 1,
                    return_reason_id: None,
                    note: None,
                }],
            )
            .await?;
            Ok(())
        }
        _ => {
            let delta = rng.between(-6, 6);
            if delta == 0 {
                return Ok(());
            }
            inventory::adjust_stock(tx, ctx, shelf.item, shelf.location, delta, None).await?;
            walk.counted = true;
            Ok(())
        }
    }
}

fn pick(rng: &mut Seeded, lines: &[cart::LineItem]) -> Option<LineItemId> {
    if lines.is_empty() {
        return None;
    }
    let at = rng.below(lines.len());
    Some(lines[at].id)
}

/// Everything that has to be true after every single step.
async fn hold(shop: &Shop, ctx: &Ctx<'_>, shelf: &Shelf, walk: &Walk, seed: u64, step: u32) {
    let mut tx = shop.begin().await;
    let at = format!("seed {seed}, step {step}");

    let lines = cart::lines(&mut tx, ctx, walk.cart)
        .await
        .expect("the cart's lines");
    let by_hand: Decimal = lines
        .iter()
        .map(|line| line.unit_price * Decimal::from(line.quantity))
        .sum();
    let totals = cart::totals(&mut tx, ctx, walk.cart)
        .await
        .expect("the cart's totals");

    assert_eq!(
        totals.subtotal.amount,
        by_hand.round_dp(2),
        "{at}: a cart's subtotal is not the sum of its lines"
    );
    assert_eq!(
        totals.total.amount,
        totals.subtotal.amount - totals.discount.amount
            + totals.shipping.amount
            + totals.tax.amount,
        "{at}: a cart's total is not what its parts come to"
    );
    assert!(
        lines.iter().all(|line| line.quantity > 0),
        "{at}: a line survived with no quantity on it"
    );

    let level = inventory::level(&mut tx, ctx, shelf.item, shelf.location)
        .await
        .expect("the level");
    assert!(
        level.stocked_quantity >= 0,
        "{at}: stock fell below none ({})",
        level.stocked_quantity
    );
    assert!(
        level.reserved_quantity >= 0,
        "{at}: a reservation fell below none ({})",
        level.reserved_quantity
    );
    assert!(
        walk.counted || level.available_quantity >= 0,
        "{at}: reserving drove available below none with no backorder allowed ({})",
        level.available_quantity
    );

    let promotion = promotion::promotion(&mut tx, ctx, shelf.promotion)
        .await
        .expect("the promotion");
    assert!(
        promotion.used <= USAGE_LIMIT,
        "{at}: a promotion was used {} times of {USAGE_LIMIT}",
        promotion.used
    );

    if let Some(paid) = walk.payment {
        let balance = payment::balance(&mut tx, ctx, paid)
            .await
            .expect("a balance");
        assert!(
            balance.captured <= balance.authorized,
            "{at}: {} captured against {} authorised",
            balance.captured,
            balance.authorized
        );
        assert!(
            balance.refunded <= balance.captured,
            "{at}: {} refunded of {} captured",
            balance.refunded,
            balance.captured
        );
    }

    if let Some(id) = walk.order {
        let order = order::get(&mut tx, ctx, id).await.expect("the order");
        let totals = order::totals(&mut tx, ctx, id, order.version)
            .await
            .expect("the order's totals");
        let items = order::items(&mut tx, ctx, id, order.version)
            .await
            .expect("the order's items");
        let line_items = order::line_items(&mut tx, ctx, id)
            .await
            .expect("the order's lines");

        let by_hand: Decimal = items
            .iter()
            .map(|item| {
                let unit = item.unit_price.unwrap_or_else(|| {
                    line_items
                        .iter()
                        .find(|line| line.id == item.order_line_item_id)
                        .map(|line| line.unit_price)
                        .unwrap_or(Decimal::ZERO)
                });
                unit * Decimal::from(item.quantity)
            })
            .sum();

        assert_eq!(
            totals.subtotal.amount,
            by_hand.round_dp(2),
            "{at}: an order's subtotal is not the sum of its lines"
        );
        assert_eq!(
            totals.total.amount,
            totals.subtotal.amount - totals.discount.amount
                + totals.shipping.amount
                + totals.tax.amount,
            "{at}: an order's total is not what its parts come to"
        );

        let ledger = order::ledger(&mut tx, ctx, id).await.expect("the ledger");
        assert!(
            ledger.refunded.amount <= ledger.captured.amount,
            "{at}: {} refunded of {} captured",
            ledger.refunded.amount,
            ledger.captured.amount
        );
        assert_eq!(
            ledger.paid.amount,
            ledger.captured.amount - ledger.refunded.amount,
            "{at}: what is held is not what was taken less what was given back"
        );
    }

    tx.commit().await.expect("to commit the reading");
}

/// The one thing the walk cannot reach: a currency whose exponent is not two.
///
/// `allocate` is the same code either way, and the generated cases above cover
/// 0, 2 and 3 — this pins the two that a shop actually rounds with.
#[test]
fn an_allocation_holds_for_a_currency_with_no_minor_unit_and_for_one_with_three() {
    for (exponent, total) in [(0u32, dec!(100)), (3, dec!(100.000))] {
        let parts = money::allocate(try_(total), &[dec!(1), dec!(1), dec!(1)], exponent)
            .expect("an allocation");
        let back: Decimal = parts.iter().map(|part| part.amount).sum();
        assert_eq!(back, total, "an exponent of {exponent} lost the remainder");
    }
}

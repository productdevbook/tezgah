// Shared by every test binary, so each one leaves some of it unused.
#![allow(dead_code)]

//! A real Postgres, two scopes, and a host that says yes.
//!
//! Every test gets its own database. Not a transaction rolled back at the end:
//! the crate under test opens its own transactions, and half of what is worth
//! testing is what happens when two of them meet.
//!
//! Two scopes are seeded rather than one, because the interesting question is
//! not whether a query returns rows — it is whether it returns somebody else's.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{Executor, PgPool, postgres::PgPoolOptions};
use tezgah::ports::{
    Action, Actor, AuditEntry, AuditSink, Authorizer, Clock, Ctx, Event, EventSink, Host, JobSpec,
    Jobs, Permit, Resource, Scope, Tx,
};
use uuid::Uuid;

/// A host that permits everything and remembers what it was told, so a test
/// can ask whether an audit row was written without a table to read.
#[derive(Debug, Default)]
pub struct Recorder {
    pub audits: parking_lot::Mutex<Vec<(&'static str, Uuid)>>,
    pub events: parking_lot::Mutex<Vec<&'static str>>,
    pub payloads: parking_lot::Mutex<Vec<(&'static str, serde_json::Value)>>,
    pub jobs: parking_lot::Mutex<Vec<&'static str>>,
    now: Option<DateTime<Utc>>,
}

impl Recorder {
    pub fn new() -> Arc<Self> {
        Arc::new(Recorder::default())
    }

    /// A clock stopped at a chosen moment, for anything that expires.
    pub fn at(moment: DateTime<Utc>) -> Arc<Self> {
        Arc::new(Recorder {
            now: Some(moment),
            ..Recorder::default()
        })
    }

    pub fn emitted(&self, name: &str) -> bool {
        self.events.lock().contains(&name)
    }

    /// What one kind of event carried, in the order it was emitted.
    pub fn payloads_of(&self, name: &str) -> Vec<serde_json::Value> {
        self.payloads
            .lock()
            .iter()
            .filter(|(seen, _)| *seen == name)
            .map(|(_, payload)| payload.clone())
            .collect()
    }

    pub fn queued(&self, kind: &str) -> bool {
        self.jobs.lock().contains(&kind)
    }

    pub fn audited(&self, entity: &str) -> bool {
        self.audits.lock().iter().any(|(seen, _)| *seen == entity)
    }
}

impl Authorizer for Recorder {
    fn authorize(&self, _: &Actor, _: Action, _: &Resource) -> tezgah::Result<Permit> {
        Ok(Permit::granted())
    }
}

impl Clock for Recorder {
    fn now(&self) -> DateTime<Utc> {
        self.now.unwrap_or_else(Utc::now)
    }
}

#[async_trait]
impl AuditSink for Recorder {
    async fn record(&self, _: &mut Tx<'_>, entry: AuditEntry) -> tezgah::Result<()> {
        self.audits.lock().push((entry.entity, entry.entity_id));
        Ok(())
    }
}

#[async_trait]
impl EventSink for Recorder {
    async fn emit(&self, _: &mut Tx<'_>, event: Event) -> tezgah::Result<()> {
        self.events.lock().push(event.name);
        self.payloads.lock().push((event.name, event.payload));
        Ok(())
    }
}

#[async_trait]
impl Jobs for Recorder {
    async fn enqueue(&self, _: &mut Tx<'_>, job: JobSpec) -> tezgah::Result<()> {
        self.jobs.lock().push(job.kind);
        Ok(())
    }
}

/// A host that refuses everything, for proving a call actually asks.
#[derive(Debug, Default)]
pub struct Doorman;

impl Authorizer for Doorman {
    fn authorize(&self, _: &Actor, _: Action, _: &Resource) -> tezgah::Result<Permit> {
        Err(tezgah::Error::denied())
    }
}

impl Clock for Doorman {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[async_trait]
impl AuditSink for Doorman {
    async fn record(&self, _: &mut Tx<'_>, _: AuditEntry) -> tezgah::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl EventSink for Doorman {
    async fn emit(&self, _: &mut Tx<'_>, _: Event) -> tezgah::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Jobs for Doorman {
    async fn enqueue(&self, _: &mut Tx<'_>, _: JobSpec) -> tezgah::Result<()> {
        Ok(())
    }
}

pub struct Shop {
    pub pool: PgPool,
    pub here: Scope,
    /// Somebody else's shop, seeded so isolation can be asserted rather than
    /// assumed.
    pub elsewhere: Scope,
    pub host: Arc<Recorder>,
    name: String,
    role: String,
    admin: PgPool,
    its_url: String,
    /// Roles a test made for itself, dropped with the database.
    extra_roles: parking_lot::Mutex<Vec<String>>,
}

impl Shop {
    /// A fresh database with the migrations applied and two scopes in it.
    ///
    /// Connects as a role that is neither the superuser nor the owner of the
    /// tables. Both of those bypass row-level security — a superuser always,
    /// an owner unless the policy is forced — so a test that connected as
    /// either would prove nothing about isolation while appearing to.
    pub async fn open() -> Shop {
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".into());

        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("a Postgres to test against");

        let name = format!("tezgah_{}", Uuid::now_v7().simple());
        admin
            .execute(format!(r#"create database "{name}""#).as_str())
            .await
            .expect("a database of its own");

        let mut its_url = url::Url::parse(&url).expect("a database url");
        its_url.set_path(&name);

        let owner = PgPoolOptions::new()
            .max_connections(1)
            .connect(its_url.as_str())
            .await
            .expect("its own database");

        tezgah::MIGRATIONS
            .run(&owner)
            .await
            .expect("the migrations to apply");

        // What a host should deploy as, and therefore what to test as.
        let role = format!("{name}_app");
        for statement in [
            format!(r#"create role "{role}" login password 'app'"#),
            format!(r#"grant usage on schema public to "{role}""#),
            format!(
                r#"grant select, insert, update, delete on all tables in schema public to "{role}""#
            ),
            format!(r#"grant execute on all functions in schema public to "{role}""#),
        ] {
            owner
                .execute(statement.as_str())
                .await
                .expect("to make the application role");
        }

        let here = Scope(Uuid::now_v7());
        let elsewhere = Scope(Uuid::now_v7());
        for scope in [here, elsewhere] {
            sqlx::query("insert into tezgah_scope (id) values ($1)")
                .bind(scope.0)
                .execute(&owner)
                .await
                .expect("a scope");
        }
        owner.close().await;

        let mut app_url = its_url.clone();
        app_url.set_username(&role).expect("a username");
        app_url.set_password(Some("app")).expect("a password");

        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect(app_url.as_str())
            .await
            .expect("to connect as the application role");

        Shop {
            pool,
            here,
            elsewhere,
            host: Recorder::new(),
            name,
            role,
            admin,
            its_url: its_url.to_string(),
            extra_roles: parking_lot::Mutex::new(Vec::new()),
        }
    }

    /// A connection as a non-superuser owner of every table — what a host runs
    /// migrations as, and the only role forced row-level security applies to.
    ///
    /// The role `Shop::open` used is the cluster superuser, which bypasses row
    /// security outright, so a migration tested through it proves nothing
    /// about what a host's migration will reach.
    pub async fn migrator(&self) -> PgPool {
        let owner = format!("{}_owner", self.name);
        self.extra_roles.lock().push(owner.clone());

        for statement in [
            format!(r#"create role "{owner}" login password 'owner'"#),
            format!(r#"grant usage, create on schema public to "{owner}""#),
            format!(
                r#"do $$ declare r record; begin
                     for r in select tablename from pg_tables where schemaname = 'public' loop
                       execute format('alter table public.%I owner to "{owner}"', r.tablename);
                     end loop;
                   end $$"#
            ),
            // The functions and procedures too: a migration replacing one is
            // refused outright unless the role running it owns it.
            format!(
                r#"do $$ declare r record; begin
                     for r in select p.oid::regprocedure as signature
                              from pg_proc p
                              join pg_namespace n on n.oid = p.pronamespace
                              where n.nspname = 'public' loop
                       execute format('alter routine %s owner to "{owner}"', r.signature);
                     end loop;
                   end $$"#
            ),
        ] {
            let admin = PgPoolOptions::new()
                .max_connections(1)
                .connect(&self.its_url)
                .await
                .expect("the test database");
            admin
                .execute(statement.as_str())
                .await
                .expect("to hand the tables to a plain owner");
            admin.close().await;
        }

        let mut url = url::Url::parse(&self.its_url).expect("a database url");
        url.set_username(&owner).expect("a username");
        url.set_password(Some("owner")).expect("a password");

        PgPoolOptions::new()
            .max_connections(1)
            .connect(url.as_str())
            .await
            .expect("to connect as the owner")
    }

    pub fn ctx(&self) -> Ctx<'_> {
        Ctx::new(self.here, Actor::System, self.host.as_ref() as &dyn Host)
    }

    /// The same shop seen by somebody else's scope.
    pub fn theirs(&self) -> Ctx<'_> {
        Ctx::new(
            self.elsewhere,
            Actor::System,
            self.host.as_ref() as &dyn Host,
        )
    }

    pub fn ctx_as<'a>(&'a self, actor: Actor, host: &'a dyn Host) -> Ctx<'a> {
        Ctx::new(self.here, actor, host)
    }

    /// A transaction with the scope announced on it, which is the only kind the
    /// row-level security policies admit.
    pub async fn begin(&self) -> Tx<'static> {
        self.begin_as(self.here).await
    }

    pub async fn begin_as(&self, scope: Scope) -> Tx<'static> {
        let mut tx = self.pool.begin().await.expect("a transaction");
        sqlx::query("select set_config('app.scope', $1, true)")
            .bind(scope.0.to_string())
            .execute(&mut *tx)
            .await
            .expect("to announce the scope");
        tx
    }

    /// Drops the database and the role. Called by the test, because a `Drop`
    /// cannot await.
    pub async fn close(self) {
        let Shop {
            pool,
            admin,
            name,
            role,
            extra_roles,
            ..
        } = self;
        pool.close().await;
        let _ = admin
            .execute(format!(r#"drop database if exists "{name}" with (force)"#).as_str())
            .await;
        // Roles are cluster-wide, so one is left behind per test otherwise.
        let _ = admin
            .execute(format!(r#"drop role if exists "{role}""#).as_str())
            .await;
        for extra in extra_roles.into_inner() {
            let _ = admin
                .execute(format!(r#"drop role if exists "{extra}""#).as_str())
                .await;
        }
    }
}

/// A customer row that exists, for anything holding a foreign key to one.
pub async fn a_customer(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> tezgah::id::CustomerId {
    tezgah::customer::create(tx, ctx, tezgah::customer::NewCustomer::guest())
        .await
        .expect("a customer")
        .id
}

/// A currency this shop trades in. Nothing rounds an amount without one.
pub async fn a_currency(tx: &mut Tx<'_>, scope: Scope, code: &str, exponent: i16) {
    sqlx::query(
        "insert into currency (id, scope, code, exponent, symbol, symbol_native, name)
         values ($1, $2, $3, $4, 'x', 'x', $3)
         on conflict do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(scope.0)
    .bind(code)
    .bind(exponent)
    .execute(&mut **tx)
    .await
    .expect("a currency");
}

/// A shop with one thing in one cart, ready to be checked out.
///
/// The same seed the checkout tests grew: a currency, a payment provider, a
/// location, an inventory item the variant consumes, an address and an
/// idempotency key. Everything a `Checkout` reads before it asks a bank for
/// anything.
pub struct Shelf {
    pub cart_id: tezgah::id::CartId,
    pub location_id: tezgah::id::StockLocationId,
    pub variant_id: tezgah::id::VariantId,
    pub product_id: Uuid,
    pub inventory_item_id: tezgah::id::InventoryItemId,
}

/// `stocked` on the shelf, `quantity` in the cart, at ten lira each.
pub async fn a_cart_ready(shop: &Shop, stocked: i32, quantity: i32) -> Shelf {
    use rust_decimal_macros::dec;
    use tezgah::id::{CartId, VariantId};
    use tezgah::money::{Currency, Money};
    use tezgah::{cart, inventory};

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    sqlx::query(
        "insert into currency (id, scope, code, exponent, symbol, symbol_native, name)
         values ($1, $2, 'TRY', 2, 'x', 'x', 'Turkish lira')
         on conflict do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .execute(&mut *tx)
    .await
    .expect("a currency");

    sqlx::query(
        "insert into payment_provider (id, scope, code, is_enabled) values ($1, $2, $3, true)
         on conflict do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .bind("bank")
    .execute(&mut *tx)
    .await
    .expect("a payment provider");

    let location = inventory::create_stock_location(
        &mut tx,
        &ctx,
        inventory::NewStockLocation {
            name: format!("warehouse {}", Uuid::now_v7()),
            address: None,
        },
    )
    .await
    .expect("a location");

    let item = inventory::create_inventory_item(
        &mut tx,
        &ctx,
        inventory::NewInventoryItem {
            sku: Some(format!("sku-{}", Uuid::now_v7())),
            title: Some("a thing".into()),
            requires_shipping: true,
        },
    )
    .await
    .expect("an inventory item");

    inventory::set_stock(&mut tx, &ctx, item.id, location.id, stocked, 0)
        .await
        .expect("a level");

    let product = Uuid::now_v7();
    sqlx::query("insert into product (id, scope, handle, title) values ($1, $2, $3, $4)")
        .bind(product)
        .bind(shop.here.0)
        .bind(format!("thing-{product}"))
        .bind("A thing")
        .execute(&mut *tx)
        .await
        .expect("a product");

    let variant = VariantId::new();
    sqlx::query(
        "insert into product_variant (id, scope, product_id, title) values ($1, $2, $3, $4)",
    )
    .bind(variant.as_uuid())
    .bind(shop.here.0)
    .bind(product)
    .bind("The only one")
    .execute(&mut *tx)
    .await
    .expect("a variant");

    inventory::attach_inventory_item(&mut tx, &ctx, variant, item.id, 1)
        .await
        .expect("the variant to consume the item");

    let address = Uuid::now_v7();
    sqlx::query(
        "insert into cart_address (id, scope, address_1, city, country_code)
         values ($1, $2, '1 Example Street', 'Istanbul', 'TR')",
    )
    .bind(address)
    .bind(shop.here.0)
    .execute(&mut *tx)
    .await
    .expect("an address");

    let cart = CartId::new();
    sqlx::query(
        "insert into cart
             (id, scope, currency_code, email, shipping_address_id, idempotency_key)
         values ($1, $2, 'TRY', 'shopper@example.com', $3, $4)",
    )
    .bind(cart.as_uuid())
    .bind(shop.here.0)
    .bind(address)
    .bind(format!("key-{}", cart.as_uuid()))
    .execute(&mut *tx)
    .await
    .expect("a cart");

    if quantity > 0 {
        cart::add_line(
            &mut tx,
            &ctx,
            cart,
            cart::AddLine {
                variant_id: variant,
                quantity,
                unit_price: Money::new(dec!(10), Currency::parse("TRY").expect("a currency")),
                is_tax_inclusive: false,
            },
        )
        .await
        .expect("a line");
    }

    tx.commit().await.expect("to commit the seed");

    Shelf {
        cart_id: cart,
        location_id: location.id,
        variant_id: variant,
        product_id: product,
        inventory_item_id: item.id,
    }
}

/// A payment provider that authorises whatever it is asked for.
#[derive(Debug, Default)]
pub struct Teller;

#[async_trait]
impl tezgah::payment::PaymentProvider for Teller {
    fn code(&self) -> &'static str {
        "bank"
    }

    async fn create_session(
        &self,
        _: tezgah::payment::SessionRequest,
    ) -> tezgah::Result<tezgah::payment::SessionResponse> {
        Ok(tezgah::payment::SessionResponse {
            data: serde_json::json!({ "intent": "an-intent" }),
            status: tezgah::payment::SessionStatus::Pending,
        })
    }

    async fn authorize(
        &self,
        req: tezgah::payment::AuthorizeRequest,
    ) -> tezgah::Result<tezgah::payment::Authorization> {
        Ok(tezgah::payment::Authorization {
            status: tezgah::payment::AuthorizationStatus::Authorized,
            amount: Some(req.amount),
            data: serde_json::json!({ "intent": "an-intent" }),
            redirect: None,
            message: None,
            installment: None,
        })
    }

    async fn capture(
        &self,
        req: tezgah::payment::CaptureRequest,
    ) -> tezgah::Result<tezgah::payment::CaptureResult> {
        Ok(tezgah::payment::CaptureResult {
            amount: req.amount,
            data: serde_json::Value::Null,
        })
    }

    async fn refund(
        &self,
        req: tezgah::payment::RefundRequest,
    ) -> tezgah::Result<tezgah::payment::RefundResult> {
        Ok(tezgah::payment::RefundResult {
            amount: req.amount,
            data: serde_json::Value::Null,
        })
    }

    async fn cancel(&self, _: tezgah::payment::CancelRequest) -> tezgah::Result<()> {
        Ok(())
    }

    fn parse_webhook(
        &self,
        _: &[(String, String)],
        _: &[u8],
    ) -> tezgah::Result<tezgah::payment::WebhookEvent> {
        Err(tezgah::Error::invalid("this bank sends no webhooks"))
    }
}

/// A host that lets one named customer at their own things and nobody else at
/// them, so "only your own cart" can be asserted rather than assumed.
pub struct OnlyMine {
    pub customer: Uuid,
}

impl Authorizer for OnlyMine {
    fn authorize(&self, _: &Actor, _: Action, resource: &Resource) -> tezgah::Result<Permit> {
        // `None` is a row nobody owns yet, which is what a domain call hands
        // over before the storefront names whose it is. Refusing that would
        // refuse everybody rather than the stranger.
        let mine = match resource {
            Resource::Cart { customer, .. } | Resource::Order { customer, .. } => {
                customer.is_none_or(|whose| whose == self.customer)
            }
            _ => true,
        };

        if mine {
            Ok(Permit::granted())
        } else {
            Err(tezgah::Error::denied())
        }
    }
}

impl Clock for OnlyMine {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[async_trait]
impl AuditSink for OnlyMine {
    async fn record(&self, _: &mut Tx<'_>, _: AuditEntry) -> tezgah::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl EventSink for OnlyMine {
    async fn emit(&self, _: &mut Tx<'_>, _: Event) -> tezgah::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Jobs for OnlyMine {
    async fn enqueue(&self, _: &mut Tx<'_>, _: JobSpec) -> tezgah::Result<()> {
        Ok(())
    }
}

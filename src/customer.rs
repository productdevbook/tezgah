//! Who is buying: accounts, guests, their address books and the groups they
//! are priced by.
//!
//! A guest and an account are one table and `has_account` is the whole
//! difference, so a guest who signs up keeps their orders instead of becoming
//! a second person.
//!
//! Erasure is [`erase`] and never a delete: an order has to keep pointing
//! somewhere, so the row stays and the personal columns are emptied.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::error::{Error, Result};
use crate::id::{AddressId, CustomerGroupId, CustomerId};
use crate::page::{Cursor, Order, Page, Paging, Search};
use crate::payment;
use crate::ports::{Action, AuditEntry, Ctx, Event, Permit, Resource, Tx};

const COLUMNS: &str = "id, email, first_name, last_name, phone, company_name, has_account, \
                       metadata, anonymised_at, created_at, updated_at";

const ADDRESS_COLUMNS: &str = "id, customer_id, label, is_default_shipping, is_default_billing, company, first_name, \
     last_name, address_1, address_2, city, province, postal_code, country_code, phone, \
     metadata, created_at, updated_at";

/// Most groups one customer is read as belonging to. Callers match price rules
/// against the whole set, so this is a ceiling rather than a page.
const MAX_GROUPS: i64 = 200;

const GROUP_COLUMNS: &str = "id, name, metadata, created_at, updated_at";

const MEMBER_COLUMNS: &str = "c.id, c.email, c.first_name, c.last_name, c.phone, c.company_name, c.has_account, \
     c.metadata, c.anonymised_at, c.created_at, c.updated_at";

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Customer {
    pub id: CustomerId,
    pub email: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
    pub company_name: Option<String>,
    /// False for somebody who only ever checked out as a guest.
    pub has_account: bool,
    pub metadata: Option<serde_json::Value>,
    /// Set once [`erase`] has emptied the personal columns.
    pub anonymised_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Customer {
    pub fn is_anonymised(&self) -> bool {
        self.anonymised_at.is_some()
    }
}

#[derive(Debug, Clone, Default)]
pub struct NewCustomer {
    pub email: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
    pub company_name: Option<String>,
    pub has_account: bool,
    pub metadata: Option<serde_json::Value>,
}

impl NewCustomer {
    /// Somebody checking out without signing in.
    pub fn guest() -> Self {
        NewCustomer::default()
    }

    pub fn account(email: impl Into<String>) -> Self {
        NewCustomer {
            email: Some(email.into()),
            has_account: true,
            ..NewCustomer::default()
        }
    }
}

/// Every field left `None` is left alone.
#[derive(Debug, Clone, Default)]
pub struct CustomerPatch {
    pub email: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
    pub company_name: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct CustomerAddress {
    pub id: AddressId,
    pub customer_id: CustomerId,
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
    pub metadata: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct NewAddress {
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
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct CustomerGroup {
    pub id: CustomerGroupId,
    pub name: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

fn tidy(text: Option<String>) -> Option<String> {
    text.map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn tidy_email(text: Option<String>) -> Option<String> {
    tidy(text).map(|value| value.to_lowercase())
}

fn tidy_country(text: Option<String>) -> Result<Option<String>> {
    let code = tidy(text).map(|value| value.to_uppercase());
    match &code {
        Some(value) if value.len() != 2 || !value.chars().all(|c| c.is_ascii_alphabetic()) => {
            Err(Error::invalid("a country code is two letters"))
        }
        _ => Ok(code),
    }
}

pub async fn create(tx: &mut Tx<'_>, ctx: &Ctx<'_>, new: NewCustomer) -> Result<Customer> {
    let _: Permit = ctx.permit(Action::Write, Resource::Customer { id: None })?;

    let email = tidy_email(new.email);
    if new.has_account && email.is_none() {
        return Err(Error::invalid("an account needs an e-mail"));
    }

    let id = CustomerId::new();
    let customer = sqlx::query_as::<_, Customer>(&format!(
        "insert into customer
             (id, scope, email, first_name, last_name, phone, company_name, has_account, metadata)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         returning {COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(email)
    .bind(tidy(new.first_name))
    .bind(tidy(new.last_name))
    .bind(tidy(new.phone))
    .bind(tidy(new.company_name))
    .bind(new.has_account)
    .bind(new.metadata)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "customer",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "has_account": customer.has_account }),
        },
    )
    .await?;

    Ok(customer)
}

pub async fn get(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CustomerId) -> Result<Customer> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Customer {
            id: Some(id.as_uuid()),
        },
    )?;

    sqlx::query_as::<_, Customer>(&format!(
        "select {COLUMNS} from customer
         where scope = $1 and id = $2 and deleted_at is null"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("customer"))
}

pub async fn by_email(tx: &mut Tx<'_>, ctx: &Ctx<'_>, email: &str) -> Result<Customer> {
    let _: Permit = ctx.permit(Action::View, Resource::Customer { id: None })?;

    sqlx::query_as::<_, Customer>(&format!(
        "select {COLUMNS} from customer
         where scope = $1 and email = $2 and has_account and deleted_at is null"
    ))
    .bind(ctx.scope.0)
    .bind(email.trim().to_lowercase())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("customer"))
}

/// What narrows a listing of customers.
///
/// One field today, and a struct anyway: the alternative is a positional
/// argument that every caller and every test has to be changed for the day a
/// second one arrives — which is what this commit had to do to the two lists
/// that took theirs positionally.
#[derive(Debug, Clone, Default)]
pub struct CustomerFilter {
    /// Which end first. A back office opening Customers wants whoever
    /// arrived this week.
    pub order: Order,
    /// Matched against e-mail, first and last name, and company — the four
    /// ways somebody asks for a person. A guest with none of them set is
    /// findable through the order they left, not here.
    pub search: Option<Search>,
}

pub async fn list(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    filter: CustomerFilter,
    paging: Paging,
) -> Result<Page<Customer>> {
    let _: Permit = ctx.permit(Action::View, Resource::Customer { id: None })?;

    let (beyond, direction) = (filter.order.beyond(), filter.order.direction());

    let rows = sqlx::query_as::<_, Customer>(&format!(
        "select {COLUMNS} from customer
         where scope = $1
           and deleted_at is null
           and ($2::text is null
                or email ilike $2
                or first_name ilike $2
                or last_name ilike $2
                or company_name ilike $2)
           and ($3::timestamptz is null or (created_at, id) {beyond} ($3, $4))
         order by created_at {direction}, id {direction}
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(filter.search.as_ref().map(Search::pattern))
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

pub async fn update(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CustomerId,
    patch: CustomerPatch,
) -> Result<Customer> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Customer {
            id: Some(id.as_uuid()),
        },
    )?;

    let customer = sqlx::query_as::<_, Customer>(&format!(
        "update customer set
             email = coalesce($3, email),
             first_name = coalesce($4, first_name),
             last_name = coalesce($5, last_name),
             phone = coalesce($6, phone),
             company_name = coalesce($7, company_name),
             metadata = coalesce($8, metadata)
         where scope = $1 and id = $2 and deleted_at is null and anonymised_at is null
         returning {COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(tidy_email(patch.email))
    .bind(tidy(patch.first_name))
    .bind(tidy(patch.last_name))
    .bind(tidy(patch.phone))
    .bind(tidy(patch.company_name))
    .bind(patch.metadata)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("customer"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "customer",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "updated": true }),
        },
    )
    .await?;

    Ok(customer)
}

/// Soft, so a customer restored next week still owns their orders.
pub async fn delete(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CustomerId) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Delete,
        Resource::Customer {
            id: Some(id.as_uuid()),
        },
    )?;

    let done = sqlx::query(
        "update customer set deleted_at = $3
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("customer"));
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "customer",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "soft": true }),
        },
    )
    .await?;

    Ok(())
}

pub async fn add_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
    new: NewAddress,
) -> Result<CustomerAddress> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Customer {
            id: Some(customer_id.as_uuid()),
        },
    )?;

    let country = tidy_country(new.country_code)?;
    // The unique partial index allows one default of each kind, so the old one
    // is stood down in this transaction rather than left to collide.
    clear_defaults(
        tx,
        ctx,
        customer_id,
        new.is_default_shipping,
        new.is_default_billing,
        None,
    )
    .await?;

    let id = AddressId::new();
    let address = sqlx::query_as::<_, CustomerAddress>(&format!(
        "insert into customer_address
             (id, scope, customer_id, label, is_default_shipping, is_default_billing, company,
              first_name, last_name, address_1, address_2, city, province, postal_code,
              country_code, phone, metadata)
         select $1::uuid, $2::uuid, c.id, $4::text, $5::boolean, $6::boolean, $7::text, $8::text,
                $9::text, $10::text, $11::text, $12::text, $13::text, $14::text, $15::char(2),
                $16::text, $17::jsonb
         from customer c
         where c.scope = $2 and c.id = $3 and c.deleted_at is null
         returning {ADDRESS_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(customer_id.as_uuid())
    .bind(tidy(new.label))
    .bind(new.is_default_shipping)
    .bind(new.is_default_billing)
    .bind(tidy(new.company))
    .bind(tidy(new.first_name))
    .bind(tidy(new.last_name))
    .bind(tidy(new.address_1))
    .bind(tidy(new.address_2))
    .bind(tidy(new.city))
    .bind(tidy(new.province))
    .bind(tidy(new.postal_code))
    .bind(country)
    .bind(tidy(new.phone))
    .bind(new.metadata)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("customer"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "customer_address",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "customer": customer_id }),
        },
    )
    .await?;

    Ok(address)
}

/// One address, carrying whose it is so a caller can check before it writes.
pub async fn address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    address_id: AddressId,
) -> Result<CustomerAddress> {
    let _: Permit = ctx.permit(Action::View, Resource::Customer { id: None })?;

    sqlx::query_as::<_, CustomerAddress>(&format!(
        "select {ADDRESS_COLUMNS} from customer_address
         where scope = $1 and id = $2 and deleted_at is null"
    ))
    .bind(ctx.scope.0)
    .bind(address_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("address"))
}

pub async fn addresses(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
    paging: Paging,
) -> Result<Page<CustomerAddress>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Customer {
            id: Some(customer_id.as_uuid()),
        },
    )?;

    let rows = sqlx::query_as::<_, CustomerAddress>(&format!(
        "select {ADDRESS_COLUMNS} from customer_address
         where scope = $1
           and customer_id = $2
           and deleted_at is null
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(customer_id.as_uuid())
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

pub async fn update_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    address_id: AddressId,
    new: NewAddress,
) -> Result<CustomerAddress> {
    let _: Permit = ctx.permit(Action::Write, Resource::Customer { id: None })?;

    let country = tidy_country(new.country_code)?;
    let owner: Option<CustomerId> = sqlx::query_scalar(
        "select customer_id from customer_address
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(address_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;
    let owner = owner.ok_or_else(|| Error::not_found("address"))?;

    clear_defaults(
        tx,
        ctx,
        owner,
        new.is_default_shipping,
        new.is_default_billing,
        Some(address_id),
    )
    .await?;

    let address = sqlx::query_as::<_, CustomerAddress>(&format!(
        "update customer_address set
             label = $3,
             is_default_shipping = $4,
             is_default_billing = $5,
             company = $6,
             first_name = $7,
             last_name = $8,
             address_1 = $9,
             address_2 = $10,
             city = $11,
             province = $12,
             postal_code = $13,
             country_code = $14,
             phone = $15,
             metadata = coalesce($16, metadata)
         where scope = $1 and id = $2 and deleted_at is null
         returning {ADDRESS_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(address_id.as_uuid())
    .bind(tidy(new.label))
    .bind(new.is_default_shipping)
    .bind(new.is_default_billing)
    .bind(tidy(new.company))
    .bind(tidy(new.first_name))
    .bind(tidy(new.last_name))
    .bind(tidy(new.address_1))
    .bind(tidy(new.address_2))
    .bind(tidy(new.city))
    .bind(tidy(new.province))
    .bind(tidy(new.postal_code))
    .bind(country)
    .bind(tidy(new.phone))
    .bind(new.metadata)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("address"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "customer_address",
            entity_id: address_id.as_uuid(),
            summary: serde_json::json!({ "updated": true }),
        },
    )
    .await?;

    Ok(address)
}

pub async fn delete_address(tx: &mut Tx<'_>, ctx: &Ctx<'_>, address_id: AddressId) -> Result<()> {
    let _: Permit = ctx.permit(Action::Delete, Resource::Customer { id: None })?;

    let done = sqlx::query(
        "update customer_address
            set deleted_at = $3, is_default_shipping = false, is_default_billing = false
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(address_id.as_uuid())
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("address"));
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "customer_address",
            entity_id: address_id.as_uuid(),
            summary: serde_json::json!({ "soft": true }),
        },
    )
    .await?;

    Ok(())
}

async fn clear_defaults(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
    shipping: bool,
    billing: bool,
    except: Option<AddressId>,
) -> Result<()> {
    if !shipping && !billing {
        return Ok(());
    }

    sqlx::query(
        "update customer_address set
             is_default_shipping = is_default_shipping and not $3,
             is_default_billing = is_default_billing and not $4
         where scope = $1
           and customer_id = $2
           and deleted_at is null
           and ($5::uuid is null or id <> $5)",
    )
    .bind(ctx.scope.0)
    .bind(customer_id.as_uuid())
    .bind(shipping)
    .bind(billing)
    .bind(except.map(AddressId::as_uuid))
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub async fn create_group(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    name: &str,
    metadata: Option<serde_json::Value>,
) -> Result<CustomerGroup> {
    let _: Permit = ctx.permit(Action::Write, Resource::Customer { id: None })?;

    let name = name.trim();
    if name.is_empty() {
        return Err(Error::invalid("a group needs a name"));
    }

    let id = CustomerGroupId::new();
    let group = sqlx::query_as::<_, CustomerGroup>(&format!(
        "insert into customer_group (id, scope, name, metadata)
         values ($1, $2, $3, $4)
         returning {GROUP_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(name)
    .bind(metadata)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "customer_group",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "name": group.name }),
        },
    )
    .await?;

    Ok(group)
}

pub async fn groups(tx: &mut Tx<'_>, ctx: &Ctx<'_>, paging: Paging) -> Result<Page<CustomerGroup>> {
    let _: Permit = ctx.permit(Action::View, Resource::Customer { id: None })?;

    let rows = sqlx::query_as::<_, CustomerGroup>(&format!(
        "select {GROUP_COLUMNS} from customer_group
         where scope = $1
           and deleted_at is null
           and ($2::timestamptz is null or (created_at, id) > ($2, $3))
         order by created_at, id
         limit $4"
    ))
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

pub async fn rename_group(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    group_id: CustomerGroupId,
    name: &str,
) -> Result<CustomerGroup> {
    let _: Permit = ctx.permit(Action::Write, Resource::Customer { id: None })?;

    let name = name.trim();
    if name.is_empty() {
        return Err(Error::invalid("a group needs a name"));
    }

    sqlx::query_as::<_, CustomerGroup>(&format!(
        "update customer_group set name = $3
         where scope = $1 and id = $2 and deleted_at is null
         returning {GROUP_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(group_id.as_uuid())
    .bind(name)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("customer group"))
}

pub async fn delete_group(tx: &mut Tx<'_>, ctx: &Ctx<'_>, group_id: CustomerGroupId) -> Result<()> {
    let _: Permit = ctx.permit(Action::Delete, Resource::Customer { id: None })?;

    let done = sqlx::query(
        "update customer_group set deleted_at = $3
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(group_id.as_uuid())
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("customer group"));
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "customer_group",
            entity_id: group_id.as_uuid(),
            summary: serde_json::json!({ "soft": true }),
        },
    )
    .await?;

    Ok(())
}

pub async fn join_group(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
    group_id: CustomerGroupId,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Customer {
            id: Some(customer_id.as_uuid()),
        },
    )?;

    let done = sqlx::query(
        "insert into customer_group_customer (id, scope, customer_group_id, customer_id)
         select $1::uuid, $2::uuid, g.id, c.id
         from customer_group g
         join customer c on c.scope = g.scope and c.id = $4 and c.deleted_at is null
         where g.scope = $2 and g.id = $3 and g.deleted_at is null
         on conflict (scope, customer_group_id, customer_id) do nothing",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(group_id.as_uuid())
    .bind(customer_id.as_uuid())
    .execute(&mut **tx)
    .await?;

    if done.rows_affected() == 0 {
        let known: Option<uuid::Uuid> = sqlx::query_scalar(
            "select customer_id from customer_group_customer
             where scope = $1 and customer_group_id = $2 and customer_id = $3",
        )
        .bind(ctx.scope.0)
        .bind(group_id.as_uuid())
        .bind(customer_id.as_uuid())
        .fetch_optional(&mut **tx)
        .await?;

        if known.is_none() {
            return Err(Error::not_found("customer group"));
        }
        return Ok(());
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "customer_group_customer",
            entity_id: customer_id.as_uuid(),
            summary: serde_json::json!({ "group": group_id }),
        },
    )
    .await?;

    Ok(())
}

pub async fn leave_group(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
    group_id: CustomerGroupId,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Customer {
            id: Some(customer_id.as_uuid()),
        },
    )?;

    let done = sqlx::query(
        "delete from customer_group_customer
         where scope = $1 and customer_group_id = $2 and customer_id = $3",
    )
    .bind(ctx.scope.0)
    .bind(group_id.as_uuid())
    .bind(customer_id.as_uuid())
    .execute(&mut **tx)
    .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("customer group"));
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "customer_group_customer",
            entity_id: customer_id.as_uuid(),
            summary: serde_json::json!({ "group": group_id }),
        },
    )
    .await?;

    Ok(())
}

pub async fn members(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    group_id: CustomerGroupId,
    paging: Paging,
) -> Result<Page<Customer>> {
    let _: Permit = ctx.permit(Action::View, Resource::Customer { id: None })?;

    let rows = sqlx::query_as::<_, Customer>(&format!(
        "select {MEMBER_COLUMNS} from customer c
         join customer_group_customer m
           on m.scope = c.scope and m.customer_id = c.id
         where c.scope = $1
           and m.customer_group_id = $2
           and c.deleted_at is null
           and ($3::timestamptz is null or (c.created_at, c.id) > ($3, $4))
         order by c.created_at, c.id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(group_id.as_uuid())
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

pub async fn group_ids(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
) -> Result<Vec<CustomerGroupId>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Customer {
            id: Some(customer_id.as_uuid()),
        },
    )?;

    let rows: Vec<CustomerGroupId> = sqlx::query_scalar(
        "select m.customer_group_id
         from customer_group_customer m
         join customer_group g on g.scope = m.scope and g.id = m.customer_group_id
         where m.scope = $1 and m.customer_id = $2 and g.deleted_at is null
         order by m.customer_group_id
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(customer_id.as_uuid())
    .bind(MAX_GROUPS)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

/// Everything held about one customer, as one document: what a subject access
/// request is answered with.
///
/// Rows arrive whole and lose only `scope`, which is the shop's fact rather
/// than the customer's.
pub async fn export(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
) -> Result<serde_json::Value> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Customer {
            id: Some(customer_id.as_uuid()),
        },
    )?;

    let document: Option<serde_json::Value> = sqlx::query_scalar(
        r#"select jsonb_build_object(
               'customer', to_jsonb(c) - 'scope',
               'addresses', (
                   select coalesce(jsonb_agg(to_jsonb(a) - 'scope' order by a.created_at), '[]'::jsonb)
                   from customer_address a
                   where a.scope = c.scope and a.customer_id = c.id
               ),
               'groups', (
                   select coalesce(jsonb_agg(to_jsonb(g) - 'scope' order by g.created_at), '[]'::jsonb)
                   from customer_group g
                   join customer_group_customer m
                     on m.scope = g.scope and m.customer_group_id = g.id
                   where m.scope = c.scope and m.customer_id = c.id
               ),
               'carts', (
                   select coalesce(jsonb_agg(
                       (to_jsonb(t) - 'scope') || jsonb_build_object('line_items', (
                           select coalesce(jsonb_agg(to_jsonb(l) - 'scope' order by l.created_at), '[]'::jsonb)
                           from cart_line_item l
                           where l.scope = t.scope and l.cart_id = t.id
                       ))
                       order by t.created_at), '[]'::jsonb)
                   from cart t
                   where t.scope = c.scope and t.customer_id = c.id
               ),
               'orders', (
                   select coalesce(jsonb_agg(to_jsonb(o) - 'scope' order by o.created_at), '[]'::jsonb)
                   from "order" o
                   where o.scope = c.scope and o.customer_id = c.id
               )
           )
           from customer c
           where c.scope = $1 and c.id = $2"#,
    )
    .bind(ctx.scope.0)
    .bind(customer_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    let document = document.ok_or_else(|| Error::not_found("customer"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::View,
            entity: "customer",
            entity_id: customer_id.as_uuid(),
            summary: serde_json::json!({ "exported": true }),
        },
    )
    .await?;

    Ok(document)
}

/// Empties the personal columns and leaves the row, because an order still has
/// to point somewhere.
///
/// Addresses go with it: an address is the customer, written out longhand.
pub async fn erase(tx: &mut Tx<'_>, ctx: &Ctx<'_>, customer_id: CustomerId) -> Result<Customer> {
    let _: Permit = ctx.permit(
        Action::Delete,
        Resource::Customer {
            id: Some(customer_id.as_uuid()),
        },
    )?;

    let now = ctx.now();
    let customer = sqlx::query_as::<_, Customer>(&format!(
        "update customer set
             email = null,
             phone = null,
             first_name = null,
             last_name = null,
             company_name = null,
             metadata = null,
             anonymised_at = coalesce(anonymised_at, $3)
         where scope = $1 and id = $2
         returning {COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(customer_id.as_uuid())
    .bind(now)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("customer"))?;

    sqlx::query(
        "update customer_address set
             company = null, first_name = null, last_name = null, address_1 = null,
             address_2 = null, city = null, province = null, postal_code = null,
             phone = null, metadata = null, label = null,
             is_default_shipping = false, is_default_billing = false,
             deleted_at = coalesce(deleted_at, $3)
         where scope = $1 and customer_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(customer_id.as_uuid())
    .bind(now)
    .execute(&mut **tx)
    .await?;

    sqlx::query("update cart set email = null where scope = $1 and customer_id = $2")
        .bind(ctx.scope.0)
        .bind(customer_id.as_uuid())
        .execute(&mut **tx)
        .await?;

    // The saved-card reference at a provider is not tezgah's data to keep
    // once the customer that owns it is gone: the token itself stays with
    // the provider (kasapay's, or the host's, to remove), but the email and
    // the reference to it must not survive under the id this just erased.
    payment::scrub_account_holders_for_customer(tx, ctx, customer_id).await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "customer",
            entity_id: customer_id.as_uuid(),
            summary: serde_json::json!({ "anonymised": true }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "customer.anonymised",
            entity_id: customer_id.as_uuid(),
            payload: serde_json::json!({ "at": now }),
        },
    )
    .await?;

    Ok(customer)
}

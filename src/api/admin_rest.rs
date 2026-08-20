//! The back office's settings surfaces: customers, promotions, tax, and the
//! shop's own regions and channels.
//!
//! Catalogue and orders are declared beside this in their own modules. What is
//! here is what a shop configures rather than what it sells or ships.
//!
//! Every handler is a thin translation: an input a host can deserialise from a
//! request, a call into the domain, and a view on the way back. The permission
//! is asked for by the domain call itself, which is why nothing here holds a
//! `Permit` — one that never reached the domain would prove nothing.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::customer;
use crate::error::{Error, Result};
use crate::id::{
    AddressId, CampaignId, CustomerGroupId, CustomerId, PromotionId, PublishableKeyId, RegionId,
    SalesChannelId, StoreId, TaxRateId, TaxRegionId, WorkflowRunId,
};
use crate::money::Currency;
use crate::page::{Page, Paging};
use crate::ports::{Action, Ctx, Tx};
use crate::promotion;
use crate::store;
use crate::tax;
use crate::workflow;

use super::{Method, Route, Surface};

/// What every listing takes. Kept in one type so a collection cannot quietly
/// grow a paging convention of its own.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct List {
    pub after: Option<String>,
    pub limit: Option<u32>,
}

impl List {
    fn paging(&self) -> Result<Paging> {
        let limit = self.limit.unwrap_or(crate::page::DEFAULT_LIMIT);
        match &self.after {
            Some(text) => Ok(Paging::after(crate::page::Cursor::decode(text)?, limit)),
            None => Ok(Paging::first(limit)),
        }
    }
}

fn map<T, U>(page: Page<T>, into: impl Fn(T) -> U) -> Page<U> {
    Page {
        items: page.items.into_iter().map(into).collect(),
        next: page.next,
    }
}

// ---------------------------------------------------------------- customers

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct CustomerView {
    pub id: CustomerId,
    pub email: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
    pub company_name: Option<String>,
    pub has_account: bool,
    pub anonymised: bool,
    pub metadata: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<customer::Customer> for CustomerView {
    fn from(row: customer::Customer) -> Self {
        CustomerView {
            anonymised: row.is_anonymised(),
            id: row.id,
            email: row.email,
            first_name: row.first_name,
            last_name: row.last_name,
            phone: row.phone,
            company_name: row.company_name,
            has_account: row.has_account,
            metadata: row.metadata,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCustomer {
    pub email: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
    pub company_name: Option<String>,
    #[serde(default)]
    pub has_account: bool,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateCustomer {
    pub email: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
    pub company_name: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn list_customers(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: List,
) -> Result<Page<CustomerView>> {
    Ok(map(
        customer::list(tx, ctx, query.paging()?).await?,
        CustomerView::from,
    ))
}

pub async fn create_customer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreateCustomer,
) -> Result<CustomerView> {
    let new = customer::NewCustomer {
        email: body.email,
        first_name: body.first_name,
        last_name: body.last_name,
        phone: body.phone,
        company_name: body.company_name,
        has_account: body.has_account,
        metadata: body.metadata,
    };
    Ok(CustomerView::from(customer::create(tx, ctx, new).await?))
}

pub async fn get_customer(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CustomerId) -> Result<CustomerView> {
    Ok(CustomerView::from(customer::get(tx, ctx, id).await?))
}

pub async fn update_customer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CustomerId,
    body: UpdateCustomer,
) -> Result<CustomerView> {
    let patch = customer::CustomerPatch {
        email: body.email,
        first_name: body.first_name,
        last_name: body.last_name,
        phone: body.phone,
        company_name: body.company_name,
        metadata: body.metadata,
    };
    Ok(CustomerView::from(
        customer::update(tx, ctx, id, patch).await?,
    ))
}

pub async fn delete_customer(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CustomerId) -> Result<()> {
    customer::delete(tx, ctx, id).await
}

/// Everything held about one person, for a subject access request.
pub async fn export_customer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CustomerId,
) -> Result<serde_json::Value> {
    customer::export(tx, ctx, id).await
}

pub async fn erase_customer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CustomerId,
) -> Result<CustomerView> {
    Ok(CustomerView::from(customer::erase(tx, ctx, id).await?))
}

// ---------------------------------------------------------------- addresses

#[derive(Debug, Clone, Serialize)]
pub struct AddressView {
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

impl From<customer::CustomerAddress> for AddressView {
    fn from(row: customer::CustomerAddress) -> Self {
        AddressView {
            id: row.id,
            customer_id: row.customer_id,
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
            metadata: row.metadata,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
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
    pub metadata: Option<serde_json::Value>,
}

impl From<WriteAddress> for customer::NewAddress {
    fn from(body: WriteAddress) -> Self {
        customer::NewAddress {
            label: body.label,
            is_default_shipping: body.is_default_shipping,
            is_default_billing: body.is_default_billing,
            company: body.company,
            first_name: body.first_name,
            last_name: body.last_name,
            address_1: body.address_1,
            address_2: body.address_2,
            city: body.city,
            province: body.province,
            postal_code: body.postal_code,
            country_code: body.country_code,
            phone: body.phone,
            metadata: body.metadata,
        }
    }
}

pub async fn list_addresses(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
    query: List,
) -> Result<Page<AddressView>> {
    Ok(map(
        customer::addresses(tx, ctx, customer_id, query.paging()?).await?,
        AddressView::from,
    ))
}

pub async fn add_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
    body: WriteAddress,
) -> Result<AddressView> {
    Ok(AddressView::from(
        customer::add_address(tx, ctx, customer_id, body.into()).await?,
    ))
}

pub async fn update_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    address_id: AddressId,
    body: WriteAddress,
) -> Result<AddressView> {
    Ok(AddressView::from(
        customer::update_address(tx, ctx, address_id, body.into()).await?,
    ))
}

pub async fn delete_address(tx: &mut Tx<'_>, ctx: &Ctx<'_>, address_id: AddressId) -> Result<()> {
    customer::delete_address(tx, ctx, address_id).await
}

// ----------------------------------------------------------- customer groups

#[derive(Debug, Clone, Serialize)]
pub struct CustomerGroupView {
    pub id: CustomerGroupId,
    pub name: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<customer::CustomerGroup> for CustomerGroupView {
    fn from(row: customer::CustomerGroup) -> Self {
        CustomerGroupView {
            id: row.id,
            name: row.name,
            metadata: row.metadata,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteGroup {
    pub name: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupMember {
    pub customer_id: CustomerId,
}

pub async fn list_groups(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: List,
) -> Result<Page<CustomerGroupView>> {
    Ok(map(
        customer::groups(tx, ctx, query.paging()?).await?,
        CustomerGroupView::from,
    ))
}

pub async fn create_group(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: WriteGroup,
) -> Result<CustomerGroupView> {
    Ok(CustomerGroupView::from(
        customer::create_group(tx, ctx, &body.name, body.metadata).await?,
    ))
}

pub async fn rename_group(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    group_id: CustomerGroupId,
    body: WriteGroup,
) -> Result<CustomerGroupView> {
    Ok(CustomerGroupView::from(
        customer::rename_group(tx, ctx, group_id, &body.name).await?,
    ))
}

pub async fn delete_group(tx: &mut Tx<'_>, ctx: &Ctx<'_>, group_id: CustomerGroupId) -> Result<()> {
    customer::delete_group(tx, ctx, group_id).await
}

pub async fn list_group_members(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    group_id: CustomerGroupId,
    query: List,
) -> Result<Page<CustomerView>> {
    Ok(map(
        customer::members(tx, ctx, group_id, query.paging()?).await?,
        CustomerView::from,
    ))
}

pub async fn add_group_member(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    group_id: CustomerGroupId,
    body: GroupMember,
) -> Result<()> {
    customer::join_group(tx, ctx, body.customer_id, group_id).await
}

pub async fn remove_group_member(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    group_id: CustomerGroupId,
    customer_id: CustomerId,
) -> Result<()> {
    customer::leave_group(tx, ctx, customer_id, group_id).await
}

// --------------------------------------------------------------- promotions

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct PromotionView {
    pub id: PromotionId,
    pub campaign_id: Option<CampaignId>,
    pub code: String,
    pub kind: String,
    pub status: String,
    pub is_automatic: bool,
    pub usage_limit: Option<i32>,
    pub used: i32,
    pub customer_usage_limit: Option<i32>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<promotion::Promotion> for PromotionView {
    fn from(row: promotion::Promotion) -> Self {
        PromotionView {
            id: row.id,
            campaign_id: row.campaign_id,
            code: row.code,
            kind: row.kind,
            status: row.status,
            is_automatic: row.is_automatic,
            usage_limit: row.usage_limit,
            used: row.used,
            customer_usage_limit: row.customer_usage_limit,
            metadata: row.metadata,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplicationMethodView {
    pub id: Uuid,
    pub promotion_id: PromotionId,
    pub kind: String,
    pub target_type: String,
    pub allocation: Option<String>,
    pub value: Option<Decimal>,
    pub currency_code: Option<String>,
    pub max_quantity: Option<i32>,
    pub apply_to_quantity: Option<i32>,
    pub buy_rules_min_quantity: Option<i32>,
}

impl From<promotion::ApplicationMethod> for ApplicationMethodView {
    fn from(row: promotion::ApplicationMethod) -> Self {
        ApplicationMethodView {
            id: row.id,
            promotion_id: row.promotion_id,
            kind: row.kind,
            target_type: row.target_type,
            allocation: row.allocation,
            value: row.value,
            currency_code: row.currency_code,
            max_quantity: row.max_quantity,
            apply_to_quantity: row.apply_to_quantity,
            buy_rules_min_quantity: row.buy_rules_min_quantity,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleView {
    pub id: Uuid,
    pub attribute: String,
    pub operator: String,
    pub allowed_values: Vec<String>,
    pub description: Option<String>,
}

impl From<promotion::Rule> for RuleView {
    fn from(row: promotion::Rule) -> Self {
        RuleView {
            id: row.id,
            attribute: row.attribute,
            operator: row.operator,
            allowed_values: row.allowed_values,
            description: row.description,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePromotion {
    pub code: String,
    pub kind: promotion::PromotionKind,
    pub status: promotion::Status,
    #[serde(default)]
    pub is_automatic: bool,
    pub campaign_id: Option<CampaignId>,
    pub usage_limit: Option<i32>,
    pub customer_usage_limit: Option<i32>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetStatus {
    pub status: promotion::Status,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetApplicationMethod {
    pub kind: promotion::MethodKind,
    pub target_type: promotion::TargetKind,
    pub allocation: Option<promotion::Allocation>,
    pub value: Decimal,
    pub currency_code: Option<String>,
    pub max_quantity: Option<i32>,
    pub apply_to_quantity: Option<i32>,
    pub buy_rules_min_quantity: Option<i32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddRule {
    pub attribute: String,
    pub operator: promotion::Operator,
    pub allowed_values: Vec<String>,
    pub description: Option<String>,
}

/// The three rule sets, as they appear in the path.
fn rule_kind(segment: &str) -> Result<promotion::RuleKind> {
    match segment {
        "rules" => Ok(promotion::RuleKind::Eligibility),
        "target-rules" => Ok(promotion::RuleKind::Target),
        "buy-rules" => Ok(promotion::RuleKind::Buy),
        other => Err(Error::invalid(format!("{other:?} is not a rule set"))),
    }
}

fn currency(code: Option<String>) -> Result<Option<Currency>> {
    match code {
        Some(text) => Currency::parse(&text).map(Some),
        None => Ok(None),
    }
}

pub async fn list_promotions(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: List,
) -> Result<Page<PromotionView>> {
    Ok(map(
        promotion::promotions(tx, ctx, query.paging()?).await?,
        PromotionView::from,
    ))
}

pub async fn create_promotion(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreatePromotion,
) -> Result<PromotionView> {
    let new = promotion::NewPromotion {
        code: body.code,
        kind: body.kind,
        status: body.status,
        is_automatic: body.is_automatic,
        campaign_id: body.campaign_id,
        usage_limit: body.usage_limit,
        customer_usage_limit: body.customer_usage_limit,
        metadata: body.metadata,
    };
    Ok(PromotionView::from(
        promotion::create_promotion(tx, ctx, new).await?,
    ))
}

pub async fn get_promotion(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PromotionId,
) -> Result<PromotionView> {
    Ok(PromotionView::from(
        promotion::promotion(tx, ctx, id).await?,
    ))
}

#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePromotion {
    pub code: Option<String>,
    pub is_automatic: Option<bool>,
    pub usage_limit: Option<i32>,
    pub customer_usage_limit: Option<i32>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn update_promotion(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PromotionId,
    body: UpdatePromotion,
) -> Result<PromotionView> {
    let patch = promotion::PromotionPatch {
        code: body.code,
        is_automatic: body.is_automatic,
        usage_limit: body.usage_limit,
        customer_usage_limit: body.customer_usage_limit,
        metadata: body.metadata,
    };
    Ok(PromotionView::from(
        promotion::update_promotion(tx, ctx, id, patch).await?,
    ))
}

pub async fn set_promotion_status(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PromotionId,
    body: SetStatus,
) -> Result<PromotionView> {
    Ok(PromotionView::from(
        promotion::set_status(tx, ctx, id, body.status).await?,
    ))
}

pub async fn delete_promotion(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: PromotionId) -> Result<()> {
    promotion::delete_promotion(tx, ctx, id).await
}

pub async fn set_application_method(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PromotionId,
    body: SetApplicationMethod,
) -> Result<ApplicationMethodView> {
    let new = promotion::NewApplicationMethod {
        promotion_id: id,
        kind: body.kind,
        target_type: body.target_type,
        allocation: body.allocation,
        value: body.value,
        currency_code: currency(body.currency_code)?,
        max_quantity: body.max_quantity,
        apply_to_quantity: body.apply_to_quantity,
        buy_rules_min_quantity: body.buy_rules_min_quantity,
    };
    Ok(ApplicationMethodView::from(
        promotion::set_application_method(tx, ctx, new).await?,
    ))
}

pub async fn add_promotion_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PromotionId,
    rule_type: &str,
    body: AddRule,
) -> Result<RuleView> {
    let new = promotion::NewRule {
        attribute: body.attribute,
        operator: body.operator,
        allowed_values: body.allowed_values,
        description: body.description,
    };
    Ok(RuleView::from(
        promotion::add_rule(tx, ctx, id, rule_kind(rule_type)?, new).await?,
    ))
}

/// Bounded by how many rules one promotion carries, which is configuration
/// rather than a customer's data.
pub async fn list_promotion_rules(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PromotionId,
    rule_type: &str,
) -> Result<Vec<RuleView>> {
    Ok(promotion::rules(tx, ctx, id, rule_kind(rule_type)?)
        .await?
        .into_iter()
        .map(RuleView::from)
        .collect())
}

pub async fn delete_promotion_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PromotionId,
    rule_type: &str,
    rule_id: Uuid,
) -> Result<()> {
    promotion::remove_rule(tx, ctx, id, rule_kind(rule_type)?, rule_id).await
}

// ---------------------------------------------------------------- campaigns

#[derive(Debug, Clone, Serialize)]
pub struct CampaignView {
    pub id: CampaignId,
    pub identifier: String,
    pub name: String,
    pub description: Option<String>,
    pub starts_at: Option<chrono::DateTime<chrono::Utc>>,
    pub ends_at: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<promotion::Campaign> for CampaignView {
    fn from(row: promotion::Campaign) -> Self {
        CampaignView {
            id: row.id,
            identifier: row.identifier,
            name: row.name,
            description: row.description,
            starts_at: row.starts_at,
            ends_at: row.ends_at,
            metadata: row.metadata,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CampaignBudgetView {
    pub id: Uuid,
    pub campaign_id: CampaignId,
    pub kind: String,
    pub cap: Option<Decimal>,
    pub used: Decimal,
    pub currency_code: Option<String>,
    pub attribute: Option<String>,
}

impl From<promotion::CampaignBudget> for CampaignBudgetView {
    fn from(row: promotion::CampaignBudget) -> Self {
        CampaignBudgetView {
            id: row.id,
            campaign_id: row.campaign_id,
            kind: row.kind,
            cap: row.cap,
            used: row.used,
            currency_code: row.currency_code,
            attribute: row.attribute,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CampaignBudgetUsageView {
    pub id: Uuid,
    pub campaign_budget_id: Uuid,
    pub attribute_value: String,
    pub used: Decimal,
    pub cap: Option<Decimal>,
}

impl From<promotion::CampaignBudgetUsage> for CampaignBudgetUsageView {
    fn from(row: promotion::CampaignBudgetUsage) -> Self {
        CampaignBudgetUsageView {
            id: row.id,
            campaign_budget_id: row.campaign_budget_id,
            attribute_value: row.attribute_value,
            used: row.used,
            cap: row.cap,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCampaign {
    pub identifier: String,
    pub name: String,
    pub description: Option<String>,
    pub starts_at: Option<chrono::DateTime<chrono::Utc>>,
    pub ends_at: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetBudget {
    pub kind: promotion::BudgetKind,
    pub cap: Option<Decimal>,
    pub currency_code: Option<String>,
    pub attribute: Option<String>,
}

pub async fn list_campaigns(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: List,
) -> Result<Page<CampaignView>> {
    Ok(map(
        promotion::campaigns(tx, ctx, query.paging()?).await?,
        CampaignView::from,
    ))
}

pub async fn create_campaign(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreateCampaign,
) -> Result<CampaignView> {
    let new = promotion::NewCampaign {
        identifier: body.identifier,
        name: body.name,
        description: body.description,
        starts_at: body.starts_at,
        ends_at: body.ends_at,
        metadata: body.metadata,
    };
    Ok(CampaignView::from(
        promotion::create_campaign(tx, ctx, new).await?,
    ))
}

pub async fn get_campaign(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CampaignId) -> Result<CampaignView> {
    Ok(CampaignView::from(promotion::campaign(tx, ctx, id).await?))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateCampaign {
    pub identifier: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub starts_at: Option<chrono::DateTime<chrono::Utc>>,
    pub ends_at: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn update_campaign(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CampaignId,
    body: UpdateCampaign,
) -> Result<CampaignView> {
    let patch = promotion::CampaignPatch {
        identifier: body.identifier,
        name: body.name,
        description: body.description,
        starts_at: body.starts_at,
        ends_at: body.ends_at,
        metadata: body.metadata,
    };
    Ok(CampaignView::from(
        promotion::update_campaign(tx, ctx, id, patch).await?,
    ))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachPromotion {
    pub promotion_id: PromotionId,
}

pub async fn add_campaign_promotion(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    campaign_id: CampaignId,
    body: AttachPromotion,
) -> Result<PromotionView> {
    Ok(PromotionView::from(
        promotion::attach_promotion(tx, ctx, campaign_id, body.promotion_id).await?,
    ))
}

pub async fn remove_campaign_promotion(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    campaign_id: CampaignId,
    promotion_id: PromotionId,
) -> Result<PromotionView> {
    Ok(PromotionView::from(
        promotion::detach_promotion(tx, ctx, campaign_id, promotion_id).await?,
    ))
}

pub async fn set_campaign_budget(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    campaign_id: CampaignId,
    body: SetBudget,
) -> Result<CampaignBudgetView> {
    let new = promotion::NewCampaignBudget {
        campaign_id,
        kind: body.kind,
        cap: body.cap,
        currency_code: currency(body.currency_code)?,
        attribute: body.attribute,
    };
    Ok(CampaignBudgetView::from(
        promotion::set_campaign_budget(tx, ctx, new).await?,
    ))
}

/// Bounded the way a promotion's own rule sets are: configuration a shop
/// looks over, not a customer's own data.
pub async fn list_campaign_budget_usage(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    campaign_id: CampaignId,
) -> Result<Vec<CampaignBudgetUsageView>> {
    Ok(promotion::campaign_budget_usage(tx, ctx, campaign_id)
        .await?
        .into_iter()
        .map(CampaignBudgetUsageView::from)
        .collect())
}

// ---------------------------------------------------------------------- tax

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct TaxRegionView {
    pub id: TaxRegionId,
    pub country_code: String,
    pub province_code: Option<String>,
    pub parent_id: Option<TaxRegionId>,
    pub provider: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<tax::TaxRegion> for TaxRegionView {
    fn from(row: tax::TaxRegion) -> Self {
        TaxRegionView {
            id: row.id,
            country_code: row.country_code,
            province_code: row.province_code,
            parent_id: row.parent_id,
            provider: row.provider,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct TaxRateView {
    pub id: TaxRateId,
    pub tax_region_id: TaxRegionId,
    pub rate: Decimal,
    pub code: Option<String>,
    pub name: String,
    pub is_default: bool,
    pub is_combinable: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<tax::TaxRate> for TaxRateView {
    fn from(row: tax::TaxRate) -> Self {
        TaxRateView {
            id: row.id,
            tax_region_id: row.tax_region_id,
            rate: row.rate,
            code: row.code,
            name: row.name,
            is_default: row.is_default,
            is_combinable: row.is_combinable,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct TaxRateRuleView {
    pub id: Uuid,
    pub tax_rate_id: TaxRateId,
    pub reference: String,
    pub reference_id: Uuid,
}

impl From<tax::TaxRateRuleRow> for TaxRateRuleView {
    fn from(row: tax::TaxRateRuleRow) -> Self {
        TaxRateRuleView {
            id: row.id,
            tax_rate_id: row.tax_rate_id,
            reference: row.reference,
            reference_id: row.reference_id,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTaxRegion {
    pub country_code: String,
    pub province_code: Option<String>,
    pub parent_id: Option<TaxRegionId>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTaxRate {
    pub tax_region_id: TaxRegionId,
    pub rate: Decimal,
    pub code: Option<String>,
    pub name: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub is_combinable: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListTaxRates {
    pub after: Option<String>,
    pub limit: Option<u32>,
    pub tax_region_id: Option<TaxRegionId>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTaxRateRule {
    pub reference: tax::TaxReference,
    pub reference_id: Uuid,
}

pub async fn list_tax_regions(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: List,
) -> Result<Page<TaxRegionView>> {
    Ok(map(
        tax::tax_regions(tx, ctx, query.paging()?).await?,
        TaxRegionView::from,
    ))
}

pub async fn create_tax_region(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreateTaxRegion,
) -> Result<TaxRegionView> {
    let new = tax::NewTaxRegion {
        country_code: body.country_code,
        province_code: body.province_code,
        parent_id: body.parent_id,
        provider: body.provider,
    };
    Ok(TaxRegionView::from(
        tax::create_tax_region(tx, ctx, new).await?,
    ))
}

pub async fn get_tax_region(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: TaxRegionId,
) -> Result<TaxRegionView> {
    Ok(TaxRegionView::from(tax::tax_region(tx, ctx, id).await?))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateTaxRegion {
    pub country_code: Option<String>,
    pub province_code: Option<String>,
    pub provider: Option<String>,
}

pub async fn update_tax_region(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: TaxRegionId,
    body: UpdateTaxRegion,
) -> Result<TaxRegionView> {
    let patch = tax::TaxRegionPatch {
        country_code: body.country_code,
        province_code: body.province_code,
        provider: body.provider,
    };
    Ok(TaxRegionView::from(
        tax::update_tax_region(tx, ctx, id, patch).await?,
    ))
}

pub async fn delete_tax_region(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: TaxRegionId) -> Result<()> {
    tax::delete_tax_region(tx, ctx, id).await
}

pub async fn list_tax_rates(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListTaxRates,
) -> Result<Page<TaxRateView>> {
    let paging = List {
        after: query.after,
        limit: query.limit,
    }
    .paging()?;
    Ok(map(
        tax::tax_rates(tx, ctx, query.tax_region_id, paging).await?,
        TaxRateView::from,
    ))
}

pub async fn create_tax_rate(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreateTaxRate,
) -> Result<TaxRateView> {
    let new = tax::NewTaxRate {
        tax_region_id: body.tax_region_id,
        rate: body.rate,
        code: body.code,
        name: body.name,
        is_default: body.is_default,
        is_combinable: body.is_combinable,
    };
    Ok(TaxRateView::from(tax::create_tax_rate(tx, ctx, new).await?))
}

pub async fn get_tax_rate(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: TaxRateId) -> Result<TaxRateView> {
    Ok(TaxRateView::from(tax::tax_rate(tx, ctx, id).await?))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateTaxRate {
    pub rate: Option<Decimal>,
    pub code: Option<String>,
    pub name: Option<String>,
    pub is_default: Option<bool>,
    pub is_combinable: Option<bool>,
}

pub async fn update_tax_rate(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: TaxRateId,
    body: UpdateTaxRate,
) -> Result<TaxRateView> {
    let patch = tax::TaxRatePatch {
        rate: body.rate,
        code: body.code,
        name: body.name,
        is_default: body.is_default,
        is_combinable: body.is_combinable,
    };
    Ok(TaxRateView::from(
        tax::update_tax_rate(tx, ctx, id, patch).await?,
    ))
}

pub async fn delete_tax_rate(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: TaxRateId) -> Result<()> {
    tax::delete_tax_rate(tx, ctx, id).await
}

/// Capped rather than paged: rules on one rate are configuration rather than a
/// customer's data.
pub async fn list_tax_rate_rules(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    rate: TaxRateId,
) -> Result<Vec<TaxRateRuleView>> {
    Ok(tax::tax_rate_rules(tx, ctx, rate)
        .await?
        .into_iter()
        .map(TaxRateRuleView::from)
        .collect())
}

pub async fn create_tax_rate_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    rate: TaxRateId,
    body: CreateTaxRateRule,
) -> Result<TaxRateRuleView> {
    let new = tax::NewTaxRateRule {
        tax_rate_id: rate,
        reference: body.reference,
        reference_id: body.reference_id,
    };
    Ok(TaxRateRuleView::from(
        tax::create_tax_rate_rule(tx, ctx, new).await?,
    ))
}

pub async fn delete_tax_rate_rule(tx: &mut Tx<'_>, ctx: &Ctx<'_>, rule_id: Uuid) -> Result<()> {
    tax::delete_tax_rate_rule(tx, ctx, rule_id).await
}

// -------------------------------------------------- regions, channels, money

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct RegionView {
    pub id: RegionId,
    pub name: String,
    pub currency_code: String,
    pub is_tax_inclusive: bool,
    pub has_automatic_taxes: bool,
    pub payment_providers: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<store::Region> for RegionView {
    fn from(row: store::Region) -> Self {
        RegionView {
            id: row.id,
            name: row.name,
            currency_code: row.currency_code,
            is_tax_inclusive: row.is_tax_inclusive,
            has_automatic_taxes: row.has_automatic_taxes,
            payment_providers: row.payment_providers,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct SalesChannelView {
    pub id: SalesChannelId,
    pub name: String,
    pub description: Option<String>,
    pub is_disabled: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<store::SalesChannel> for SalesChannelView {
    fn from(row: store::SalesChannel) -> Self {
        SalesChannelView {
            id: row.id,
            name: row.name,
            description: row.description,
            is_disabled: row.is_disabled,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
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

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRegion {
    pub name: String,
    pub currency_code: String,
    #[serde(default)]
    pub is_tax_inclusive: bool,
    #[serde(default = "default_true")]
    pub has_automatic_taxes: bool,
    #[serde(default)]
    pub payment_providers: Vec<String>,
}

pub async fn list_regions(tx: &mut Tx<'_>, ctx: &Ctx<'_>, query: List) -> Result<Page<RegionView>> {
    Ok(map(
        store::regions(tx, ctx, query.paging()?).await?,
        RegionView::from,
    ))
}

pub async fn create_region(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreateRegion,
) -> Result<RegionView> {
    let new = store::NewRegion {
        name: body.name,
        currency_code: Currency::parse(&body.currency_code)?,
        is_tax_inclusive: body.is_tax_inclusive,
        has_automatic_taxes: body.has_automatic_taxes,
        payment_providers: body.payment_providers,
    };
    Ok(RegionView::from(store::create_region(tx, ctx, new).await?))
}

pub async fn get_region(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: RegionId) -> Result<RegionView> {
    Ok(RegionView::from(store::region(tx, ctx, id).await?))
}

#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateRegion {
    pub name: Option<String>,
    pub currency_code: Option<String>,
    pub is_tax_inclusive: Option<bool>,
    pub has_automatic_taxes: Option<bool>,
    pub payment_providers: Option<Vec<String>>,
}

pub async fn update_region(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: RegionId,
    body: UpdateRegion,
) -> Result<RegionView> {
    let patch = store::RegionPatch {
        name: body.name,
        currency_code: currency(body.currency_code)?,
        is_tax_inclusive: body.is_tax_inclusive,
        has_automatic_taxes: body.has_automatic_taxes,
        payment_providers: body.payment_providers,
    };
    Ok(RegionView::from(
        store::update_region(tx, ctx, id, patch).await?,
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionCountryView {
    pub id: Uuid,
    pub iso_2: String,
    pub iso_3: String,
    pub numeric_code: String,
    pub name: String,
    pub display_name: String,
    pub region_id: Option<RegionId>,
}

impl From<store::RegionCountry> for RegionCountryView {
    fn from(row: store::RegionCountry) -> Self {
        RegionCountryView {
            id: row.id,
            iso_2: row.iso_2,
            iso_3: row.iso_3,
            numeric_code: row.numeric_code,
            name: row.name,
            display_name: row.display_name,
            region_id: row.region_id,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddRegionCountry {
    pub iso_2: String,
    pub iso_3: String,
    pub numeric_code: String,
    pub name: String,
    pub display_name: Option<String>,
}

pub async fn list_region_countries(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: RegionId,
    query: List,
) -> Result<Page<RegionCountryView>> {
    Ok(map(
        store::region_countries(tx, ctx, id, query.paging()?).await?,
        RegionCountryView::from,
    ))
}

pub async fn add_region_country(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: RegionId,
    body: AddRegionCountry,
) -> Result<RegionCountryView> {
    Ok(RegionCountryView::from(
        store::add_region_country(
            tx,
            ctx,
            id,
            store::NewRegionCountry {
                iso_2: body.iso_2,
                iso_3: body.iso_3,
                numeric_code: body.numeric_code,
                name: body.name,
                display_name: body.display_name,
            },
        )
        .await?,
    ))
}

pub async fn remove_region_country(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    country_code: String,
) -> Result<()> {
    store::remove_region_country(tx, ctx, &country_code).await
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateSalesChannel {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub is_disabled: bool,
}

#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateSalesChannel {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_disabled: Option<bool>,
}

pub async fn list_sales_channels(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: List,
) -> Result<Page<SalesChannelView>> {
    Ok(map(
        store::sales_channels(tx, ctx, query.paging()?).await?,
        SalesChannelView::from,
    ))
}

pub async fn get_sales_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SalesChannelId,
) -> Result<SalesChannelView> {
    Ok(SalesChannelView::from(
        store::sales_channel(tx, ctx, id).await?,
    ))
}

pub async fn create_sales_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreateSalesChannel,
) -> Result<SalesChannelView> {
    let new = store::NewSalesChannel {
        name: body.name,
        description: body.description,
        is_disabled: body.is_disabled,
    };
    Ok(SalesChannelView::from(
        store::create_sales_channel(tx, ctx, new).await?,
    ))
}

pub async fn update_sales_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SalesChannelId,
    body: UpdateSalesChannel,
) -> Result<SalesChannelView> {
    let patch = store::SalesChannelPatch {
        name: body.name,
        description: body.description,
        is_disabled: body.is_disabled,
    };
    Ok(SalesChannelView::from(
        store::update_sales_channel(tx, ctx, id, patch).await?,
    ))
}

pub async fn delete_sales_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SalesChannelId,
) -> Result<()> {
    store::delete_sales_channel(tx, ctx, id).await
}

// -------------------------------------------------------- publishable keys

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct PublishableKeyView {
    pub id: PublishableKeyId,
    pub title: String,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<store::PublishableKey> for PublishableKeyView {
    fn from(row: store::PublishableKey) -> Self {
        PublishableKeyView {
            id: row.id,
            title: row.title,
            revoked_at: row.revoked_at,
            last_used_at: row.last_used_at,
            created_at: row.created_at,
        }
    }
}

/// The only response the token appears in. It is not stored and cannot be
/// fetched again.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct IssuedKeyView {
    #[serde(flatten)]
    pub key: PublishableKeyView,
    pub token: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePublishableKey {
    pub title: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkSalesChannel {
    pub sales_channel_id: SalesChannelId,
}

pub async fn list_publishable_keys(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: List,
) -> Result<Page<PublishableKeyView>> {
    Ok(map(
        store::publishable_keys(tx, ctx, query.paging()?).await?,
        PublishableKeyView::from,
    ))
}

pub async fn get_publishable_key(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PublishableKeyId,
) -> Result<PublishableKeyView> {
    Ok(PublishableKeyView::from(
        store::publishable_key(tx, ctx, id).await?,
    ))
}

pub async fn create_publishable_key(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreatePublishableKey,
) -> Result<IssuedKeyView> {
    let issued = store::create_publishable_key(tx, ctx, &body.title).await?;
    Ok(IssuedKeyView {
        key: PublishableKeyView::from(issued.key),
        token: issued.token,
    })
}

pub async fn revoke_publishable_key(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PublishableKeyId,
) -> Result<PublishableKeyView> {
    Ok(PublishableKeyView::from(
        store::revoke_publishable_key(tx, ctx, id).await?,
    ))
}

/// Bounded by how many channels one key is allowed to see.
pub async fn list_key_sales_channels(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PublishableKeyId,
) -> Result<Vec<SalesChannelView>> {
    Ok(store::channels_for_key(tx, ctx, id)
        .await?
        .into_iter()
        .map(SalesChannelView::from)
        .collect())
}

pub async fn link_key_sales_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PublishableKeyId,
    body: LinkSalesChannel,
) -> Result<()> {
    store::link_key_to_channel(tx, ctx, id, body.sales_channel_id).await
}

pub async fn unlink_key_sales_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PublishableKeyId,
    channel: SalesChannelId,
) -> Result<()> {
    store::unlink_key_from_channel(tx, ctx, id, channel).await
}

/// Bounded by how many currencies the shop trades in.
pub async fn list_currencies(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Vec<CurrencyView>> {
    Ok(store::currencies(tx, ctx)
        .await?
        .into_iter()
        .map(CurrencyView::from)
        .collect())
}

pub async fn get_currency(tx: &mut Tx<'_>, ctx: &Ctx<'_>, code: &str) -> Result<CurrencyView> {
    Ok(CurrencyView::from(
        store::currency(tx, ctx, Currency::parse(code)?).await?,
    ))
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCurrency {
    pub code: String,
    pub numeric_code: Option<String>,
    pub exponent: i16,
    pub symbol: String,
    pub symbol_native: String,
    pub name: String,
}

/// The only writer of a shop's `currency` table: nothing prices, opens a cart
/// or pays out in a currency until a shop has enabled it here.
pub async fn create_currency(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreateCurrency,
) -> Result<CurrencyView> {
    let new = store::NewCurrency {
        code: Currency::parse(&body.code)?,
        numeric_code: body.numeric_code,
        exponent: body.exponent,
        symbol: body.symbol,
        symbol_native: body.symbol_native,
        name: body.name,
    };
    Ok(CurrencyView::from(
        store::create_currency(tx, ctx, new).await?,
    ))
}

// ------------------------------------------------------------------- store

#[derive(Debug, Clone, Serialize)]
pub struct StoreView {
    pub id: StoreId,
    pub name: String,
    pub default_currency_code: String,
    pub supported_currency_codes: Vec<String>,
    pub supported_locales: Vec<String>,
    pub default_region_id: Option<RegionId>,
    pub default_sales_channel_id: Option<SalesChannelId>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<store::Store> for StoreView {
    fn from(row: store::Store) -> Self {
        StoreView {
            id: row.id,
            name: row.name,
            default_currency_code: row.default_currency_code,
            supported_currency_codes: row.supported_currency_codes,
            supported_locales: row.supported_locales,
            default_region_id: row.default_region_id,
            default_sales_channel_id: row.default_sales_channel_id,
            metadata: row.metadata,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateStore {
    pub name: String,
    pub default_currency_code: String,
    #[serde(default)]
    pub supported_currency_codes: Vec<String>,
    #[serde(default)]
    pub supported_locales: Vec<String>,
    pub default_region_id: Option<RegionId>,
    pub default_sales_channel_id: Option<SalesChannelId>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateStore {
    pub name: Option<String>,
    pub default_currency_code: Option<String>,
    pub supported_currency_codes: Option<Vec<String>>,
    pub supported_locales: Option<Vec<String>>,
    pub default_region_id: Option<RegionId>,
    pub default_sales_channel_id: Option<SalesChannelId>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetLocales {
    pub supported_locales: Vec<String>,
}

fn currency_list(codes: Option<Vec<String>>) -> Result<Option<Vec<Currency>>> {
    codes
        .map(|codes| codes.iter().map(|code| Currency::parse(code)).collect())
        .transpose()
}

pub async fn get_store(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<StoreView> {
    Ok(StoreView::from(store::store(tx, ctx).await?))
}

pub async fn create_store(tx: &mut Tx<'_>, ctx: &Ctx<'_>, body: CreateStore) -> Result<StoreView> {
    let new = store::NewStore {
        name: body.name,
        default_currency_code: Currency::parse(&body.default_currency_code)?,
        supported_currency_codes: body
            .supported_currency_codes
            .iter()
            .map(|code| Currency::parse(code))
            .collect::<Result<Vec<_>>>()?,
        supported_locales: body.supported_locales,
        default_region_id: body.default_region_id,
        default_sales_channel_id: body.default_sales_channel_id,
        metadata: body.metadata,
    };
    Ok(StoreView::from(store::create_store(tx, ctx, new).await?))
}

pub async fn update_store(tx: &mut Tx<'_>, ctx: &Ctx<'_>, body: UpdateStore) -> Result<StoreView> {
    let patch = store::StorePatch {
        name: body.name,
        default_currency_code: body
            .default_currency_code
            .as_deref()
            .map(Currency::parse)
            .transpose()?,
        supported_currency_codes: currency_list(body.supported_currency_codes)?,
        supported_locales: body.supported_locales,
        default_region_id: body.default_region_id,
        default_sales_channel_id: body.default_sales_channel_id,
        metadata: body.metadata,
    };
    Ok(StoreView::from(store::update_store(tx, ctx, patch).await?))
}

/// Bounded by how many languages one shop is served in.
pub async fn list_locales(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Vec<String>> {
    store::locales(tx, ctx).await
}

pub async fn set_locales(tx: &mut Tx<'_>, ctx: &Ctx<'_>, body: SetLocales) -> Result<Vec<String>> {
    let patch = store::StorePatch {
        supported_locales: Some(body.supported_locales),
        ..store::StorePatch::default()
    };
    Ok(store::update_store(tx, ctx, patch).await?.supported_locales)
}

// ---------------------------------------------------------- workflow runs

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct WorkflowRunView {
    pub id: WorkflowRunId,
    pub state: String,
    pub failure: Option<String>,
}

fn run_state(state: workflow::State) -> &'static str {
    match state {
        workflow::State::Running => "running",
        workflow::State::Compensating => "compensating",
        workflow::State::Waiting => "waiting",
        workflow::State::Done => "done",
        workflow::State::Reverted => "reverted",
        workflow::State::Failed => "failed",
    }
}

/// Takes a pool rather than a transaction: the runner opens one transaction per
/// step and reads a run outside anybody else's.
pub async fn get_workflow_run(
    pool: &PgPool,
    ctx: &Ctx<'_>,
    id: WorkflowRunId,
) -> Result<WorkflowRunView> {
    let run = workflow::get(pool, ctx, id).await?;
    Ok(WorkflowRunView {
        id: run.id,
        state: run_state(run.state).to_owned(),
        failure: run.failure,
    })
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct WorkflowRunSummaryView {
    pub id: WorkflowRunId,
    pub name: String,
    pub transaction_key: String,
    pub state: String,
    pub failure: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<workflow::RunSummary> for WorkflowRunSummaryView {
    fn from(row: workflow::RunSummary) -> Self {
        WorkflowRunSummaryView {
            id: row.id,
            name: row.name,
            transaction_key: row.transaction_key,
            state: run_state(row.state).to_owned(),
            failure: row.failure,
            created_at: row.created_at,
            finished_at: row.finished_at,
        }
    }
}

/// What a person debugging a stuck run needs: the step, its state, how many
/// times it was tried, and why it failed. Not `output` — that is what the
/// runner needs to resume a step, and for a checkout it is the cart, the
/// addresses and the payment context. See #123: a payload is not readable
/// through this view, on purpose, whatever it holds.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct WorkflowStepView {
    pub id: Uuid,
    pub name: String,
    pub ordering: i32,
    pub group_ordering: i32,
    pub state: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub failure: Option<String>,
    pub run_after: chrono::DateTime<chrono::Utc>,
    pub lease_until: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<workflow::StepSummary> for WorkflowStepView {
    fn from(row: workflow::StepSummary) -> Self {
        WorkflowStepView {
            id: row.id,
            name: row.name,
            ordering: row.ordering,
            group_ordering: row.group_ordering,
            state: row.state,
            attempts: row.attempts,
            max_attempts: row.max_attempts,
            failure: row.failure,
            run_after: row.run_after,
            lease_until: row.lease_until,
        }
    }
}

/// Same rule as [`WorkflowStepView`]: `state` is the run's full compensation
/// context and stays out. `step_name` and `failure` are what a dead letter is
/// for a person to see.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct WorkflowDeadLetterView {
    pub id: Uuid,
    pub run_id: WorkflowRunId,
    pub step_name: String,
    pub failure: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<workflow::DeadLetter> for WorkflowDeadLetterView {
    fn from(row: workflow::DeadLetter) -> Self {
        WorkflowDeadLetterView {
            id: row.id,
            run_id: row.run_id,
            step_name: row.step_name,
            failure: row.failure,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListWorkflowRuns {
    pub after: Option<String>,
    pub limit: Option<u32>,
    pub state: Option<String>,
}

fn parse_run_state(text: &str) -> Result<workflow::State> {
    Ok(match text {
        "running" => workflow::State::Running,
        "compensating" => workflow::State::Compensating,
        "waiting" => workflow::State::Waiting,
        "done" => workflow::State::Done,
        "reverted" => workflow::State::Reverted,
        "failed" => workflow::State::Failed,
        other => return Err(Error::invalid(format!("{other:?} is not a run state"))),
    })
}

pub async fn list_workflow_runs(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListWorkflowRuns,
) -> Result<Page<WorkflowRunSummaryView>> {
    let state = query.state.as_deref().map(parse_run_state).transpose()?;
    let paging = List {
        after: query.after,
        limit: query.limit,
    }
    .paging()?;

    Ok(map(
        workflow::runs(tx, ctx, paging, state).await?,
        WorkflowRunSummaryView::from,
    ))
}

/// Bounded by how many steps the workflow that ran was written with.
pub async fn list_workflow_run_steps(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: WorkflowRunId,
) -> Result<Vec<WorkflowStepView>> {
    Ok(workflow::steps(tx, ctx, id)
        .await?
        .into_iter()
        .map(WorkflowStepView::from)
        .collect())
}

/// Scope-wide, with no per-letter ownership check: a dead letter has no
/// single owner to ask about, and this is an operator surface rather than one
/// scoped to whoever's run failed. Gate it on the operator role at the host,
/// the way every other `/admin` route already is.
pub async fn list_workflow_dead_letters(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: List,
) -> Result<Page<WorkflowDeadLetterView>> {
    Ok(map(
        workflow::dead_letters(tx, ctx, query.paging()?).await?,
        WorkflowDeadLetterView::from,
    ))
}

pub(super) static ROUTES: &[Route] = &[
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/customers",
        action: Action::View,
        domain: "customer",
        summary: "List customers",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/customers",
        action: Action::Write,
        domain: "customer",
        summary: "Create a customer",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/customers/{id}",
        action: Action::View,
        domain: "customer",
        summary: "Fetch one customer",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/customers/{id}",
        action: Action::Write,
        domain: "customer",
        summary: "Edit a customer",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/customers/{id}",
        action: Action::Delete,
        domain: "customer",
        summary: "Delete a customer, keeping their orders",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/customers/{id}/export",
        action: Action::View,
        domain: "customer",
        summary: "Everything held about one customer, for a data request",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/customers/{id}/erase",
        action: Action::Delete,
        domain: "customer",
        summary: "Empty a customer's personal columns, keeping their orders",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/customers/{id}/addresses",
        action: Action::View,
        domain: "customer",
        summary: "List a customer's addresses",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/customers/{id}/addresses",
        action: Action::Write,
        domain: "customer",
        summary: "Add an address to a customer",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/customers/{id}/addresses/{address_id}",
        action: Action::Write,
        domain: "customer",
        summary: "Edit a customer's address",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/customers/{id}/addresses/{address_id}",
        action: Action::Delete,
        domain: "customer",
        summary: "Delete a customer's address",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/customer-groups",
        action: Action::View,
        domain: "customer",
        summary: "List customer groups",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/customer-groups",
        action: Action::Write,
        domain: "customer",
        summary: "Create a customer group",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/customer-groups/{id}",
        action: Action::Write,
        domain: "customer",
        summary: "Rename a customer group",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/customer-groups/{id}",
        action: Action::Delete,
        domain: "customer",
        summary: "Delete a customer group",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/customer-groups/{id}/customers",
        action: Action::View,
        domain: "customer",
        summary: "List a group's members",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/customer-groups/{id}/customers",
        action: Action::Write,
        domain: "customer",
        summary: "Put a customer in a group",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/customer-groups/{id}/customers/{customer_id}",
        action: Action::Write,
        domain: "customer",
        summary: "Take a customer out of a group",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/promotions",
        action: Action::View,
        domain: "promotion",
        summary: "List promotions",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/promotions",
        action: Action::Write,
        domain: "promotion",
        summary: "Create a promotion",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/promotions/{id}",
        action: Action::View,
        domain: "promotion",
        summary: "Fetch one promotion",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/promotions/{id}",
        action: Action::Delete,
        domain: "promotion",
        summary: "Withdraw a promotion, keeping the discounts it granted",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/promotions/{id}",
        action: Action::Write,
        domain: "promotion",
        summary: "Change a promotion's code, automation or usage limits",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/promotions/{id}/status",
        action: Action::Write,
        domain: "promotion",
        summary: "Set a promotion's status",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/promotions/{id}/application-method",
        action: Action::Write,
        domain: "promotion",
        summary: "Set how a promotion discounts",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/promotions/{id}/{rule_type}",
        action: Action::Write,
        domain: "promotion",
        summary: "Add an eligibility, target or buy rule to a promotion",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/promotions/{id}/{rule_type}",
        action: Action::View,
        domain: "promotion",
        summary: "List a promotion's eligibility, target or buy rules",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/promotions/{id}/{rule_type}/{rule_id}",
        action: Action::Delete,
        domain: "promotion",
        summary: "Take a rule off a promotion",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/campaigns",
        action: Action::View,
        domain: "promotion",
        summary: "List campaigns",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/campaigns",
        action: Action::Write,
        domain: "promotion",
        summary: "Create a campaign",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/campaigns/{id}",
        action: Action::View,
        domain: "promotion",
        summary: "Fetch one campaign",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/campaigns/{id}",
        action: Action::Write,
        domain: "promotion",
        summary: "Change a campaign's name, identifier or dates",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/campaigns/{id}/promotions",
        action: Action::Write,
        domain: "promotion",
        summary: "Put a promotion under a campaign",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/campaigns/{id}/promotions/{promotion_id}",
        action: Action::Write,
        domain: "promotion",
        summary: "Take a promotion off a campaign",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/campaigns/{id}/budget",
        action: Action::Write,
        domain: "promotion",
        summary: "Set a campaign's spend, usage or per-attribute budget",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/campaigns/{id}/budget/usage",
        action: Action::View,
        domain: "promotion",
        summary: "List how much of a per-attribute budget each attribute value has used",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/tax-regions",
        action: Action::View,
        domain: "tax",
        summary: "List tax regions",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/tax-regions",
        action: Action::Write,
        domain: "tax",
        summary: "Create a tax region",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/tax-regions/{id}",
        action: Action::View,
        domain: "tax",
        summary: "Fetch one tax region",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/tax-regions/{id}",
        action: Action::Write,
        domain: "tax",
        summary: "Change a tax region's country, province or provider",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/tax-regions/{id}",
        action: Action::Delete,
        domain: "tax",
        summary: "Delete a tax region",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/tax-rates",
        action: Action::View,
        domain: "tax",
        summary: "List tax rates, optionally within one region",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/tax-rates",
        action: Action::Write,
        domain: "tax",
        summary: "Create a tax rate",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/tax-rates/{id}",
        action: Action::View,
        domain: "tax",
        summary: "Fetch one tax rate",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/tax-rates/{id}",
        action: Action::Write,
        domain: "tax",
        summary: "Change a tax rate",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/tax-rates/{id}",
        action: Action::Delete,
        domain: "tax",
        summary: "Delete a tax rate",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/tax-rates/{id}/rules",
        action: Action::View,
        domain: "tax",
        summary: "List what a tax rate applies to",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/tax-rates/{id}/rules",
        action: Action::Write,
        domain: "tax",
        summary: "Make a tax rate apply to a product, type, collection or shipping option",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/tax-rates/{id}/rules/{rule_id}",
        action: Action::Delete,
        domain: "tax",
        summary: "Stop a tax rate applying to something",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/regions",
        action: Action::View,
        domain: "store",
        summary: "List regions",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/regions",
        action: Action::Write,
        domain: "store",
        summary: "Create a region",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/regions/{id}",
        action: Action::View,
        domain: "store",
        summary: "Fetch one region",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/regions/{id}",
        action: Action::Write,
        domain: "store",
        summary: "Change a region's name, currency or tax inclusiveness",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/regions/{id}/countries",
        action: Action::View,
        domain: "store",
        summary: "List the countries a region serves",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/regions/{id}/countries",
        action: Action::Write,
        domain: "store",
        summary: "Put a country under a region",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/regions/{id}/countries/{country_code}",
        action: Action::Delete,
        domain: "store",
        summary: "Take a country out of its region",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/sales-channels",
        action: Action::View,
        domain: "store",
        summary: "List sales channels",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/sales-channels",
        action: Action::Write,
        domain: "store",
        summary: "Create a sales channel",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/sales-channels/{id}",
        action: Action::View,
        domain: "store",
        summary: "Fetch one sales channel",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/sales-channels/{id}",
        action: Action::Write,
        domain: "store",
        summary: "Edit a sales channel",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/sales-channels/{id}",
        action: Action::Delete,
        domain: "store",
        summary: "Delete a sales channel",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/publishable-api-keys",
        action: Action::View,
        domain: "store",
        summary: "List the storefront keys, live and revoked",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/publishable-api-keys",
        action: Action::Write,
        domain: "store",
        summary: "Issue a storefront key, whose token is returned once",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/publishable-api-keys/{id}",
        action: Action::View,
        domain: "store",
        summary: "Fetch one storefront key",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/publishable-api-keys/{id}/revoke",
        action: Action::Write,
        domain: "store",
        summary: "Revoke a storefront key without forgetting it",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/publishable-api-keys/{id}/sales-channels",
        action: Action::View,
        domain: "store",
        summary: "The channels one storefront key may see",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/publishable-api-keys/{id}/sales-channels",
        action: Action::Write,
        domain: "store",
        summary: "Let a storefront key see a channel",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/publishable-api-keys/{id}/sales-channels/{channel_id}",
        action: Action::Delete,
        domain: "store",
        summary: "Stop a storefront key seeing a channel",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/currencies",
        action: Action::View,
        domain: "store",
        summary: "List the currencies the shop trades in",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/currencies",
        action: Action::Write,
        domain: "store",
        summary: "Enable a currency for the shop",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/currencies/{code}",
        action: Action::View,
        domain: "store",
        summary: "Fetch one currency and its exponent",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/stores",
        action: Action::View,
        domain: "store",
        summary: "The shop's own settings",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/stores",
        action: Action::Write,
        domain: "store",
        summary: "Create the shop's own settings",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/stores",
        action: Action::Write,
        domain: "store",
        summary: "Edit the shop's own settings",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/locales",
        action: Action::View,
        domain: "store",
        summary: "List the languages the shop is served in",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/locales",
        action: Action::Write,
        domain: "store",
        summary: "Set the languages the shop is served in",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/workflows-executions",
        action: Action::View,
        domain: "workflow",
        summary: "List workflow runs, optionally in one state",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/workflows-executions/{id}",
        action: Action::View,
        domain: "workflow",
        summary: "How a workflow run ended, and what it returned",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/workflows-executions/{id}/steps",
        action: Action::View,
        domain: "workflow",
        summary: "The steps of one workflow run, and how each ended",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/workflow-dead-letters",
        action: Action::View,
        domain: "workflow",
        summary: "Runs whose compensation failed, which nothing will retry",
    },
];

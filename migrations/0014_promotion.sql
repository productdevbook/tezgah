-- Promotion: what comes off a cart, who may have it, and how often.
--
-- Usage is a claim, not a count. A promotion carries its own counter and the
-- constraints below refuse the increment that would pass the limit, so the
-- claim is taken at checkout by an update that either moves the counter or
-- affects no row — and two checkouts racing cannot both win.

set lock_timeout = '3s';
set statement_timeout = '60s';

create table campaign (
    id          uuid primary key,
    identifier  text not null,
    name        text not null,
    description text,
    starts_at   timestamptz,
    ends_at     timestamptz,
    constraint campaign_window_valid
        check (starts_at is null or ends_at is null or starts_at < ends_at)
);
call tezgah_register('campaign');

create unique index campaign_identifier_key
    on campaign (scope, identifier);
create index campaign_window_idx
    on campaign (scope, starts_at, ends_at);

-- `used <= "limit"` is what makes spending atomic: the decrement is an update
-- adding to `used`, and the row is refused rather than overdrawn.
create table campaign_budget (
    id          uuid primary key,
    campaign_id uuid not null references campaign (id) on delete cascade,
    type        text not null,
    "limit"     numeric(20, 6),
    used        numeric(20, 6) not null default 0,
    currency    char(3),
    constraint campaign_budget_type_valid
        check (type in ('spend', 'usage')),
    constraint campaign_budget_used_valid
        check (used >= 0),
    constraint campaign_budget_limit_valid
        check ("limit" is null or "limit" >= 0),
    constraint campaign_budget_within_limit_valid
        check ("limit" is null or used <= "limit"),
    constraint campaign_budget_currency_valid
        check (type <> 'spend' or currency is not null)
);
call tezgah_register('campaign_budget');

create unique index campaign_budget_campaign_key
    on campaign_budget (scope, campaign_id);

create table promotion (
    id           uuid primary key,
    campaign_id  uuid references campaign (id) on delete set null,
    code         text not null,
    type         text not null default 'standard',
    status       text not null default 'draft',
    is_automatic boolean not null default false,
    -- Total claims allowed across every customer; null is unlimited.
    usage_limit  integer,
    used         integer not null default 0,
    -- Claims allowed to one customer; null is unlimited.
    customer_usage_limit integer,
    deleted_at   timestamptz,
    constraint promotion_type_valid
        check (type in ('standard', 'buyget')),
    constraint promotion_status_valid
        check (status in ('draft', 'active', 'inactive')),
    constraint promotion_used_valid
        check (used >= 0),
    constraint promotion_usage_limit_valid
        check (usage_limit is null or usage_limit > 0),
    constraint promotion_customer_usage_limit_valid
        check (customer_usage_limit is null or customer_usage_limit > 0),
    constraint promotion_within_usage_limit_valid
        check (usage_limit is null or used <= usage_limit)
);
call tezgah_register('promotion');

create unique index promotion_code_key
    on promotion (scope, code)
    where deleted_at is null;
create index promotion_campaign_idx
    on promotion (scope, campaign_id);
-- The automatic ones a cart collects without anybody typing a code.
create index promotion_automatic_idx
    on promotion (scope, type)
    where deleted_at is null and is_automatic and status = 'active';

create table application_method (
    id                     uuid primary key,
    promotion_id           uuid not null references promotion (id) on delete cascade,
    type                   text not null,
    target_type            text not null,
    allocation             text,
    value                  numeric(20, 6),
    currency               char(3),
    max_quantity           integer,
    apply_to_quantity      integer,
    buy_rules_min_quantity integer,
    constraint application_method_type_valid
        check (type in ('fixed', 'percentage')),
    constraint application_method_target_type_valid
        check (target_type in ('order', 'items', 'shipping_methods')),
    constraint application_method_allocation_valid
        check (allocation is null or allocation in ('each', 'across')),
    constraint application_method_value_valid
        check (value is null or value >= 0),
    constraint application_method_percentage_valid
        check (type <> 'percentage' or (value is not null and value <= 100)),
    -- A fixed amount is money, and money is never one column without the other.
    constraint application_method_currency_valid
        check (type <> 'fixed' or currency is not null),
    constraint application_method_max_quantity_valid
        check (max_quantity is null or max_quantity > 0),
    constraint application_method_apply_to_quantity_valid
        check (apply_to_quantity is null or apply_to_quantity > 0),
    constraint application_method_buy_rules_min_quantity_valid
        check (buy_rules_min_quantity is null or buy_rules_min_quantity > 0)
);
call tezgah_register('application_method');

create unique index application_method_promotion_key
    on application_method (scope, promotion_id);

-- Who the promotion is for: the conditions the cart itself must satisfy.
create table promotion_rule (
    id             uuid primary key,
    promotion_id   uuid not null references promotion (id) on delete cascade,
    attribute      text not null,
    operator       text not null default 'eq',
    allowed_values text[] not null,
    description    text,
    constraint promotion_rule_operator_valid
        check (operator in ('eq', 'ne', 'in', 'gt', 'lt', 'gte', 'lte')),
    constraint promotion_rule_allowed_values_valid
        check (cardinality(allowed_values) > 0)
);
call tezgah_register('promotion_rule');

create index promotion_rule_promotion_idx
    on promotion_rule (scope, promotion_id);
create index promotion_rule_attribute_idx
    on promotion_rule (scope, attribute, operator);
create index promotion_rule_allowed_values_idx
    on promotion_rule using gin (allowed_values);
create unique index promotion_rule_promotion_attribute_key
    on promotion_rule (scope, promotion_id, attribute, operator);

-- Which lines the discount lands on.
create table promotion_target_rule (
    id                    uuid primary key,
    application_method_id uuid not null
        references application_method (id) on delete cascade,
    attribute             text not null,
    operator              text not null default 'eq',
    allowed_values        text[] not null,
    description           text,
    constraint promotion_target_rule_operator_valid
        check (operator in ('eq', 'ne', 'in', 'gt', 'lt', 'gte', 'lte')),
    constraint promotion_target_rule_allowed_values_valid
        check (cardinality(allowed_values) > 0)
);
call tezgah_register('promotion_target_rule');

create index promotion_target_rule_method_idx
    on promotion_target_rule (scope, application_method_id);
create index promotion_target_rule_attribute_idx
    on promotion_target_rule (scope, attribute, operator);
create index promotion_target_rule_allowed_values_idx
    on promotion_target_rule using gin (allowed_values);
create unique index promotion_target_rule_method_attribute_key
    on promotion_target_rule (scope, application_method_id, attribute, operator);

-- Which lines have to be in the cart before the target lines are discounted.
create table promotion_buy_rule (
    id                    uuid primary key,
    application_method_id uuid not null
        references application_method (id) on delete cascade,
    attribute             text not null,
    operator              text not null default 'eq',
    allowed_values        text[] not null,
    description           text,
    constraint promotion_buy_rule_operator_valid
        check (operator in ('eq', 'ne', 'in', 'gt', 'lt', 'gte', 'lte')),
    constraint promotion_buy_rule_allowed_values_valid
        check (cardinality(allowed_values) > 0)
);
call tezgah_register('promotion_buy_rule');

create index promotion_buy_rule_method_idx
    on promotion_buy_rule (scope, application_method_id);
create index promotion_buy_rule_attribute_idx
    on promotion_buy_rule (scope, attribute, operator);
create index promotion_buy_rule_allowed_values_idx
    on promotion_buy_rule using gin (allowed_values);
create unique index promotion_buy_rule_method_attribute_key
    on promotion_buy_rule (scope, application_method_id, attribute, operator);

-- One row per customer per promotion, so "once per account" is the database's
-- answer and not a query somebody remembered to run first. `used <= "limit"`
-- makes the claim an update that either moves or fails.
create table promotion_usage (
    id           uuid primary key,
    promotion_id uuid not null references promotion (id) on delete cascade,
    customer_id  uuid not null,
    used         integer not null default 0,
    "limit"      integer,
    claimed_at   timestamptz not null default now(),
    constraint promotion_usage_used_valid
        check (used >= 0),
    constraint promotion_usage_limit_valid
        check ("limit" is null or "limit" > 0),
    constraint promotion_usage_within_limit_valid
        check ("limit" is null or used <= "limit")
);
call tezgah_register('promotion_usage');

create unique index promotion_usage_promotion_customer_key
    on promotion_usage (scope, promotion_id, customer_id);
create index promotion_usage_customer_idx
    on promotion_usage (scope, customer_id, promotion_id);

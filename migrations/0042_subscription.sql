set lock_timeout = '3s';
set statement_timeout = '120s';

-- Recurrence belongs to the catalogue, and the contract it produces is not an
-- order. A `selling_plan` says how often a variant is billed and how often it
-- is delivered — two policies, not one — and a `subscription` is the long-lived
-- thing checkout or the back office materialises from it. The orders come
-- afterwards, one per cycle, and `subscription_order` is what makes "how much
-- has this subscriber paid" answerable.

create table selling_plan_group (
    id           uuid primary key,
    name         text not null,
    description  text,
    deleted_at   timestamptz,
    metadata     jsonb
);
call tezgah_register('selling_plan_group');

create unique index selling_plan_group_name_key
    on selling_plan_group (scope, name) where deleted_at is null;

-- The billing policy and the delivery policy are separate columns because they
-- are separate facts: prepay six months, deliver one box a month.
create table selling_plan (
    id                      uuid primary key,
    selling_plan_group_id   uuid not null,
    name                    text not null,
    billing_interval_unit   text not null,
    billing_interval_count  integer not null,
    delivery_interval_unit  text,
    delivery_interval_count integer,
    prepaid_cycles          integer,
    min_cycles              integer,
    max_cycles              integer,
    discount_kind           text,
    discount_value          numeric(19, 4),
    currency_code           text,
    applies_to              text not null default 'renewals',
    dunning_max_attempts    integer not null default 3,
    dunning_interval_hours  integer not null default 72,
    position                integer not null default 0,
    deleted_at              timestamptz,
    metadata                jsonb,
    -- One column per constraint where a number and a word are both involved:
    -- the isolation seeder reads a check to learn what a column accepts, and a
    -- constraint naming both teaches it to write 'day' into an integer.
    constraint selling_plan_billing_unit_valid
        check (billing_interval_unit in ('day', 'week', 'month', 'year')),
    constraint selling_plan_billing_count_valid check (billing_interval_count > 0),
    constraint selling_plan_delivery_unit_valid
        check (delivery_interval_unit is null
               or delivery_interval_unit in ('day', 'week', 'month', 'year')),
    constraint selling_plan_delivery_count_valid
        check (delivery_interval_count is null or delivery_interval_count > 0),
    constraint selling_plan_cycles_valid
        check ((prepaid_cycles is null or prepaid_cycles > 0)
               and (min_cycles is null or min_cycles > 0)
               and (max_cycles is null or max_cycles > 0)),
    -- A percentage is a number of hundredths and needs no currency; a fixed
    -- amount is money and cannot be read without one.
    constraint selling_plan_discount_valid
        check ((discount_kind is null and discount_value is null)
               or (discount_kind = 'percentage' and discount_value >= 0 and discount_value <= 100
                   and currency_code is null)
               or (discount_kind = 'fixed' and discount_value >= 0
                   and currency_code is not null)),
    constraint selling_plan_applies_to_valid
        check (applies_to in ('first_order', 'renewals', 'every_order')),
    constraint selling_plan_dunning_valid
        check (dunning_max_attempts > 0 and dunning_interval_hours > 0)
);
call tezgah_register('selling_plan');

create index selling_plan_group_idx
    on selling_plan (scope, selling_plan_group_id, position) where deleted_at is null;

call tezgah_fk('selling_plan', 'selling_plan_group_id', 'selling_plan_group', 'cascade', true);

create table selling_plan_variant (
    id              uuid primary key,
    selling_plan_id uuid not null,
    variant_id      uuid not null
);
call tezgah_register('selling_plan_variant');

create unique index selling_plan_variant_key
    on selling_plan_variant (scope, selling_plan_id, variant_id);
create index selling_plan_variant_variant_idx
    on selling_plan_variant (scope, variant_id);

call tezgah_fk('selling_plan_variant', 'selling_plan_id', 'selling_plan', 'cascade', true);
call tezgah_fk('selling_plan_variant', 'variant_id', 'product_variant', 'cascade', true);

-- The contract. `account_holder_id` and `payment_method_reference` are the two
-- opaque strings the provider issued: tezgah holds the permission to charge an
-- instrument, never the instrument. `paused_until` is carried here and written
-- by nothing yet — pause, resume, skip and swap are their own stage.
create table subscription (
    id                       uuid primary key,
    customer_id              uuid not null,
    selling_plan_id          uuid not null,
    currency_code            text not null,
    region_id                uuid,
    sales_channel_id         uuid,
    status                   text not null default 'active',
    account_holder_id        uuid,
    payment_method_reference text,
    mandate_reference        text,
    mandate_accepted_at      timestamptz,
    shipping_address_id      uuid,
    billing_address_id       uuid,
    next_billing_at          timestamptz not null,
    current_period_start     timestamptz not null,
    current_period_end       timestamptz not null,
    cycle                    integer not null default 0,
    line_version             integer not null default 1,
    cancel_at_period_end     boolean not null default false,
    paused_until             timestamptz,
    ended_at                 timestamptz,
    dunning_attempts         integer not null default 0,
    metadata                 jsonb,
    constraint subscription_status_valid
        check (status in ('active', 'past_due', 'cancelled', 'expired')),
    constraint subscription_period_valid
        check (current_period_end >= current_period_start),
    constraint subscription_counters_valid
        check (cycle >= 0 and line_version >= 1 and dunning_attempts >= 0),
    constraint subscription_ended_valid
        check (ended_at is null or status in ('cancelled', 'expired'))
);
call tezgah_register('subscription');

create index subscription_customer_idx
    on subscription (scope, customer_id, created_at, id);
create index subscription_selling_plan_idx on subscription (scope, selling_plan_id);
create index subscription_account_holder_idx
    on subscription (scope, account_holder_id) where account_holder_id is not null;
create index subscription_region_idx on subscription (scope, region_id)
    where region_id is not null;
create index subscription_sales_channel_idx on subscription (scope, sales_channel_id)
    where sales_channel_id is not null;
create index subscription_shipping_address_idx
    on subscription (scope, shipping_address_id) where shipping_address_id is not null;
create index subscription_billing_address_idx
    on subscription (scope, billing_address_id) where billing_address_id is not null;

-- What the host's clock polls: the active contracts owed a renewal, oldest
-- first, without reading the ones that are not.
create index subscription_due_idx
    on subscription (scope, next_billing_at, id)
    where status in ('active', 'past_due');

call tezgah_fk('subscription', 'customer_id', 'customer', 'restrict', true);
call tezgah_fk('subscription', 'selling_plan_id', 'selling_plan', 'restrict', true);
call tezgah_fk('subscription', 'account_holder_id', 'account_holder', 'restrict', true);
call tezgah_fk('subscription', 'region_id', 'region', 'set null', true);
call tezgah_fk('subscription', 'sales_channel_id', 'sales_channel', 'set null', true);
call tezgah_fk('subscription', 'shipping_address_id', 'customer_address', 'set null', true);
call tezgah_fk('subscription', 'billing_address_id', 'customer_address', 'set null', true);

-- Versioned the way `order_item` is: a mid-contract price change writes a new
-- version and an event, and nothing is edited in place.
create table subscription_line (
    id              uuid primary key,
    subscription_id uuid not null,
    version         integer not null,
    variant_id      uuid not null,
    title           text,
    quantity        integer not null,
    unit_price      numeric(19, 4) not null,
    currency_code   text not null,
    constraint subscription_line_quantity_valid check (quantity > 0),
    constraint subscription_line_price_valid check (unit_price >= 0),
    constraint subscription_line_version_valid check (version >= 1)
);
call tezgah_register('subscription_line');

create unique index subscription_line_key
    on subscription_line (scope, subscription_id, version, variant_id);
create index subscription_line_variant_idx on subscription_line (scope, variant_id);

call tezgah_fk('subscription_line', 'subscription_id', 'subscription', 'cascade', true);
call tezgah_fk('subscription_line', 'variant_id', 'product_variant', 'restrict', true);

-- Evidence, and the reason the same cycle cannot be billed twice however many
-- schedulers fire at once.
create table subscription_order (
    id              uuid primary key,
    subscription_id uuid not null,
    order_id        uuid not null,
    cycle           integer not null,
    period_start    timestamptz not null,
    period_end      timestamptz not null,
    kind            text not null default 'renewal',
    constraint subscription_order_cycle_valid check (cycle >= 0),
    constraint subscription_order_period_valid check (period_end >= period_start),
    constraint subscription_order_kind_valid check (kind in ('renewal'))
);
call tezgah_register('subscription_order');

create unique index subscription_order_cycle_key
    on subscription_order (scope, subscription_id, cycle);
create index subscription_order_order_idx on subscription_order (scope, order_id);

call tezgah_fk('subscription_order', 'subscription_id', 'subscription', 'restrict', true);
call tezgah_fk('subscription_order', 'order_id', 'order', 'restrict', true);

-- Why it stopped. Append-only, and evidence: a support conversation is
-- answered from here.
create table subscription_event (
    id              uuid primary key,
    subscription_id uuid not null,
    kind            text not null,
    payload         jsonb,
    at              timestamptz not null default now(),
    constraint subscription_event_kind_valid
        check (kind in ('created', 'renewed', 'price_changed', 'payment_failed',
                        'dunning_exhausted', 'cancelled', 'expired'))
);
call tezgah_register('subscription_event');

create index subscription_event_subscription_idx
    on subscription_event (scope, subscription_id, at, id);

call tezgah_fk('subscription_event', 'subscription_id', 'subscription', 'restrict', true);

insert into tezgah_scoped_fk_table (name) values
    ('selling_plan'),
    ('selling_plan_variant'),
    ('subscription'),
    ('subscription_line'),
    ('subscription_order'),
    ('subscription_event')
on conflict do nothing;

insert into tezgah_evidence_table (name) values
    ('subscription_order'),
    ('subscription_event')
on conflict do nothing;

-- Which plan a line was bought under, so a cart and the order it becomes carry
-- the recurrence rather than the contract having to be guessed back out of
-- them. Expand-only, which is what an append-only migration may do.
alter table cart_line_item add column if not exists selling_plan_id uuid;
alter table order_line_item add column if not exists selling_plan_id uuid;

create index if not exists cart_line_item_selling_plan_idx
    on cart_line_item (scope, selling_plan_id) where selling_plan_id is not null;
create index if not exists order_line_item_selling_plan_idx
    on order_line_item (scope, selling_plan_id) where selling_plan_id is not null;

call tezgah_fk('cart_line_item', 'selling_plan_id', 'selling_plan', 'set null', true);
call tezgah_fk('order_line_item', 'selling_plan_id', 'selling_plan', 'set null', true);

-- Pricing: what a thing costs, to whom, in what currency, at what quantity.
--
-- A price set is the priceable handle other domains point at — a variant, a
-- shipping option. Resolution runs entirely off the indexes below: an exact
-- match first, then the candidate satisfying the most rules, then priority.

set lock_timeout = '3s';
set statement_timeout = '60s';

create table price_set (
    id          uuid primary key
);
call tezgah_register('price_set');

create table price_list (
    id          uuid primary key,
    title       text not null,
    description text,
    type        text not null default 'sale',
    status      text not null default 'draft',
    starts_at   timestamptz,
    ends_at     timestamptz,
    rules_count integer not null default 0,
    deleted_at  timestamptz,
    constraint price_list_type_valid
        check (type in ('sale', 'override')),
    constraint price_list_status_valid
        check (status in ('draft', 'active', 'expired')),
    constraint price_list_rules_count_valid
        check (rules_count >= 0),
    constraint price_list_window_valid
        check (starts_at is null or ends_at is null or starts_at < ends_at)
);
call tezgah_register('price_list');

-- The window scan a cart does: which lists are live right now.
create index price_list_live_idx
    on price_list (scope, starts_at, ends_at)
    where deleted_at is null and status = 'active';

create table price_list_rule (
    id            uuid primary key,
    price_list_id uuid not null references price_list (id) on delete cascade,
    attribute     text not null,
    allowed_values text[] not null,
    constraint price_list_rule_allowed_values_valid
        check (cardinality(allowed_values) > 0)
);
call tezgah_register('price_list_rule');

create index price_list_rule_list_idx
    on price_list_rule (scope, price_list_id);
create index price_list_rule_attribute_idx
    on price_list_rule (scope, attribute);
create index price_list_rule_allowed_values_idx
    on price_list_rule using gin (allowed_values);
create unique index price_list_rule_list_attribute_key
    on price_list_rule (scope, price_list_id, attribute);

-- `rules_count` is denormalised on purpose: resolution ranks candidates by how
-- many rules each carries, and counting price_rule rows per candidate at read
-- time is the query that does not scale.
create table price (
    id            uuid primary key,
    price_set_id  uuid not null references price_set (id) on delete cascade,
    price_list_id uuid references price_list (id) on delete cascade,
    title         text,
    amount        numeric(20, 6) not null,
    currency      char(3) not null,
    min_quantity  integer,
    max_quantity  integer,
    rules_count   integer not null default 0,
    deleted_at    timestamptz,
    constraint price_amount_valid
        check (amount >= 0),
    constraint price_min_quantity_valid
        check (min_quantity is null or min_quantity > 0),
    constraint price_max_quantity_valid
        check (max_quantity is null or max_quantity > 0),
    constraint price_quantity_band_valid
        check (min_quantity is null or max_quantity is null
               or min_quantity <= max_quantity),
    constraint price_rules_count_valid
        check (rules_count >= 0)
);
call tezgah_register('price');

-- Candidate gathering: one set, one currency, best-first by rule count, then
-- the quantity band. Descending so the most specific row is read first.
create index price_resolution_idx
    on price (scope, price_set_id, currency, rules_count desc, min_quantity)
    where deleted_at is null;

-- The floor underneath every resolution: the ruleless, listless default.
create index price_default_idx
    on price (scope, price_set_id, currency, min_quantity)
    where deleted_at is null and price_list_id is null and rules_count = 0;

create index price_list_idx
    on price (scope, price_list_id)
    where deleted_at is null;

create index price_currency_idx
    on price (scope, currency)
    where deleted_at is null;

create table price_rule (
    id         uuid primary key,
    price_id   uuid not null references price (id) on delete cascade,
    attribute  text not null,
    value      text not null,
    operator   text not null default 'eq',
    priority   integer not null default 0,
    constraint price_rule_operator_valid
        check (operator in ('eq', 'in', 'gt', 'lt', 'gte', 'lte'))
);
call tezgah_register('price_rule');

-- An exact match is one lookup: hand the context's (attribute, value) pairs in
-- and get the prices they satisfy, without touching a price row first.
create index price_rule_match_idx
    on price_rule (scope, attribute, value, price_id)
    where operator = 'eq';

-- Everything else is a comparison, and those are scanned per attribute.
create index price_rule_attribute_idx
    on price_rule (scope, attribute, operator, price_id);

create index price_rule_price_idx
    on price_rule (scope, price_id, priority desc);

-- One rule per attribute and operator on a price: two rows on 'region_id eq'
-- would make "satisfies all its rules" unanswerable.
create unique index price_rule_price_attribute_key
    on price_rule (scope, price_id, attribute, operator);

-- Whether an amount already includes tax, decided per attribute and value —
-- 'currency' + 'eur', 'region_id' + a region.
create table price_preference (
    id               uuid primary key,
    attribute        text not null,
    value            text,
    is_tax_inclusive boolean not null default false
);
call tezgah_register('price_preference');

create unique index price_preference_attribute_value_key
    on price_preference (scope, attribute, coalesce(value, ''));

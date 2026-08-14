set lock_timeout = '3s';
set statement_timeout = '60s';

-- A country sits at the top and a province hangs beneath it. One level, not a
-- tree of any depth: no tax authority nests deeper than that, and a deeper
-- one would make "which rate applies" ambiguous.
create table tax_region (
    id              uuid primary key,
    country_code    char(2) not null,
    province_code   text,
    parent_id       uuid references tax_region (id) on delete cascade,
    provider        text,
    metadata        jsonb,
    constraint tax_region_country_code_valid
        check (country_code = upper(country_code) and country_code ~ '^[A-Z]{2}$'),
    constraint tax_region_parent_valid
        check (parent_id is null or province_code is not null),
    constraint tax_region_provider_valid
        check (parent_id is null or provider is null)
);
call tezgah_register('tax_region');

create unique index tax_region_country_province_key
    on tax_region (scope, country_code, province_code)
    where province_code is not null;
create unique index tax_region_country_key
    on tax_region (scope, country_code)
    where province_code is null;
create index tax_region_parent_id_idx on tax_region (scope, parent_id);

-- The rate is a percentage, not money: 18 means eighteen percent. Five decimal
-- places because some jurisdictions legislate rates like 8.87500.
create table tax_rate (
    id              uuid primary key,
    tax_region_id   uuid not null references tax_region (id) on delete cascade,
    rate            numeric(8, 5) not null,
    code            text,
    name            text not null,
    is_default      boolean not null default false,
    is_combinable   boolean not null default false,
    metadata        jsonb,
    constraint tax_rate_rate_valid check (rate >= 0 and rate <= 100)
);
call tezgah_register('tax_rate');

create index tax_rate_tax_region_id_idx on tax_rate (scope, tax_region_id);
create unique index tax_rate_default_key
    on tax_rate (scope, tax_region_id)
    where is_default;
create unique index tax_rate_code_key
    on tax_rate (scope, tax_region_id, code)
    where code is not null;

-- A rule narrows a rate to the things it applies to. `reference` names the
-- kind of thing and `reference_id` the row; there is no foreign key because
-- the target lives in whichever domain the reference names.
create table tax_rate_rule (
    id              uuid primary key,
    tax_rate_id     uuid not null references tax_rate (id) on delete cascade,
    reference       text not null,
    reference_id    uuid not null,
    metadata        jsonb,
    constraint tax_rate_rule_reference_valid check (
        reference in ('product', 'product_type', 'product_collection', 'shipping_option')
    )
);
call tezgah_register('tax_rate_rule');

create index tax_rate_rule_tax_rate_id_idx on tax_rate_rule (scope, tax_rate_id);
create index tax_rate_rule_reference_idx on tax_rate_rule (scope, reference, reference_id);
create unique index tax_rate_rule_target_key
    on tax_rate_rule (scope, tax_rate_id, reference, reference_id);

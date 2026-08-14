set lock_timeout = '3s';
set statement_timeout = '60s';

-- The exponent is what rounding is done at, not a display hint: JPY 0, TRY 2,
-- KWD 3, and money that ignores it is money that stops adding up.
create table currency (
    id              uuid primary key,
    code            char(3) not null,
    numeric_code    char(3),
    exponent        smallint not null,
    symbol          text not null,
    symbol_native   text not null,
    name            text not null,
    constraint currency_code_valid check (code = upper(code) and code ~ '^[A-Z]{3}$'),
    constraint currency_exponent_valid check (exponent between 0 and 4)
);
call tezgah_register('currency');

create unique index currency_code_key on currency (scope, code);

create table region (
    id                  uuid primary key,
    name                text not null,
    currency_code       char(3) not null,
    is_tax_inclusive    boolean not null default false,
    has_automatic_taxes boolean not null default true,
    payment_providers   text[] not null default '{}',
    metadata            jsonb,
    constraint region_currency_code_valid check (currency_code = upper(currency_code)),
    constraint region_payment_providers_valid check (array_position(payment_providers, null) is null)
);
call tezgah_register('region');

create unique index region_name_key on region (scope, name);
create index region_currency_code_idx on region (scope, currency_code);

-- ISO 3166-1 is a fixed list, so a country belongs to at most one region and
-- losing the region leaves the country behind rather than deleting it.
create table region_country (
    id              uuid primary key,
    iso_2           char(2) not null,
    iso_3           char(3) not null,
    numeric_code    char(3) not null,
    name            text not null,
    display_name    text not null,
    region_id       uuid references region (id) on delete set null,
    metadata        jsonb,
    constraint region_country_iso_2_valid check (iso_2 = upper(iso_2) and iso_2 ~ '^[A-Z]{2}$'),
    constraint region_country_iso_3_valid check (iso_3 = upper(iso_3) and iso_3 ~ '^[A-Z]{3}$')
);
call tezgah_register('region_country');

create unique index region_country_iso_2_key on region_country (scope, iso_2);
create index region_country_region_id_idx on region_country (scope, region_id);

create table sales_channel (
    id          uuid primary key,
    name        text not null,
    description text,
    is_disabled boolean not null default false,
    metadata    jsonb
);
call tezgah_register('sales_channel');

create unique index sales_channel_name_key on sales_channel (scope, name);

-- One store per scope: a second one would be a second shop, and a second shop
-- is a second scope.
create table store (
    id                          uuid primary key,
    name                        text not null,
    default_currency_code       char(3) not null,
    supported_currency_codes    char(3)[] not null default '{}',
    supported_locales           text[] not null default '{}',
    default_region_id           uuid references region (id) on delete set null,
    default_sales_channel_id    uuid references sales_channel (id) on delete set null,
    metadata                    jsonb,
    constraint store_default_currency_code_valid
        check (default_currency_code = upper(default_currency_code)),
    constraint store_supported_currency_codes_valid
        check (default_currency_code = any (supported_currency_codes))
);
call tezgah_register('store');

create unique index store_scope_key on store (scope);
create index store_default_region_id_idx on store (scope, default_region_id);
create index store_default_sales_channel_id_idx on store (scope, default_sales_channel_id);

-- What a storefront sends instead of an admin credential. It carries no
-- permission of its own; it says which channels the request may see.
create table publishable_key (
    id              uuid primary key,
    title           text not null,
    token           text not null,
    revoked_at      timestamptz,
    last_used_at    timestamptz
);
call tezgah_register('publishable_key');

create unique index publishable_key_token_key on publishable_key (scope, token);
create index publishable_key_live_idx on publishable_key (scope, id) where revoked_at is null;

create table publishable_key_sales_channel (
    id                  uuid primary key,
    publishable_key_id  uuid not null references publishable_key (id) on delete cascade,
    sales_channel_id    uuid not null references sales_channel (id) on delete cascade
);
call tezgah_register('publishable_key_sales_channel');

create unique index publishable_key_sales_channel_key
    on publishable_key_sales_channel (scope, publishable_key_id, sales_channel_id);
create index publishable_key_sales_channel_sales_channel_id_idx
    on publishable_key_sales_channel (scope, sales_channel_id);

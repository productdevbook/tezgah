set lock_timeout = '3s';
set statement_timeout = '60s';

-- A customer with an account and a customer who only ever checked out as a
-- guest are the same shape, and `has_account` is the whole difference.
-- `email` is nullable because a guest record can exist before an address or a
-- payment has produced one, and it is unique only among accounts: two guests
-- ordering from one household e-mail are two purchases, not a collision.
create table customer (
    id              uuid primary key,
    email           text,
    first_name      text,
    last_name       text,
    phone           text,
    company_name    text,
    has_account     boolean not null default false,
    metadata        jsonb,
    -- Erasure keeps the row so orders still point somewhere; `anonymised_at`
    -- says the personal columns were emptied rather than merely hidden.
    anonymised_at   timestamptz,
    deleted_at      timestamptz,
    constraint customer_email_valid check (email is null or email = lower(email)),
    constraint customer_account_needs_email
        check (not has_account or email is not null or anonymised_at is not null),
    constraint customer_anonymised_valid
        check (anonymised_at is null or (email is null and phone is null))
);
call tezgah_register('customer');

create unique index customer_account_email_key
    on customer (scope, email)
    where has_account and deleted_at is null;
create index customer_email_idx
    on customer (scope, email)
    where deleted_at is null;
create index customer_live_idx
    on customer (scope, id)
    where deleted_at is null;

create table customer_address (
    id                  uuid primary key,
    customer_id         uuid not null references customer (id) on delete cascade,
    label               text,
    is_default_shipping boolean not null default false,
    is_default_billing  boolean not null default false,
    company             text,
    first_name          text,
    last_name           text,
    address_1           text,
    address_2           text,
    city                text,
    province            text,
    postal_code         text,
    country_code        char(2),
    phone               text,
    metadata            jsonb,
    deleted_at          timestamptz,
    constraint customer_address_country_code_valid
        check (country_code is null or (country_code = upper(country_code) and country_code ~ '^[A-Z]{2}$'))
);
call tezgah_register('customer_address');

create index customer_address_customer_id_idx
    on customer_address (scope, customer_id)
    where deleted_at is null;

-- One default of each kind per customer, enforced here rather than by whoever
-- remembers to clear the old one.
create unique index customer_address_default_shipping_key
    on customer_address (scope, customer_id)
    where is_default_shipping and deleted_at is null;
create unique index customer_address_default_billing_key
    on customer_address (scope, customer_id)
    where is_default_billing and deleted_at is null;

create table customer_group (
    id          uuid primary key,
    name        text not null,
    metadata    jsonb,
    deleted_at  timestamptz
);
call tezgah_register('customer_group');

create unique index customer_group_name_key
    on customer_group (scope, name)
    where deleted_at is null;

create table customer_group_customer (
    id                  uuid primary key,
    customer_group_id   uuid not null references customer_group (id) on delete cascade,
    customer_id         uuid not null references customer (id) on delete cascade
);
call tezgah_register('customer_group_customer');

create unique index customer_group_customer_key
    on customer_group_customer (scope, customer_group_id, customer_id);
create index customer_group_customer_customer_id_idx
    on customer_group_customer (scope, customer_id);

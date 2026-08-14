-- Who the buyer is, what the shop is registered as, and what it relied on when
-- it decided. A rate table answers "how much"; none of it answers "why nothing",
-- and an audit ten years later asks the second question.

set lock_timeout = '3s';
set statement_timeout = '60s';

-- The shop's own registrations. Which country it is at home in decides whether
-- a sale crossed a border at all, and an `oss` row is what lets a distance sale
-- be labelled as filed under that scheme rather than charged domestically.
-- No country list lives here on purpose: which countries are in a union changes,
-- and a library that ships the list ships it stale.
create table tax_registration (
    id              uuid primary key,
    country_code    char(2) not null,
    scheme          text not null,
    tax_id          text,
    is_home         boolean not null default false,
    valid_from      timestamptz not null default now(),
    valid_until     timestamptz,
    metadata        jsonb,
    constraint tax_registration_country_code_valid
        check (country_code = upper(country_code) and country_code ~ '^[A-Z]{2}$'),
    constraint tax_registration_scheme_valid
        check (scheme in ('domestic', 'oss', 'ioss', 'other'))
);
call tezgah_register('tax_registration');

create unique index tax_registration_home_key
    on tax_registration (scope)
    where is_home;
create unique index tax_registration_country_scheme_key
    on tax_registration (scope, country_code, scheme);
create index tax_registration_scheme_idx on tax_registration (scope, scheme);

-- The number a buyer gave, and the evidence of the moment it was checked.
-- `evidence` is what the check returned then — a VIES consultation number is
-- proof of what was known that day, and asking again next year proves nothing
-- about the sale that already happened.
create table customer_tax_id (
    id              uuid primary key,
    customer_id     uuid not null,
    tax_id          text not null,
    tax_id_type     text not null,
    tax_id_country  char(2) not null,
    validated_at    timestamptz,
    evidence        text,
    metadata        jsonb,
    constraint customer_tax_id_type_valid
        check (tax_id_type in ('vat', 'ein', 'vkn', 'tckn', 'gst', 'abn', 'other')),
    constraint customer_tax_id_country_valid
        check (tax_id_country = upper(tax_id_country) and tax_id_country ~ '^[A-Z]{2}$')
);
call tezgah_register('customer_tax_id');
call tezgah_fk('customer_tax_id', 'customer_id', 'customer', 'cascade', true);

create index customer_tax_id_customer_idx on customer_tax_id (scope, customer_id);
create unique index customer_tax_id_number_key
    on customer_tax_id (scope, customer_id, tax_id_country, tax_id);

-- A certificate the buyer holds, per jurisdiction, with an end date. tezgah
-- stores it and expires it; whoever verifies it is the host's business.
create table tax_exemption (
    id                      uuid primary key,
    customer_id             uuid not null,
    kind                    text not null,
    reason_code             text,
    certificate_reference   text,
    country_code            char(2) not null,
    province_code           text,
    valid_from              timestamptz not null default now(),
    valid_until             timestamptz,
    verified_at             timestamptz,
    evidence                text,
    metadata                jsonb,
    constraint tax_exemption_kind_valid
        check (kind in (
            'reverse_charge', 'resale_certificate', 'nonprofit',
            'government', 'diplomatic', 'export', 'other'
        )),
    constraint tax_exemption_country_valid
        check (country_code = upper(country_code) and country_code ~ '^[A-Z]{2}$')
);
call tezgah_register('tax_exemption');
call tezgah_fk('tax_exemption', 'customer_id', 'customer', 'cascade', true);

create index tax_exemption_customer_idx
    on tax_exemption (scope, customer_id, country_code);
create index tax_exemption_valid_until_idx on tax_exemption (scope, valid_until);

insert into tezgah_scoped_fk_table (name)
values ('customer_tax_id'), ('tax_exemption')
on conflict do nothing;

-- These two follow the customer out on a delete; what an audit reads is the
-- copy frozen on the order's tax lines, which is why the snapshot carries the
-- number and the certificate rather than a key into here.

-- The taxability code a provider wants as input. It is the shop's, not the
-- provider's: an Avalara or Stripe taxonomy string has to survive changing
-- provider, and a variant that carries one overrides the product it belongs to.
alter table product add column if not exists tax_code text;
alter table product_variant add column if not exists tax_code text;

create index if not exists product_tax_code_idx on product (scope, tax_code);
create index if not exists product_variant_tax_code_idx on product_variant (scope, tax_code);

-- What was decided, on the line it was decided for, frozen there. A zero rate
-- and a zero because the buyer accounted for it are the same number and
-- different invoices, and only `treatment` tells them apart.
--
-- `jurisdiction_*` is why a line may carry several of these: a United States
-- sale is a state line, a county line, a city line and sometimes a district
-- line, each with its own rate and its own remittance.
do $$
declare
    t text;
begin
    foreach t in array array[
        'cart_line_item_tax_line',
        'cart_shipping_method_tax_line',
        'order_line_item_tax_line',
        'order_shipping_method_tax_line'
    ] loop
        execute format($f$
            alter table %I
                add column if not exists treatment text not null default 'standard',
                add column if not exists jurisdiction_level text,
                add column if not exists jurisdiction_code text,
                add column if not exists jurisdiction_name text,
                add column if not exists tax_code text,
                add column if not exists provider text,
                add column if not exists provider_transaction_id text,
                add column if not exists calculated_at timestamptz,
                add column if not exists address_country_code char(2),
                add column if not exists address_province_code text,
                add column if not exists address_postal_code text,
                add column if not exists tax_id text,
                add column if not exists tax_id_evidence text,
                add column if not exists exemption_id uuid,
                add column if not exists evidence jsonb
        $f$, t);

        execute format(
            'alter table %I add constraint %I check (treatment in '
            '(''standard'', ''zero'', ''exempt'', ''reverse_charge'', ''oss'', ''ioss'', '
            '''out_of_scope''))',
            t, t || '_treatment_valid'
        );

        execute format(
            'alter table %I add constraint %I check (jurisdiction_level is null or '
            'jurisdiction_level in (''country'', ''state'', ''province'', ''county'', '
            '''city'', ''district'', ''special''))',
            t, t || '_jurisdiction_level_valid'
        );

        execute format(
            'create index if not exists %I on %I (scope, provider_transaction_id)',
            t || '_provider_txn_idx', t
        );

        -- `set null`, never cascade: an order's tax lines are the record of what
        -- was charged, and revoking a certificate must not erase the invoice
        -- that relied on it.
        call tezgah_fk(t, 'exemption_id', 'tax_exemption', 'set null', true);
    end loop;
end
$$;

-- A jurisdiction breakdown puts several rows under one line, so `code` alone is
-- no longer what makes a row unique — the jurisdiction it names does too. The
-- old key is dropped rather than kept beside this one, because keeping it is
-- what stops a county line from landing at all.
drop index if exists cart_line_item_tax_line_code_key;
drop index if exists cart_shipping_method_tax_line_code_key;
drop index if exists order_line_item_tax_line_code_key;
drop index if exists order_shipping_method_tax_line_code_key;

create unique index cart_line_item_tax_line_code_key
    on cart_line_item_tax_line
       (scope, cart_line_item_id, code, coalesce(jurisdiction_code, ''));
create unique index cart_shipping_method_tax_line_code_key
    on cart_shipping_method_tax_line
       (scope, cart_shipping_method_id, code, coalesce(jurisdiction_code, ''));
create unique index order_line_item_tax_line_code_key
    on order_line_item_tax_line
       (scope, order_line_item_id, code, coalesce(jurisdiction_code, ''));
create unique index order_shipping_method_tax_line_code_key
    on order_shipping_method_tax_line
       (scope, order_shipping_method_id, code, coalesce(jurisdiction_code, ''));

set lock_timeout = '3s';
set statement_timeout = '60s';

-- What a promotion took off an order line. The cart has had these four tables
-- since 0009; the order kept the same two figures as strings in `metadata`,
-- where nothing could constrain them and a malformed value became zero.
--
-- These hang off `order_line_item` rather than `order_item`, so a new version
-- of the order inherits the discount and the rates with the line instead of
-- copying them; what changes between versions is the quantity, and that lives
-- on `order_item`.
--
-- `promotion_id` has no foreign key for the reason the cart's has none: the
-- promotion may be withdrawn, and the discount already granted still happened.
create table order_line_item_adjustment (
    id                  uuid primary key,
    order_line_item_id  uuid not null references order_line_item (id) on delete cascade,
    promotion_id        uuid,
    code                text,
    amount              numeric(20, 6) not null,
    currency_code       char(3) not null,
    description         text,
    is_tax_inclusive    boolean not null default false,
    provider_id         text,
    metadata            jsonb,
    constraint order_line_item_adjustment_amount_valid check (amount >= 0),
    constraint order_line_item_adjustment_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$')
);
call tezgah_register('order_line_item_adjustment');

create index order_line_item_adjustment_line_item_id_idx
    on order_line_item_adjustment (scope, order_line_item_id);
create index order_line_item_adjustment_promotion_id_idx
    on order_line_item_adjustment (scope, promotion_id)
    where promotion_id is not null;

-- One row per rate, never a blend: an invoice has to print which rate
-- contributed what, and a partial refund has to give that same part back.
-- `rate` is a percentage, matching `tax_rate.rate`: 18 means eighteen percent.
create table order_line_item_tax_line (
    id                  uuid primary key,
    order_line_item_id  uuid not null references order_line_item (id) on delete cascade,
    rate                numeric(8, 5) not null,
    code                text not null,
    name                text not null,
    provider_id         text,
    description         text,
    metadata            jsonb,
    constraint order_line_item_tax_line_rate_valid check (rate >= 0 and rate <= 100)
);
call tezgah_register('order_line_item_tax_line');

create index order_line_item_tax_line_line_item_id_idx
    on order_line_item_tax_line (scope, order_line_item_id);
create unique index order_line_item_tax_line_code_key
    on order_line_item_tax_line (scope, order_line_item_id, code);

-- Shipping is versioned, so these hang off the version's own method row and
-- are copied forward with it.
create table order_shipping_method_adjustment (
    id                          uuid primary key,
    order_shipping_method_id    uuid not null
                                references order_shipping_method (id) on delete cascade,
    promotion_id                uuid,
    code                        text,
    amount                      numeric(20, 6) not null,
    currency_code               char(3) not null,
    description                 text,
    is_tax_inclusive            boolean not null default false,
    provider_id                 text,
    metadata                    jsonb,
    constraint order_shipping_method_adjustment_amount_valid check (amount >= 0),
    constraint order_shipping_method_adjustment_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$')
);
call tezgah_register('order_shipping_method_adjustment');

create index order_shipping_method_adjustment_shipping_method_id_idx
    on order_shipping_method_adjustment (scope, order_shipping_method_id);
create index order_shipping_method_adjustment_promotion_id_idx
    on order_shipping_method_adjustment (scope, promotion_id)
    where promotion_id is not null;

create table order_shipping_method_tax_line (
    id                          uuid primary key,
    order_shipping_method_id    uuid not null
                                references order_shipping_method (id) on delete cascade,
    rate                        numeric(8, 5) not null,
    code                        text not null,
    name                        text not null,
    provider_id                 text,
    description                 text,
    metadata                    jsonb,
    constraint order_shipping_method_tax_line_rate_valid check (rate >= 0 and rate <= 100)
);
call tezgah_register('order_shipping_method_tax_line');

create index order_shipping_method_tax_line_shipping_method_id_idx
    on order_shipping_method_tax_line (scope, order_shipping_method_id);
create unique index order_shipping_method_tax_line_code_key
    on order_shipping_method_tax_line (scope, order_shipping_method_id, code);

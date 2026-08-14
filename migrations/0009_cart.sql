set lock_timeout = '3s';
set statement_timeout = '60s';

-- An address as the cart holds it: a copy, not a pointer at the customer's
-- address book, so editing a saved address never moves an in-flight order.
create table cart_address (
    id              uuid primary key,
    customer_id     uuid references customer (id) on delete set null,
    company         text,
    first_name      text,
    last_name       text,
    address_1       text,
    address_2       text,
    city            text,
    province        text,
    postal_code     text,
    country_code    char(2),
    phone           text,
    metadata        jsonb,
    constraint cart_address_country_code_valid
        check (country_code is null or (country_code = upper(country_code) and country_code ~ '^[A-Z]{2}$'))
);
call tezgah_register('cart_address');

create index cart_address_customer_id_idx on cart_address (scope, customer_id);

-- `customer_id` is null for a guest and filled in when the guest signs in;
-- `email` stands alone because a guest cart still has to be reachable.
-- `completed_at` is the one gate: a cart with it set became an order and is
-- closed to every write after.
create table cart (
    id                  uuid primary key,
    customer_id         uuid references customer (id) on delete set null,
    email               text,
    region_id           uuid references region (id) on delete restrict,
    currency_code            char(3) not null,
    sales_channel_id    uuid references sales_channel (id) on delete set null,
    shipping_address_id uuid references cart_address (id) on delete set null,
    billing_address_id  uuid references cart_address (id) on delete set null,
    -- Two clicks on "place order" are one order: the caller sends a key and
    -- the second attempt collides here rather than charging twice.
    idempotency_key     text,
    completed_at        timestamptz,
    -- Passing it releases whatever the cart reserved; the sweeper is a job.
    expires_at          timestamptz,
    metadata            jsonb,
    constraint cart_currency_valid check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'),
    constraint cart_email_valid check (email is null or email = lower(email))
);
call tezgah_register('cart');

create index cart_customer_id_idx on cart (scope, customer_id);
create index cart_region_id_idx on cart (scope, region_id);
create index cart_sales_channel_id_idx on cart (scope, sales_channel_id);
create index cart_shipping_address_id_idx on cart (scope, shipping_address_id);
create index cart_billing_address_id_idx on cart (scope, billing_address_id);
create index cart_email_idx on cart (scope, email) where completed_at is null;
create index cart_expires_at_idx on cart (scope, expires_at)
    where completed_at is null and expires_at is not null;
create unique index cart_idempotency_key_key on cart (scope, idempotency_key)
    where idempotency_key is not null;

-- The snapshot columns are the point of this table. `variant_id` is nullable
-- and carries no foreign key, so deleting or renaming a product leaves the
-- cart intact and still describable: what a shopper put in is what they see.
create table cart_line_item (
    id                      uuid primary key,
    cart_id                 uuid not null references cart (id) on delete cascade,
    variant_id              uuid,
    product_id              uuid,
    product_title           text not null,
    product_handle          text,
    variant_title           text,
    variant_sku             text,
    variant_barcode         text,
    variant_option_values   jsonb,
    thumbnail               text,
    quantity                integer not null,
    unit_price              numeric(20, 6) not null,
    compare_at_unit_price   numeric(20, 6),
    currency_code                char(3) not null,
    is_tax_inclusive        boolean not null default false,
    is_discountable         boolean not null default true,
    requires_shipping       boolean not null default true,
    metadata                jsonb,
    constraint cart_line_item_quantity_valid check (quantity > 0),
    constraint cart_line_item_unit_price_valid check (unit_price >= 0),
    constraint cart_line_item_currency_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$')
);
call tezgah_register('cart_line_item');

create index cart_line_item_cart_id_idx on cart_line_item (scope, cart_id);
create index cart_line_item_variant_id_idx on cart_line_item (scope, variant_id)
    where variant_id is not null;

-- One row per variant per cart. Adding the same variant twice is a quantity
-- change, and without this two concurrent adds each insert and the shopper is
-- left with two lines nothing merges again.
create unique index cart_line_item_variant_key
    on cart_line_item (scope, cart_id, variant_id)
    where variant_id is not null;

-- What a promotion took off the line. `promotion_id` has no foreign key: the
-- promotion may be withdrawn, and the discount already granted still happened.
create table cart_line_item_adjustment (
    id                  uuid primary key,
    cart_line_item_id   uuid not null references cart_line_item (id) on delete cascade,
    promotion_id        uuid,
    code                text,
    amount              numeric(20, 6) not null,
    currency_code            char(3) not null,
    description         text,
    is_tax_inclusive    boolean not null default false,
    provider_id         text,
    metadata            jsonb,
    constraint cart_line_item_adjustment_amount_valid check (amount >= 0),
    constraint cart_line_item_adjustment_currency_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$')
);
call tezgah_register('cart_line_item_adjustment');

create index cart_line_item_adjustment_line_item_id_idx
    on cart_line_item_adjustment (scope, cart_line_item_id);
create index cart_line_item_adjustment_promotion_id_idx
    on cart_line_item_adjustment (scope, promotion_id)
    where promotion_id is not null;

-- `rate` is a percentage, matching `tax_rate.rate`: 18 means eighteen percent.
create table cart_line_item_tax_line (
    id                  uuid primary key,
    cart_line_item_id   uuid not null references cart_line_item (id) on delete cascade,
    rate                numeric(8, 5) not null,
    code                text not null,
    name                text not null,
    provider_id         text,
    description         text,
    metadata            jsonb,
    constraint cart_line_item_tax_line_rate_valid check (rate >= 0 and rate <= 100)
);
call tezgah_register('cart_line_item_tax_line');

create index cart_line_item_tax_line_line_item_id_idx
    on cart_line_item_tax_line (scope, cart_line_item_id);
create unique index cart_line_item_tax_line_code_key
    on cart_line_item_tax_line (scope, cart_line_item_id, code);

-- The chosen shipping, priced and named as it was when it was chosen.
-- `shipping_option_id` carries no foreign key for the same reason a line item
-- does not: the option can be retired while the cart is still open.
create table cart_shipping_method (
    id                  uuid primary key,
    cart_id             uuid not null references cart (id) on delete cascade,
    shipping_option_id  uuid,
    name                text not null,
    description         text,
    amount              numeric(20, 6) not null,
    currency_code            char(3) not null,
    is_tax_inclusive    boolean not null default false,
    data                jsonb,
    metadata            jsonb,
    constraint cart_shipping_method_amount_valid check (amount >= 0),
    constraint cart_shipping_method_currency_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$')
);
call tezgah_register('cart_shipping_method');

create index cart_shipping_method_cart_id_idx on cart_shipping_method (scope, cart_id);
create index cart_shipping_method_shipping_option_id_idx
    on cart_shipping_method (scope, shipping_option_id)
    where shipping_option_id is not null;
create unique index cart_shipping_method_option_key
    on cart_shipping_method (scope, cart_id, shipping_option_id)
    where shipping_option_id is not null;

create table cart_shipping_method_adjustment (
    id                          uuid primary key,
    cart_shipping_method_id     uuid not null
                                references cart_shipping_method (id) on delete cascade,
    promotion_id                uuid,
    code                        text,
    amount                      numeric(20, 6) not null,
    currency_code                    char(3) not null,
    description                 text,
    is_tax_inclusive            boolean not null default false,
    provider_id                 text,
    metadata                    jsonb,
    constraint cart_shipping_method_adjustment_amount_valid check (amount >= 0),
    constraint cart_shipping_method_adjustment_currency_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$')
);
call tezgah_register('cart_shipping_method_adjustment');

create index cart_shipping_method_adjustment_shipping_method_id_idx
    on cart_shipping_method_adjustment (scope, cart_shipping_method_id);
create index cart_shipping_method_adjustment_promotion_id_idx
    on cart_shipping_method_adjustment (scope, promotion_id)
    where promotion_id is not null;

create table cart_shipping_method_tax_line (
    id                          uuid primary key,
    cart_shipping_method_id     uuid not null
                                references cart_shipping_method (id) on delete cascade,
    rate                        numeric(8, 5) not null,
    code                        text not null,
    name                        text not null,
    provider_id                 text,
    description                 text,
    metadata                    jsonb,
    constraint cart_shipping_method_tax_line_rate_valid check (rate >= 0 and rate <= 100)
);
call tezgah_register('cart_shipping_method_tax_line');

create index cart_shipping_method_tax_line_shipping_method_id_idx
    on cart_shipping_method_tax_line (scope, cart_shipping_method_id);
create unique index cart_shipping_method_tax_line_code_key
    on cart_shipping_method_tax_line (scope, cart_shipping_method_id, code);

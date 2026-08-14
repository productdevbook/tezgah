set lock_timeout = '3s';
set statement_timeout = '60s';

-- Who can actually carry a parcel. Rows are written by the host registering its
-- providers, so losing one must not silently delete the shipments it made.
create table fulfillment_provider (
    id          uuid primary key,
    name        text not null,
    is_enabled  boolean not null default true
);
call tezgah_register('fulfillment_provider');

create unique index fulfillment_provider_name_key on fulfillment_provider (scope, name);

-- A way of handing goods over — delivery, or pickup at a counter.
create table fulfillment_set (
    id          uuid primary key,
    name        text not null,
    type        text not null,
    metadata    jsonb,
    constraint fulfillment_set_type_valid check (type in ('shipping', 'pickup'))
);
call tezgah_register('fulfillment_set');

create unique index fulfillment_set_name_key on fulfillment_set (scope, name);

-- Where that way of handing over reaches.
create table service_zone (
    id                  uuid primary key,
    name                text not null,
    fulfillment_set_id  uuid not null references fulfillment_set (id) on delete cascade,
    metadata            jsonb
);
call tezgah_register('service_zone');

create unique index service_zone_name_key on service_zone (scope, name);
create index service_zone_fulfillment_set_id_idx on service_zone (scope, fulfillment_set_id);

-- One piece of the world a zone covers, at whatever resolution the carrier
-- prices at. `postal_expression` is the provider's own postcode language.
create table geo_zone (
    id                  uuid primary key,
    type                text not null default 'country',
    country_code        char(2) not null,
    province_code       text,
    city                text,
    postal_expression   jsonb,
    service_zone_id     uuid not null references service_zone (id) on delete cascade,
    metadata            jsonb,
    constraint geo_zone_type_valid check (type in ('country', 'province', 'city', 'zip')),
    constraint geo_zone_country_code_valid
        check (country_code = upper(country_code) and country_code ~ '^[A-Z]{2}$'),
    constraint geo_zone_province_code_valid
        check (type = 'country' or province_code is not null),
    constraint geo_zone_city_valid check (type <> 'city' or city is not null),
    constraint geo_zone_postal_expression_valid
        check (type <> 'zip' or postal_expression is not null)
);
call tezgah_register('geo_zone');

create index geo_zone_service_zone_id_idx on geo_zone (scope, service_zone_id);
create index geo_zone_country_code_idx on geo_zone (scope, country_code, province_code, city);

-- Items sharing a profile ship together; a different profile is a different
-- parcel, which is why a cart can end up with two shipping methods.
create table shipping_profile (
    id          uuid primary key,
    name        text not null,
    type        text not null,
    metadata    jsonb
);
call tezgah_register('shipping_profile');

create unique index shipping_profile_name_key on shipping_profile (scope, name);

create table shipping_option_type (
    id          uuid primary key,
    label       text not null,
    code        text not null,
    description text
);
call tezgah_register('shipping_option_type');

create unique index shipping_option_type_code_key on shipping_option_type (scope, code);

-- What a customer picks at checkout. `calculated` means the amount is asked of
-- the provider at the time, so no amount is stored here for either kind.
create table shipping_option (
    id                      uuid primary key,
    name                    text not null,
    price_type              text not null default 'flat',
    service_zone_id         uuid not null references service_zone (id) on delete cascade,
    shipping_profile_id     uuid references shipping_profile (id) on delete restrict,
    provider_id             uuid references fulfillment_provider (id) on delete restrict,
    shipping_option_type_id uuid references shipping_option_type (id) on delete set null,
    data                    jsonb,
    metadata                jsonb,
    constraint shipping_option_price_type_valid check (price_type in ('flat', 'calculated'))
);
call tezgah_register('shipping_option');

create unique index shipping_option_name_key on shipping_option (scope, name);
create index shipping_option_service_zone_id_idx on shipping_option (scope, service_zone_id);
create index shipping_option_shipping_profile_id_idx
    on shipping_option (scope, shipping_profile_id);
create index shipping_option_provider_id_idx on shipping_option (scope, provider_id);
create index shipping_option_shipping_option_type_id_idx
    on shipping_option (scope, shipping_option_type_id);

-- Whether this option may be offered at all: an attribute of the cart, an
-- operator, and a value the operator reads.
create table shipping_option_rule (
    id                  uuid primary key,
    attribute           text not null,
    operator            text not null,
    value               jsonb,
    shipping_option_id  uuid not null references shipping_option (id) on delete cascade,
    constraint shipping_option_rule_operator_valid
        check (operator in ('in', 'nin', 'eq', 'ne', 'gt', 'gte', 'lt', 'lte'))
);
call tezgah_register('shipping_option_rule');

create index shipping_option_rule_shipping_option_id_idx
    on shipping_option_rule (scope, shipping_option_id);

-- One parcel leaving one location. The timestamps are the whole state machine:
-- null until it happened, and never unset afterwards.
create table fulfillment (
    id                  uuid primary key,
    location_id         uuid not null references stock_location (id) on delete restrict,
    shipping_option_id  uuid references shipping_option (id) on delete set null,
    provider_id         uuid references fulfillment_provider (id) on delete restrict,
    requires_shipping   boolean not null default true,
    packed_at           timestamptz,
    shipped_at          timestamptz,
    delivered_at        timestamptz,
    canceled_at         timestamptz,
    marked_shipped_by   text,
    created_by          text,
    data                jsonb,
    metadata            jsonb,
    constraint fulfillment_shipped_at_valid
        check (shipped_at is null or packed_at is not null),
    constraint fulfillment_delivered_at_valid
        check (delivered_at is null or shipped_at is not null),
    constraint fulfillment_canceled_at_valid
        check (canceled_at is null or shipped_at is null)
);
call tezgah_register('fulfillment');

create index fulfillment_location_id_idx on fulfillment (scope, location_id);
create index fulfillment_shipping_option_id_idx on fulfillment (scope, shipping_option_id);
create index fulfillment_provider_id_idx on fulfillment (scope, provider_id);
create index fulfillment_open_idx on fulfillment (scope, id)
    where delivered_at is null and canceled_at is null;

-- Title, sku and barcode are copied rather than joined: what was in the box is
-- what the label said that day, whatever the catalogue says now.
create table fulfillment_item (
    id                  uuid primary key,
    fulfillment_id      uuid not null references fulfillment (id) on delete cascade,
    inventory_item_id   uuid references inventory_item (id) on delete set null,
    -- The order's item lands in 0011; until then this cannot be a foreign key.
    line_item_id        uuid,
    title               text not null,
    sku                 text,
    barcode             text,
    quantity            integer not null,
    constraint fulfillment_item_quantity_valid check (quantity > 0)
);
call tezgah_register('fulfillment_item');

create index fulfillment_item_fulfillment_id_idx on fulfillment_item (scope, fulfillment_id);
create index fulfillment_item_inventory_item_id_idx
    on fulfillment_item (scope, inventory_item_id);
create index fulfillment_item_line_item_id_idx on fulfillment_item (scope, line_item_id);

create table fulfillment_label (
    id              uuid primary key,
    fulfillment_id  uuid not null references fulfillment (id) on delete cascade,
    tracking_number text not null,
    tracking_url    text,
    label_url       text
);
call tezgah_register('fulfillment_label');

create index fulfillment_label_fulfillment_id_idx on fulfillment_label (scope, fulfillment_id);
create index fulfillment_label_tracking_number_idx
    on fulfillment_label (scope, tracking_number);

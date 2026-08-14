set lock_timeout = '3s';
set statement_timeout = '60s';

create table stock_location_address (
    id              uuid primary key,
    address_1       text not null,
    address_2       text,
    company         text,
    city            text,
    country_code    char(2) not null,
    province        text,
    postal_code     text,
    phone           text,
    metadata        jsonb,
    constraint stock_location_address_country_code_valid
        check (country_code = upper(country_code) and country_code ~ '^[A-Z]{2}$')
);
call tezgah_register('stock_location_address');

create index stock_location_address_country_code_idx
    on stock_location_address (scope, country_code);

create table stock_location (
    id          uuid primary key,
    name        text not null,
    address_id  uuid references stock_location_address (id) on delete set null,
    metadata    jsonb
);
call tezgah_register('stock_location');

create unique index stock_location_name_key on stock_location (scope, name);
create index stock_location_address_id_idx on stock_location (scope, address_id);

-- Which channels may ship from a location: an unlisted location is invisible to
-- that channel's availability, not merely unpreferred.
create table stock_location_sales_channel (
    id                  uuid primary key,
    stock_location_id   uuid not null references stock_location (id) on delete cascade,
    sales_channel_id    uuid not null references sales_channel (id) on delete cascade
);
call tezgah_register('stock_location_sales_channel');

create unique index stock_location_sales_channel_key
    on stock_location_sales_channel (scope, stock_location_id, sales_channel_id);
create index stock_location_sales_channel_sales_channel_id_idx
    on stock_location_sales_channel (scope, sales_channel_id);

-- What is counted, which is not what is sold: several variants may be the same
-- physical thing, and one variant may consume several of these.
create table inventory_item (
    id                  uuid primary key,
    sku                 text,
    title               text,
    description         text,
    thumbnail           text,
    origin_country      char(2),
    hs_code             text,
    mid_code            text,
    material            text,
    weight              integer,
    length              integer,
    height              integer,
    width               integer,
    requires_shipping   boolean not null default true,
    metadata            jsonb,
    deleted_at          timestamptz,
    constraint inventory_item_origin_country_valid
        check (origin_country is null or origin_country ~ '^[A-Z]{2}$'),
    constraint inventory_item_weight_valid check (weight is null or weight >= 0),
    constraint inventory_item_length_valid check (length is null or length >= 0),
    constraint inventory_item_height_valid check (height is null or height >= 0),
    constraint inventory_item_width_valid check (width is null or width >= 0)
);
call tezgah_register('inventory_item');

create unique index inventory_item_sku_key
    on inventory_item (scope, sku) where deleted_at is null;
create index inventory_item_live_idx
    on inventory_item (scope, id) where deleted_at is null;

-- One row per item per location: the count itself.
create table inventory_level (
    id                  uuid primary key,
    inventory_item_id   uuid not null references inventory_item (id) on delete cascade,
    location_id         uuid not null references stock_location (id) on delete restrict,
    stocked_quantity    integer not null default 0,
    reserved_quantity   integer not null default 0,
    incoming_quantity   integer not null default 0,
    available_quantity  integer generated always as (stocked_quantity - reserved_quantity) stored,
    metadata            jsonb,
    constraint inventory_level_stocked_quantity_valid check (stocked_quantity >= 0),
    -- No `reserved <= stocked`: a backordered reservation is exactly the case
    -- where more is promised than is held, and the ban would forbid it outright.
    constraint inventory_level_reserved_quantity_valid check (reserved_quantity >= 0),
    constraint inventory_level_incoming_quantity_valid check (incoming_quantity >= 0)
);
call tezgah_register('inventory_level');

create unique index inventory_level_item_location_key
    on inventory_level (scope, inventory_item_id, location_id);
create index inventory_level_location_id_idx on inventory_level (scope, location_id);
create index inventory_level_available_idx
    on inventory_level (scope, inventory_item_id, available_quantity);

-- A claim on stock, not a movement of it: reserving raises the level's
-- `reserved_quantity` and leaves `stocked_quantity` where it was.
create table reservation_item (
    id                  uuid primary key,
    inventory_item_id   uuid not null references inventory_item (id) on delete cascade,
    location_id         uuid not null references stock_location (id) on delete restrict,
    quantity            integer not null,
    -- The cart's line item lands in 0009, so this cannot be a foreign key yet.
    line_item_id        uuid,
    allows_backorder    boolean not null default false,
    expires_at          timestamptz,
    description         text,
    external_id         text,
    created_by          text,
    metadata            jsonb,
    constraint reservation_item_quantity_valid check (quantity > 0)
);
call tezgah_register('reservation_item');

create index reservation_item_inventory_item_id_idx
    on reservation_item (scope, inventory_item_id);
create index reservation_item_location_id_idx on reservation_item (scope, location_id);
create index reservation_item_line_item_id_idx on reservation_item (scope, line_item_id);
create index reservation_item_expires_at_idx
    on reservation_item (scope, expires_at) where expires_at is not null;

-- A bundle is this table with several rows for one variant.
create table variant_inventory_item (
    id                  uuid primary key,
    variant_id          uuid not null references product_variant (id) on delete cascade,
    inventory_item_id   uuid not null references inventory_item (id) on delete restrict,
    required_quantity   integer not null default 1,
    constraint variant_inventory_item_required_quantity_valid check (required_quantity > 0)
);
call tezgah_register('variant_inventory_item');

create unique index variant_inventory_item_key
    on variant_inventory_item (scope, variant_id, inventory_item_id);
create index variant_inventory_item_inventory_item_id_idx
    on variant_inventory_item (scope, inventory_item_id);

set lock_timeout = '3s';
set statement_timeout = '120s';

-- Stock was a scalar per (item, location), so nothing could say *which* units
-- shipped. This gives an item the option of being counted as lots instead: the
-- level stays the total and keeps every existing query answering what it always
-- answered, and the lots underneath it carry the code, the expiry and the date
-- the goods arrived.
--
-- `tracking_mode` defaults to 'quantity', which is what every row already in
-- the table is, and the library skips every statement below for such an item.

alter table inventory_item
    add column if not exists tracking_mode text not null default 'quantity',
    add column if not exists allocation_strategy text not null default 'fefo';

alter table inventory_item drop constraint if exists inventory_item_tracking_mode_valid;
alter table inventory_item
    add constraint inventory_item_tracking_mode_valid
        check (tracking_mode in ('quantity', 'lot', 'serial'));

-- Which units leave first. FEFO for anything with a date on it, FIFO for
-- anything without; explicit selection is a different call, not a third value.
alter table inventory_item drop constraint if exists inventory_item_allocation_strategy_valid;
alter table inventory_item
    add constraint inventory_item_allocation_strategy_valid
        check (allocation_strategy in ('fefo', 'fifo'));

-- A serial is a lot of one rather than a table of its own: allocation, expiry,
-- the recall query and the sum invariant are the same statement for both, and
-- two tables would mean two copies of the conditional update this whole issue
-- turns on. What differs is how many rows one physical unit gets, and that is
-- the item's `tracking_mode`, not a second shape of storage.
create table inventory_lot (
    id                  uuid primary key,
    inventory_item_id   uuid not null,
    location_id         uuid not null,
    lot_code            text not null,
    is_serial           boolean not null default false,
    expires_at          timestamptz,
    received_at         timestamptz not null default now(),
    stocked_quantity    integer not null default 0,
    reserved_quantity   integer not null default 0,
    available_quantity  integer generated always as (stocked_quantity - reserved_quantity) stored,
    supplier_reference  text,
    metadata            jsonb,
    constraint inventory_lot_stocked_quantity_valid check (stocked_quantity >= 0),
    constraint inventory_lot_reserved_quantity_valid check (reserved_quantity >= 0)
);
call tezgah_register('inventory_lot');

-- A lot is counted where it is, the way a level is: the same code arriving at
-- two warehouses is two counts, and they expire on their own dates.
create unique index inventory_lot_code_key
    on inventory_lot (scope, inventory_item_id, location_id, lot_code);
-- A serial names one physical unit, so it may not repeat across locations.
create unique index inventory_lot_serial_key
    on inventory_lot (scope, inventory_item_id, lot_code) where is_serial;
create index inventory_lot_inventory_item_id_idx
    on inventory_lot (scope, inventory_item_id, location_id);
create index inventory_lot_location_id_idx on inventory_lot (scope, location_id);
-- The FEFO order itself, so picking the next lot is a read of this index.
create index inventory_lot_fefo_idx
    on inventory_lot (scope, inventory_item_id, location_id, expires_at, received_at, id);
create index inventory_lot_expires_at_idx
    on inventory_lot (scope, expires_at) where expires_at is not null;

call tezgah_fk('inventory_lot', 'inventory_item_id', 'inventory_item', 'cascade', true);
call tezgah_fk('inventory_lot', 'location_id', 'stock_location', 'restrict', true);

-- Which lots a hold was taken out of. A row per lot rather than a column on
-- `reservation_item`, because one reservation of five may span two lots the
-- moment the first one runs short, and the column would have to lie about it.
create table reservation_lot (
    id                  uuid primary key,
    reservation_item_id uuid not null,
    inventory_lot_id    uuid not null,
    quantity            integer not null,
    constraint reservation_lot_quantity_valid check (quantity > 0)
);
call tezgah_register('reservation_lot');

create index reservation_lot_reservation_item_id_idx
    on reservation_lot (scope, reservation_item_id);
create index reservation_lot_inventory_lot_id_idx
    on reservation_lot (scope, inventory_lot_id);

call tezgah_fk('reservation_lot', 'reservation_item_id', 'reservation_item', 'cascade', true);
call tezgah_fk('reservation_lot', 'inventory_lot_id', 'inventory_lot', 'restrict', true);

-- The row that answers a recall: this lot went in that parcel. `lot_code` and
-- `expires_at` are copied for the same reason `fulfillment_item` copies the
-- title — what the label said the day it shipped is the answer, whatever the
-- lot row says later.
create table fulfillment_lot (
    id                  uuid primary key,
    fulfillment_item_id uuid not null,
    inventory_lot_id    uuid not null,
    lot_code            text not null,
    expires_at          timestamptz,
    quantity            integer not null,
    constraint fulfillment_lot_quantity_valid check (quantity > 0)
);
call tezgah_register('fulfillment_lot');

create index fulfillment_lot_fulfillment_item_id_idx
    on fulfillment_lot (scope, fulfillment_item_id);
create index fulfillment_lot_inventory_lot_id_idx
    on fulfillment_lot (scope, inventory_lot_id, id);
create index fulfillment_lot_lot_code_idx on fulfillment_lot (scope, lot_code);

call tezgah_fk('fulfillment_lot', 'fulfillment_item_id', 'fulfillment_item', 'cascade', true);
call tezgah_fk('fulfillment_lot', 'inventory_lot_id', 'inventory_lot', 'restrict', true);

insert into tezgah_scoped_fk_table (name)
values ('inventory_lot'), ('reservation_lot'), ('fulfillment_lot')
on conflict do nothing;

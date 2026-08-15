set lock_timeout = '3s';
set statement_timeout = '60s';

-- A move between two locations, written as one row rather than as two
-- independent `adjust_stock` calls: the pair is provable afterwards, and
-- `inventory::transfer_stock` writes both level changes and this row in one
-- transaction, so a decrement can never commit without its increment.
create table stock_transfer (
    id                  uuid primary key,
    inventory_item_id   uuid not null,
    from_location_id    uuid not null,
    to_location_id      uuid not null,
    quantity            integer not null,
    status              text not null default 'completed',
    reason              text,
    metadata            jsonb,
    constraint stock_transfer_quantity_valid check (quantity > 0),
    constraint stock_transfer_status_valid check (status in ('completed', 'cancelled')),
    constraint stock_transfer_locations_differ check (from_location_id <> to_location_id)
);
call tezgah_register('stock_transfer');

create index stock_transfer_inventory_item_id_idx
    on stock_transfer (scope, inventory_item_id);
create index stock_transfer_from_location_id_idx
    on stock_transfer (scope, from_location_id);
create index stock_transfer_to_location_id_idx
    on stock_transfer (scope, to_location_id);

call tezgah_fk('stock_transfer', 'inventory_item_id', 'inventory_item', 'restrict', true);
call tezgah_fk('stock_transfer', 'from_location_id', 'stock_location', 'restrict', true);
call tezgah_fk('stock_transfer', 'to_location_id', 'stock_location', 'restrict', true);

-- Which lots moved, for a lot-tracked item, so the receiving location knows
-- which ones arrived — the same job `fulfillment_lot` does for a parcel.
create table stock_transfer_lot (
    id                  uuid primary key,
    stock_transfer_id   uuid not null,
    inventory_lot_id    uuid not null,
    lot_code            text not null,
    expires_at          timestamptz,
    quantity            integer not null,
    constraint stock_transfer_lot_quantity_valid check (quantity > 0)
);
call tezgah_register('stock_transfer_lot');

create index stock_transfer_lot_stock_transfer_id_idx
    on stock_transfer_lot (scope, stock_transfer_id);
create index stock_transfer_lot_inventory_lot_id_idx
    on stock_transfer_lot (scope, inventory_lot_id, id);

call tezgah_fk('stock_transfer_lot', 'stock_transfer_id', 'stock_transfer', 'restrict', true);
call tezgah_fk('stock_transfer_lot', 'inventory_lot_id', 'inventory_lot', 'restrict', true);

insert into tezgah_scoped_fk_table (name)
values ('stock_transfer'), ('stock_transfer_lot')
on conflict do nothing;

insert into tezgah_evidence_table (name) values ('stock_transfer'), ('stock_transfer_lot')
on conflict do nothing;

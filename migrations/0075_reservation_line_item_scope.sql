set lock_timeout = '3s';
set statement_timeout = '120s';

-- `reservation_item.line_item_id` (0007) carried the promise "the cart's line
-- item lands in 0009, so this cannot be a foreign key yet". 0009 landed sixty
-- migrations ago and the key was never added. Worse: the id it names is not
-- always a cart line's. `order.rs` rebinds a reservation from a cart line onto
-- an order line the moment checkout writes the order, so the same column names
-- rows in two different tables depending on where the reservation is in its
-- life. One foreign key cannot express that, so this splits it into the two
-- columns each half of the reservation's life actually points at, and cleans
-- up what the missing key already let happen: a reservation whose line item is
-- gone holds stock nothing will ever release.

alter table reservation_item add column cart_line_item_id uuid;
alter table reservation_item add column order_line_item_id uuid;

do $$
declare
    s uuid;
begin
    for s in select id from tezgah_scope loop
        perform set_config('app.scope', s::text, true);

        update reservation_item ri
        set cart_line_item_id = ri.line_item_id
        where ri.line_item_id is not null
          and exists (
              select 1 from cart_line_item c
              where c.scope = ri.scope and c.id = ri.line_item_id
          );

        update reservation_item ri
        set order_line_item_id = ri.line_item_id
        where ri.line_item_id is not null
          and exists (
              select 1 from order_line_item o
              where o.scope = ri.scope and o.id = ri.line_item_id
          );

        -- Genuinely orphaned: `line_item_id` named neither a cart line nor an
        -- order line. Every one of these is a reservation nothing will ever
        -- release on its own, so it is given back here rather than carried
        -- forward unresolved. `reservation_lot` goes with it (cascade, 0033).
        update inventory_level lvl
        set reserved_quantity = greatest(lvl.reserved_quantity - orphan.quantity, 0)
        from (
            select inventory_item_id, location_id, sum(quantity) as quantity
            from reservation_item
            where scope = s
              and line_item_id is not null
              and cart_line_item_id is null
              and order_line_item_id is null
            group by inventory_item_id, location_id
        ) orphan
        where lvl.scope = s
          and lvl.inventory_item_id = orphan.inventory_item_id
          and lvl.location_id = orphan.location_id;

        update inventory_lot lot
        set reserved_quantity = greatest(lot.reserved_quantity - claim.quantity, 0)
        from (
            select rl.inventory_lot_id, sum(rl.quantity) as quantity
            from reservation_lot rl
            join reservation_item ri on ri.scope = rl.scope and ri.id = rl.reservation_item_id
            where rl.scope = s
              and ri.line_item_id is not null
              and ri.cart_line_item_id is null
              and ri.order_line_item_id is null
            group by rl.inventory_lot_id
        ) claim
        where lot.scope = s and lot.id = claim.inventory_lot_id;

        delete from reservation_item
        where scope = s
          and line_item_id is not null
          and cart_line_item_id is null
          and order_line_item_id is null;
    end loop;

    perform set_config('app.scope', '', true);
end
$$;

alter table reservation_item drop column line_item_id;

drop index if exists reservation_item_line_item_id_idx;
create index reservation_item_cart_line_item_id_idx
    on reservation_item (scope, cart_line_item_id);
create index reservation_item_order_line_item_id_idx
    on reservation_item (scope, order_line_item_id);

-- A hold is a cart's or an order's, never both at once.
alter table reservation_item
    add constraint reservation_item_line_item_exclusive
        check (cart_line_item_id is null or order_line_item_id is null);

set lock_timeout = '3s';
set statement_timeout = '60s';

-- 0022 gave `order_transaction`, `order_summary` and `order_item` AFTER
-- triggers that update the parent `"order"` row, so every child write takes an
-- exclusive lock on it. That is a lock ordering nobody declared, and the call
-- sites took it in both directions: `receive_return` restocked before writing
-- items, `apply_action` and the cancel path wrote items before touching
-- inventory. Two of those on one order deadlocked with 40P01.
--
-- The order is now `"order"` first, then inventory, everywhere. `hold_order`
-- in `src/order.rs` takes the parent row explicitly at the top of the paths
-- that touch both.

comment on function tezgah_order_payment_status_moved() is
    'Updates the parent "order" row, so any write to a child table locks it. '
    'Take "order" before inventory_level; see migration 0035.';

comment on function tezgah_order_fulfillment_status_moved() is
    'Updates the parent "order" row, so any write to a child table locks it. '
    'Take "order" before inventory_level; see migration 0035.';

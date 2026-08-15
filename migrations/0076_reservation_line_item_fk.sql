set lock_timeout = '3s';
set statement_timeout = '60s';

-- 0075 split the column and cleaned what was already orphaned; this is the
-- key itself, composite and scoped so Postgres — which checks a foreign key
-- with row security bypassed — refuses both an orphan and a hold that names
-- another tenant's line.
--
-- `restrict`, not `cascade`: `cart::expire` and `order` cancellation now
-- release a hold before its line item can go, the same way every other
-- history-bearing row in this schema is treated (0025). `cascade` would make
-- the release automatic but silent — a reservation vanishing with no
-- `stock.released` event or audit row behind it — and this crate does not
-- let history disappear quietly.
call tezgah_fk('reservation_item', 'cart_line_item_id', 'cart_line_item', 'restrict', true);
call tezgah_fk('reservation_item', 'order_line_item_id', 'order_line_item', 'restrict', true);

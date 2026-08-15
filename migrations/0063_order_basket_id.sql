set lock_timeout = '3s';
set statement_timeout = '60s';

-- Nullable and nothing backfills it: today's single-seller order carries
-- `null` forever and this migration never touches an existing row, so the
-- `set_config('app.scope', ...)` loop `0024_order_status_backfill` uses does
-- not apply here — there is nothing under any scope for this column to catch
-- up on.
alter table "order" add column if not exists basket_id uuid;

-- The one legitimate cross-scope reference this design needs: a seller's
-- order, in the seller's own scope, naming a basket in the marketplace's.
-- `tezgah_cross_scope_fk` (0062) is what makes this a visible, named
-- exception rather than a regression to the single-column keys #91 removed.
-- `tezgah_cross_scope_fk` already indexes the column it keys, so there is no
-- second index to add here.
call tezgah_cross_scope_fk('order', 'basket_id', 'order_basket', 'restrict');

-- Under a basket, which collection paid is the basket's question to answer,
-- not the order's own — `order.payment_collection_id` stays for a
-- single-seller order exactly as it always has, and is read through the
-- basket instead of set directly once one is attached, so there is one
-- source of truth about which collection paid rather than two that can
-- disagree. `order::effective_payment_collection` is where that read lives.
alter table "order" add constraint order_basket_payment_single_source
    check (basket_id is null or payment_collection_id is null) not valid;
alter table "order" validate constraint order_basket_payment_single_source;

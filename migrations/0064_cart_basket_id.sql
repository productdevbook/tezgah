set lock_timeout = '3s';
set statement_timeout = '60s';

-- A cart's own scope already names the seller it belongs to: add_line joins
-- product_variant with `v.scope = $2`, so nothing but a single seller's
-- variants can ever land in one cart, RLS makes that literally impossible to
-- work around. What nothing records is that two such carts, each entirely
-- normal and single-scope, are one shopper's crossing of more than one
-- seller — the fact checkout::group_by_basket (src/checkout.rs) needs to
-- drive one run per scope and land every resulting order under one
-- order_basket.
--
-- Nullable, so a single-seller shop never sets it and its carts are exactly
-- what they always were. The same cross-scope shape as order.basket_id
-- (0063): a cart, in a seller's own scope, naming a basket in the
-- marketplace's.
alter table cart add column if not exists basket_id uuid;

call tezgah_cross_scope_fk('cart', 'basket_id', 'order_basket', 'restrict');

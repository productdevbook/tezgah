-- Which cart a payment collection was opened for.
--
-- Without it a collection says nothing about whose it is, and the storefront
-- had no way to refuse a shopper naming somebody else's collection: the cart
-- came with the request and was never compared against anything.
--
-- Nullable because a collection opened from the back office belongs to an
-- order rather than a cart, and `set null` because a swept-up cart must not
-- take the record of what was paid with it.

set lock_timeout = '3s';
set statement_timeout = '60s';

alter table payment_collection
    add column cart_id uuid references cart (id) on delete set null;

create index payment_collection_cart_id_idx on payment_collection (scope, cart_id);

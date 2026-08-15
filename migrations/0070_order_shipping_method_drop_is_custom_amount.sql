set lock_timeout = '3s';
set statement_timeout = '60s';

-- `is_custom_amount` (0011) was never read or written anywhere in `src/`: not
-- by the insert that creates a row, not by `carry_forward`'s copy, not on the
-- `OrderShippingMethod` struct. No writer in this crate distinguishes a
-- hand-typed shipping amount from one resolved off a rate — `insert_shipping`
-- takes whatever amount its caller already settled on, and neither
-- `checkout.rs`'s rate-resolved callers nor `admin_order.rs`'s
-- `NewShippingIn` (the operator's own typed-amount path) ever diverged on it
-- downstream, because nothing in this crate reprices a placed order's
-- shipping once it is written. A flag nothing can honour is worse than no
-- flag: it tells a reader a capability exists that does not.
alter table order_shipping_method
    drop column is_custom_amount;

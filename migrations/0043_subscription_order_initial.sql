set lock_timeout = '3s';
set statement_timeout = '120s';

-- A contract's first period runs from the moment it is created, not from a
-- renewal, so it has no `renewal` row to write. `settlement::capture` writes
-- one with `kind = 'initial'` on the order that sold it, at `cycle` 0 — the
-- same unique key every other period is kept single-billed by.
alter table subscription_order drop constraint subscription_order_kind_valid;
alter table subscription_order
    add constraint subscription_order_kind_valid check (kind in ('renewal', 'initial'));

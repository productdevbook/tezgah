set lock_timeout = '3s';
set statement_timeout = '120s';

-- Stage three: prepaid terms. Billing and delivery are already separate
-- policies on `selling_plan` (0042); this is the contract's own clock for the
-- second one. `delivery_cycle` counts deliveries produced since the period
-- was last billed — the bundled first one is `subscription_order`'s existing
-- row at `delivery_sequence` 0 — and `next_delivery_at` is null on a plan
-- with no delivery interval of its own, which bills and delivers together.
alter table subscription add column if not exists next_delivery_at timestamptz;
alter table subscription add column if not exists delivery_cycle integer not null default 0;

alter table subscription drop constraint subscription_counters_valid;
alter table subscription
    add constraint subscription_counters_valid
    check (cycle >= 0 and line_version >= 1 and dunning_attempts >= 0 and delivery_cycle >= 0);

-- One order can still be found by its billing cycle, and several can now
-- share one when a prepaid term delivers more than once before it bills
-- again.
alter table subscription_order add column if not exists delivery_sequence integer not null default 0;

drop index subscription_order_cycle_key;
create unique index subscription_order_cycle_key
    on subscription_order (scope, subscription_id, cycle, delivery_sequence);

alter table subscription_order drop constraint subscription_order_kind_valid;
alter table subscription_order
    add constraint subscription_order_kind_valid check (kind in ('renewal', 'initial', 'delivery'));

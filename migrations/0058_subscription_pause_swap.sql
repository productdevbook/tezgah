set lock_timeout = '3s';
set statement_timeout = '120s';

-- Stage two: pause, resume, skip and swap. `paused` was left out of the status
-- check in 0042 until something wrote it — this is that something.
alter table subscription drop constraint subscription_status_valid;
alter table subscription
    add constraint subscription_status_valid
    check (status in ('active', 'past_due', 'paused', 'cancelled', 'expired'));

-- A swap between two accumulates a signed balance rather than moving money on
-- the spot: a credit is granted immediately through `credit::grant_store_
-- credit` (its own `Action::Settle`), but a charge is not taken off-session
-- outside a renewal, so it waits here for the next one to collect it.
alter table subscription add column if not exists pending_adjustment numeric(19, 4) not null default 0;

alter table subscription_event drop constraint subscription_event_kind_valid;
alter table subscription_event
    add constraint subscription_event_kind_valid
    check (kind in ('created', 'renewed', 'price_changed', 'payment_failed',
                    'dunning_exhausted', 'cancelled', 'expired', 'paused', 'resumed',
                    'skipped', 'swapped', 'prorated'));

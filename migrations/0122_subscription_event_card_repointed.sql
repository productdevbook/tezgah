set lock_timeout = '3s';
set statement_timeout = '60s';

-- #197: repointing a contract at a different saved card writes an event of
-- its own, the way pausing or swapping one already does.
alter table subscription_event drop constraint subscription_event_kind_valid;
alter table subscription_event
    add constraint subscription_event_kind_valid
    check (kind in ('created', 'renewed', 'price_changed', 'payment_failed',
                    'dunning_exhausted', 'cancelled', 'expired', 'paused', 'resumed',
                    'skipped', 'swapped', 'prorated', 'card_repointed'));

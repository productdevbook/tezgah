set lock_timeout = '3s';
set statement_timeout = '60s';

-- A provider that reports an amount other than the one asked for has still
-- taken the money, so the collection has to be able to say so out loud rather
-- than be rolled back into agreement with a number nobody was charged.
alter table payment_collection
    drop constraint payment_collection_status_valid;

alter table payment_collection
    add constraint payment_collection_status_valid check (status in (
        'not_paid',
        'awaiting',
        'partially_authorized',
        'authorized',
        'partially_captured',
        'captured',
        'partially_refunded',
        'refunded',
        'canceled',
        'failed',
        'mismatch'
    ));

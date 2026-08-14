-- Handing an order to somebody else. Nothing about the goods moves, so this is
-- not an `order_change`: it is its own small state machine.

set lock_timeout = '3s';
set statement_timeout = '60s';

-- The token is never stored, only its hash: a leaked table is then not a
-- pile of working claim links.
create table order_transfer (
    id               uuid primary key,
    order_id         uuid not null references "order" (id) on delete cascade,
    from_customer_id uuid,
    to_customer_id   uuid,
    to_email         text not null,
    token_hash       text not null,
    status           text not null default 'requested',
    expires_at       timestamptz not null,
    requested_by     text,
    requested_at     timestamptz not null default now(),
    settled_at       timestamptz,
    constraint order_transfer_status_valid
        check (status in ('requested', 'accepted', 'declined', 'canceled'))
);
call tezgah_register('order_transfer');

create index order_transfer_order_idx on order_transfer (scope, order_id);
create index order_transfer_to_customer_idx
    on order_transfer (scope, to_customer_id)
    where to_customer_id is not null;
create index order_transfer_token_idx on order_transfer (scope, token_hash);

-- One order is offered to one person at a time; a second request has to wait
-- for the first to be settled.
create unique index order_transfer_open_key
    on order_transfer (scope, order_id)
    where status = 'requested';

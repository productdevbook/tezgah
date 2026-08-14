set lock_timeout = '3s';
set statement_timeout = '60s';

-- The two columns an operator reads: has this been paid, has this gone out.
-- They are kept, not derived, because a list filters on them — and they are
-- written only by these functions, from the same rows `order::ledger` and the
-- item counters already hold, so there is no second arithmetic to drift from.

-- The ledger's own case, in SQL. `owed` is not summed again here: it is the
-- total `order_summary` already carries for the version, written by the one
-- function that adds an order up.
create or replace function tezgah_order_payment_status(p_scope uuid, p_order uuid)
returns void language plpgsql as $$
declare
    owed        numeric;
    authorized  numeric;
    captured    numeric;
    refunded    numeric;
    became      text;
begin
    select (s.totals->>'total')::numeric into owed
    from order_summary s
    where s.scope = p_scope and s.order_id = p_order
    order by s.version desc
    limit 1;
    owed := coalesce(owed, 0);

    select
        coalesce(sum(amount) filter (where reference = 'payment'), 0),
        coalesce(sum(amount) filter (where reference in ('capture', 'manual')), 0),
        coalesce(-sum(amount) filter (where reference in ('refund', 'order_return',
                                                          'order_exchange', 'order_claim')), 0)
    into authorized, captured, refunded
    from order_transaction
    where scope = p_scope and order_id = p_order;

    became := case
        when refunded > 0 and refunded >= captured then 'refunded'
        when refunded > 0 then 'partially_refunded'
        when captured > 0 and captured >= owed then 'captured'
        when captured > 0 then 'partially_captured'
        when authorized > 0 and authorized >= owed then 'authorized'
        when authorized > 0 then 'partially_authorized'
        else 'not_paid'
    end;

    update "order"
    set payment_status = became
    where scope = p_scope and id = p_order and payment_status is distinct from became;
end
$$;

-- The item counters, read at the order's latest version. An order edit copies
-- the counters forward, so the newest version is the whole picture.
create or replace function tezgah_order_fulfillment_status(p_scope uuid, p_order uuid)
returns void language plpgsql as $$
declare
    ordered     bigint;
    fulfilled   bigint;
    shipped     bigint;
    delivered   bigint;
    returned    bigint;
    became      text;
begin
    select coalesce(sum(quantity), 0),
           coalesce(sum(fulfilled_quantity), 0),
           coalesce(sum(shipped_quantity), 0),
           coalesce(sum(delivered_quantity), 0),
           coalesce(sum(return_received_quantity), 0)
    into ordered, fulfilled, shipped, delivered, returned
    from order_item
    where scope = p_scope and order_id = p_order
      and version = (select max(version) from order_item
                     where scope = p_scope and order_id = p_order);

    became := case
        when coalesce(ordered, 0) = 0 then 'not_fulfilled'
        when returned >= ordered then 'returned'
        when returned > 0 then 'partially_returned'
        when delivered >= ordered then 'delivered'
        when delivered > 0 then 'partially_delivered'
        when shipped >= ordered then 'shipped'
        when shipped > 0 then 'partially_shipped'
        when fulfilled >= ordered then 'fulfilled'
        when fulfilled > 0 then 'partially_fulfilled'
        else 'not_fulfilled'
    end;

    update "order"
    set fulfillment_status = became
    where scope = p_scope and id = p_order and fulfillment_status is distinct from became;
end
$$;

create or replace function tezgah_order_payment_status_moved() returns trigger
language plpgsql as $$
declare
    touched record;
begin
    if tg_op = 'DELETE' then touched := old; else touched := new; end if;
    perform tezgah_order_payment_status(touched.scope, touched.order_id);
    return null;
end
$$;

create or replace function tezgah_order_fulfillment_status_moved() returns trigger
language plpgsql as $$
declare
    touched record;
begin
    if tg_op = 'DELETE' then touched := old; else touched := new; end if;
    perform tezgah_order_fulfillment_status(touched.scope, touched.order_id);
    return null;
end
$$;

create trigger order_transaction_payment_status
    after insert or update or delete on order_transaction
    for each row execute function tezgah_order_payment_status_moved();

-- A new version is a new total, and the same money can mean captured against
-- one and partially captured against the next.
create trigger order_summary_payment_status
    after insert or update on order_summary
    for each row execute function tezgah_order_payment_status_moved();

create trigger order_item_fulfillment_status
    after insert or update or delete on order_item
    for each row execute function tezgah_order_fulfillment_status_moved();

-- Rows already in the table. Row-level security is forced, so this reaches
-- only what the connection running the migration is allowed to see.
do $$
declare
    subject record;
begin
    for subject in select scope, id from "order" loop
        perform tezgah_order_payment_status(subject.scope, subject.id);
        perform tezgah_order_fulfillment_status(subject.scope, subject.id);
    end loop;
end
$$;

-- The same pairing `order_change` already carries: a status word without the
-- moment it happened is a cancellation nothing downstream can see.
update order_return set canceled_at = now()
where status = 'canceled' and canceled_at is null;

alter table order_return
    add constraint order_return_canceled_valid
        check ((status = 'canceled') = (canceled_at is not null));

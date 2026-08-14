set lock_timeout = '3s';
set statement_timeout = '120s';

-- `display_id` was `select coalesce(max(display_id), 0) + 1`, which two
-- checkouts committing at the same instant both read and both write. The loser
-- meets the unique index in the middle of the workflow, after the stock is
-- reserved and the card is authorised, and the compensation chain unwinds a
-- sale that was complete in every other respect.
--
-- A counter row per (scope, kind), incremented by the write itself, is the fix.
-- Not a sequence: a sequence is one global stream rather than one per scope,
-- and a rollback burns a number — for an order number a shop reads out on the
-- telephone and reconciles its books against, a gap is a support call.

-- Cancelling an order voids the authorisations it is holding, and the ledger
-- has to say so or an order nobody can charge any more still reads as
-- authorised. The hold coming off is a negative movement against the payment
-- that took it, which nets the authorisation to nothing.
alter table order_transaction drop constraint if exists order_transaction_reference_valid;
alter table order_transaction
    add constraint order_transaction_reference_valid check (
        reference is null or reference in (
            'capture',
            'refund',
            'payment',
            'payment_canceled',
            'credit_line',
            'order_return',
            'order_exchange',
            'order_claim',
            'manual'
        )
    );

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
        coalesce(sum(amount) filter (where reference in ('payment', 'payment_canceled')), 0),
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

create table if not exists display_counter (
    id      uuid primary key,
    kind    text not null,
    next    bigint not null,
    constraint display_counter_next_valid check (next > 0)
);
call tezgah_register('display_counter');

create unique index if not exists display_counter_kind_key on display_counter (scope, kind);

-- Every scope that already has numbered rows continues from where it is, so no
-- shop sees its order numbers restart.
-- Scope by scope, because `tezgah_scoped` forces row-level security and a
-- migration sets no `app.scope` of its own; see 0024.
do $$
declare
    s uuid;
    t text;
begin
    for s in select id from tezgah_scope loop
        perform set_config('app.scope', s::text, true);

        foreach t in array array['order', 'order_return', 'order_exchange', 'order_claim'] loop
            execute format(
                'insert into display_counter (id, scope, kind, next)
                 select gen_random_uuid(), $1, %L, max(display_id)
                 from %I where scope = $1 and display_id is not null
                 having max(display_id) is not null
                 on conflict (scope, kind) do nothing',
                t, t
            ) using s;
        end loop;
    end loop;
end
$$;

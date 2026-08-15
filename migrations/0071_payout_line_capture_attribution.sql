set lock_timeout = '3s';
set statement_timeout = '60s';

-- #144: a refund unwound the payout ledger at the order's blended
-- commission rate (commission_to_date / captured_to_date) rather than the
-- rate the capture it actually reverses earned at. The two only diverge once
-- an order is captured in parts across a commission rate change, which is
-- why this was invisible with one capture per order.
--
-- capture_id freezes which capture a line concerns. It is deliberately not
-- the same field as reference_id: reference_id is the event a webhook
-- redelivery dedupes on (a capture's own id for an earning line, a refund's
-- id for a reversal), and one refund can span more than one capture, each
-- fragment needing its own rate without losing per-event idempotency.
alter table payout_line add column capture_id uuid;

call tezgah_fk('payout_line', 'capture_id', 'capture', 'restrict', true);

create index payout_line_capture_idx
    on payout_line (scope, capture_id) where capture_id is not null;

-- Backfill: an earning line's reference_id has named the capture it was
-- earned against since 0067 — carry that into the new column. A refund line
-- written before this migration blended an order's whole history at the
-- time and cannot be un-blended after the fact; it is left null rather than
-- guessed at.
do $$
declare
    s uuid;
begin
    for s in select id from tezgah_scope loop
        perform set_config('app.scope', s::text, true);

        update payout_line
           set capture_id = reference_id
         where reference in ('seller_share', 'commission')
           and capture_id is null;
    end loop;

    perform set_config('app.scope', '', true);
end
$$;

-- A refund can now write one row per capture it touches, all sharing the
-- refund's own reference_id — widen the idempotency key so capture_id tells
-- those rows apart instead of the second one silently being dropped as a
-- redelivery of the first.
drop index payout_line_idempotency_key;
create unique index payout_line_idempotency_key
    on payout_line (scope, order_id, reference, reference_id, capture_id)
    where reference_id is not null and order_id is not null;

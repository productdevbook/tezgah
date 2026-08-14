set lock_timeout = '3s';
set statement_timeout = '120s';

-- The link `settlement::start_first_period` and the renewal workflow's
-- `create_order` step both need — which contract this order was placed
-- under — was carried in `metadata->>'subscription'`: untyped, unindexed and
-- unenforced, so an id could name a contract in another scope, or nothing at
-- all, and a capture would still report success without starting a period.
-- A real column, checked by `tezgah_fk` the way every other reference here
-- is, replaces it.

alter table "order" add column if not exists subscription_id uuid;

create index if not exists order_subscription_idx
    on "order" (scope, subscription_id) where subscription_id is not null;

-- Nullable and empty everywhere until the backfill below, so the constraint
-- validates instantly and the column can be relied on before it is filled.
call tezgah_fk('order', 'subscription_id', 'subscription', 'restrict', true);

-- Rows already carrying the old JSON convention. Only a value that is both a
-- well-formed uuid and names a subscription in the same scope is moved: a
-- malformed key or one pointing at another scope's contract is exactly what
-- this column exists to stop being possible, so such a row is left with no
-- link rather than one the constraint would refuse.
do $$
declare
    s uuid;
begin
    for s in select id from tezgah_scope loop
        perform set_config('app.scope', s::text, true);

        update "order" o
        set subscription_id = (o.metadata ->> 'subscription')::uuid
        where o.subscription_id is null
          and o.metadata ->> 'subscription' ~
              '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$'
          and exists (
              select 1 from subscription sub
              where sub.scope = o.scope
                and sub.id = (o.metadata ->> 'subscription')::uuid
          );
    end loop;

    perform set_config('app.scope', '', true);
end
$$;

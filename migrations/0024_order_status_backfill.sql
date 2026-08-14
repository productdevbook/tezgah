set lock_timeout = '3s';
set statement_timeout = '60s';

-- 0022's two backfills reached nothing and the constraint that followed them
-- reached everything.
--
-- Migrations run as the table owner, `tezgah_scoped` FORCES row-level
-- security, and nothing sets `app.scope` before a migration. So the policy is
-- `scope = null`, which is never true, and every plain statement a migration
-- writes against a scoped table matches zero rows. `ALTER TABLE ... ADD
-- CONSTRAINT` is not a policy-filtered read, so it validated rows the
-- backfill above it could not see, and aborted the migration on any database
-- that had one.
--
-- The pattern below is the one every later migration touching a scoped table
-- must use: announce each scope in turn, then do the work.

alter table order_return drop constraint if exists order_return_canceled_valid;

do $$
declare
    s       uuid;
    subject record;
begin
    for s in select id from tezgah_scope loop
        perform set_config('app.scope', s::text, true);

        -- `now()` would date every historical cancellation to this migration.
        update order_return
        set canceled_at = updated_at
        where status = 'canceled' and canceled_at is null;

        -- The status word is the authoritative one — it is what every read
        -- filters on — so a stray timestamp on a return that is still open is
        -- cleared rather than used to cancel it.
        update order_return
        set canceled_at = null
        where status <> 'canceled' and canceled_at is not null;

        for subject in select scope, id from "order" loop
            perform tezgah_order_payment_status(subject.scope, subject.id);
            perform tezgah_order_fulfillment_status(subject.scope, subject.id);
        end loop;
    end loop;

    perform set_config('app.scope', '', true);
end
$$;

alter table order_return
    add constraint order_return_canceled_valid
        check ((status = 'canceled') = (canceled_at is not null)) not valid;

alter table order_return validate constraint order_return_canceled_valid;

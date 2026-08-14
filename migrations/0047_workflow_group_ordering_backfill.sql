set lock_timeout = '3s';
set statement_timeout = '60s';

-- 0017's backfill ran under forced row-level security with no `app.scope`
-- set, so `update workflow_step set group_ordering = ordering where
-- group_ordering is null` matched no rows, and the `not null` right after it
-- validated a column the backfill never filled. Invisible on an empty
-- database — every CI run, every fresh install — and fatal on any that
-- already had workflow steps.
--
-- Harmless to run again where 0017 already succeeded: nothing is left null.

do $$
declare
    s uuid;
begin
    for s in select id from tezgah_scope loop
        perform set_config('app.scope', s::text, true);

        update workflow_step
        set group_ordering = ordering
        where group_ordering is null;
    end loop;

    perform set_config('app.scope', '', true);
end
$$;

alter table workflow_step alter column group_ordering set not null;

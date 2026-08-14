-- Every table tezgah owns carries a scope and reads it back from the
-- connection. A host serving one shop sets it once to a fixed uuid; a host
-- serving many sets it per transaction and Postgres does the rest.

create table if not exists tezgah_scope (
    id          uuid primary key,
    created_at  timestamptz not null default now()
);

-- Read by every row-level security policy. `current_setting(..., true)`
-- returns null rather than raising when nothing set it, so a connection that
-- forgot sees no rows instead of seeing everyone's.
create or replace function tezgah_current_scope() returns uuid
language sql stable as $$
    select nullif(current_setting('app.scope', true), '')::uuid
$$;

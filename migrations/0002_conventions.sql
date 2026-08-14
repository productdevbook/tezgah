-- Conventions every later migration follows, and the machinery that lets a
-- schema test prove it did.

-- Stamps `updated_at` so no writer has to remember to.
create or replace function tezgah_touch() returns trigger
language plpgsql as $$
begin
    new.updated_at = now();
    return new;
end
$$;

-- Gives a table the shape every tezgah table has: a scope, timestamps, the
-- touch trigger, row-level security forced on, and a policy that admits only
-- rows belonging to the scope the connection announced.
--
-- Forced, not merely enabled: a table's owner is exempt from its own policies
-- otherwise, and the owner is who migrations run as.
create or replace procedure tezgah_scoped(table_name text)
language plpgsql as $$
declare
    quoted text := quote_ident(table_name);
begin
    execute format('alter table %s add column if not exists scope uuid not null', quoted);
    execute format(
        'alter table %s add column if not exists created_at timestamptz not null default now()',
        quoted
    );
    execute format(
        'alter table %s add column if not exists updated_at timestamptz not null default now()',
        quoted
    );

    execute format(
        'create index if not exists %s on %s (scope, id)',
        quote_ident(table_name || '_scope_idx'),
        quoted
    );

    execute format('drop trigger if exists %s on %s',
        quote_ident(table_name || '_touch'), quoted);
    execute format(
        'create trigger %s before update on %s for each row execute function tezgah_touch()',
        quote_ident(table_name || '_touch'), quoted
    );

    execute format('alter table %s enable row level security', quoted);
    execute format('alter table %s force row level security', quoted);
    execute format('drop policy if exists %s on %s',
        quote_ident(table_name || '_scope'), quoted);
    execute format(
        'create policy %s on %s using (scope = tezgah_current_scope()) '
        'with check (scope = tezgah_current_scope())',
        quote_ident(table_name || '_scope'), quoted
    );
end
$$;

-- Every scoped table, so the schema test has something to check against rather
-- than guessing from names.
create table if not exists tezgah_table (
    name        text primary key,
    registered  timestamptz not null default now()
);

create or replace procedure tezgah_register(table_name text)
language plpgsql as $$
begin
    call tezgah_scoped(table_name);
    insert into tezgah_table (name) values (table_name) on conflict do nothing;
end
$$;

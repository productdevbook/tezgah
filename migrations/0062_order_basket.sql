set lock_timeout = '3s';
set statement_timeout = '60s';

-- A third form of `tezgah_fk`, for the one relationship the other two cannot
-- make: a row in one scope legitimately pointing at a row in another. The
-- scoped form builds `(scope, col) references parent (scope, id)`, same-scope
-- by construction. The unscoped form builds a bare single-column key, which is
-- exactly what #91 swept 33 of away because it let one tenant's row name
-- another's silently. This one is neither: a single-column key, the same
-- shape as the unscoped form, but named its own procedure and recorded in its
-- own registry so `tests/isolation.rs` can tell "a deliberate cross-scope
-- reference" apart from "a key nobody scoped yet" by name rather than by
-- guessing at intent. Every call here is a decision somebody has to defend in
-- review, not a default anything falls into.
create table if not exists tezgah_cross_scope_fk (
    child_table  text not null,
    child_column text not null,
    parent_table text not null,
    primary key (child_table, child_column)
);

create or replace procedure tezgah_cross_scope_fk(
    p_child text,
    p_col text,
    p_parent text,
    p_delete text
)
language plpgsql as $$
declare
    child  oid := quote_ident(p_child)::regclass;
    name   text := p_child || '_' || p_col || '_fkey';
    doomed text;
begin
    for doomed in
        select con.conname
        from pg_constraint con
        join pg_attribute a on a.attrelid = con.conrelid and a.attname = p_col
        where con.contype = 'f' and con.conrelid = child and a.attnum = any (con.conkey)
    loop
        execute format('alter table %I drop constraint %I', p_child, doomed);
    end loop;

    execute format(
        'alter table %I add constraint %I foreign key (%I) references %I (id) on delete %s',
        p_child, name, p_col, p_parent, p_delete
    );
    execute format('create index if not exists %I on %I (%I)',
                   p_child || '_' || p_col || '_idx', p_child, p_col);

    insert into tezgah_cross_scope_fk (child_table, child_column, parent_table)
    values (p_child, p_col, p_parent)
    on conflict (child_table, child_column) do update set parent_table = excluded.parent_table;
end
$$;

-- What a customer sees as one order and pays for once, joining the
-- seller-scoped orders underneath. Lives in the marketplace's own scope, the
-- same one the operator's console already reads as itself rather than through
-- a seller. A single-seller shop never opens one: `order.basket_id` stays
-- null and nothing here is on its path.
create table order_basket (
    id                     uuid primary key,
    display_id             bigint,
    customer_id            uuid,
    currency_code          char(3) not null,
    payment_collection_id  uuid,
    email                  text,
    metadata               jsonb,
    completed_at           timestamptz,
    constraint order_basket_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$')
);
call tezgah_register('order_basket');

create unique index order_basket_display_id_key
    on order_basket (scope, display_id)
    where display_id is not null;
create index order_basket_customer_id_idx
    on order_basket (scope, customer_id)
    where customer_id is not null;

call tezgah_fk('order_basket', 'customer_id', 'customer', 'set null', true);
call tezgah_fk('order_basket', 'payment_collection_id', 'payment_collection', 'restrict', true);

set lock_timeout = '3s';
set statement_timeout = '60s';

-- Which moves an order may make. Not scoped: these are the library's rules,
-- not a shop's, and a shop that could edit them could walk an order anywhere.
create table tezgah_order_status_move (
    was     text not null,
    became  text not null,
    primary key (was, became)
);

insert into tezgah_order_status_move (was, became) values
    ('draft', 'pending'),
    ('draft', 'canceled'),
    ('pending', 'requires_action'),
    ('pending', 'completed'),
    ('pending', 'canceled'),
    ('requires_action', 'pending'),
    ('requires_action', 'completed'),
    ('requires_action', 'canceled'),
    ('completed', 'archived'),
    ('completed', 'canceled');

-- Where an order has been, and what makes a walk backwards impossible rather
-- than merely refused by this crate.
--
-- A check constraint sees one row and not where it came from, so the status
-- column alone cannot forbid `completed` returning to `pending`: anything
-- writing straight to Postgres could do it and nothing would notice.
create table order_status_history (
    id          uuid primary key,
    order_id    uuid not null references "order" (id) on delete cascade,
    was         text,
    became      text not null
);

call tezgah_register('order_status_history');

create index order_status_history_order_idx
    on order_status_history (scope, order_id, created_at desc);

-- Refuses a move that is not in the table, whoever is writing, and writes down
-- the ones it allows.
create or replace function tezgah_order_status_moves() returns trigger
language plpgsql as $$
begin
    if new.status = old.status then
        return new;
    end if;

    if not exists (
        select 1 from tezgah_order_status_move
        where was = old.status and became = new.status
    ) then
        raise exception 'an order cannot go from % to %', old.status, new.status
            using errcode = 'check_violation';
    end if;

    insert into order_status_history (id, scope, order_id, was, became)
    values (gen_random_uuid(), new.scope, new.id, old.status, new.status);

    return new;
end
$$;

create trigger order_status_moves
    before update of status on "order"
    for each row execute function tezgah_order_status_moves();

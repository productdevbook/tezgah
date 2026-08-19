set lock_timeout = '3s';
set statement_timeout = '60s';

-- #191 measured the 44 keys #91 supposedly left single-column and found none
-- of them: 0030 and 0041 already made every one composite, by rewriting the
-- constraint rather than by leaving text in the migration that names each
-- column. A grep for `references` over the migrations still finds the
-- original single-column declarations, because that text was superseded, not
-- deleted — a migration is append-only.
--
-- What #191 actually found is that `tezgah_scoped_fk_table` — the registry
-- `no_key_on_a_scoped_table_can_cross_a_scope` reads instead of asking the
-- catalogue directly — is a one-time backfill from 0026/0030/0041 with no
-- migration re-running it since. Five tables added after 0041 built their
-- keys the right way, with `tezgah_fk(..., true)`, and never registered:
-- `order_basket`, `commission_rule`, `payout_line`, `campaign_budget_usage`,
-- `product_variant_image`. Correct today, and invisible to that test if any
-- of them ever regressed to a bare key — the same shape of gap #191 opens
-- with, one registration away from recurring for table forty-five.

insert into tezgah_scoped_fk_table (name)
select distinct c.relname::text
from pg_constraint con
join pg_class c on c.oid = con.conrelid
join pg_namespace n on n.oid = c.relnamespace and n.nspname = 'public'
join pg_attribute a on a.attrelid = con.conrelid and a.attname = 'scope'
where con.contype = 'f'
  and a.attnum = any (con.conkey)
  and exists (select 1 from tezgah_table t where t.name = c.relname)
on conflict do nothing;

-- The corrective part: proof against the whole catalogue, not the registry
-- that just caught up. `tests/schema.rs` is changed the same way in this
-- pass, so a table added tomorrow is covered whether or not the migration
-- that adds it remembers to insert into `tezgah_scoped_fk_table`.
do $$
declare
    bare text;
begin
    select string_agg(c.relname || '.' || con.conname, ', ') into bare
    from tezgah_table t
    join pg_class c on c.relname = t.name
    join pg_namespace n on n.oid = c.relnamespace and n.nspname = 'public'
    join pg_constraint con on con.conrelid = c.oid and con.contype = 'f'
    where not exists (
        select 1 from pg_attribute a
        where a.attrelid = con.conrelid
          and a.attname = 'scope'
          and a.attnum = any (con.conkey)
    )
    and not exists (
        select 1
        from tezgah_cross_scope_fk x
        join pg_attribute a
            on a.attrelid = con.conrelid and a.attname = x.child_column
        where x.child_table = c.relname and a.attnum = any (con.conkey)
    );

    if bare is not null then
        raise exception 'these keys would still cross a scope: %', bare;
    end if;
end
$$;

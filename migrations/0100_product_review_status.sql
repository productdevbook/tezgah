set lock_timeout = '5s';
set statement_timeout = '60s';

-- #176: a marketplace has no way to gate what a seller lists — `product`
-- only ever moves between `draft`, `published` and `archived`, none of which
-- means "submitted, not yet looked at". Widens the constraint rather than
-- replacing it: existing rows are all still one of the three original
-- values, and stay valid.

alter table product drop constraint product_status_valid;
alter table product add constraint product_status_valid
    check (status in ('draft', 'proposed', 'published', 'archived', 'rejected'));

-- Read by `catalogue::reject_product`, cleared by `catalogue::submit_for_review`.
-- Not a general-purpose note: it is the operator's answer to one specific
-- question, and lives beside the status it explains.
alter table product add column rejected_reason text;

-- Which moves a product's status may make. Shaped exactly like
-- `tezgah_order_status_move` (0020): a check constraint sees one row and not
-- where it came from, so it cannot by itself keep `published` from becoming
-- `proposed` again. The three original statuses keep every move they always
-- had between them — this widens what a product may do, not what it could
-- already do. The review states are a closed loop off `draft`: submit
-- (`draft`/`rejected` to `proposed`), then approve (to `published`) or
-- reject (to `rejected`, resubmittable). Nothing outside that loop reaches
-- `proposed` or `rejected`, and neither reaches `archived` directly.
create table tezgah_product_status_move (
    was     text not null,
    became  text not null,
    primary key (was, became)
);

insert into tezgah_product_status_move (was, became) values
    ('draft', 'published'),
    ('published', 'draft'),
    ('draft', 'archived'),
    ('archived', 'draft'),
    ('published', 'archived'),
    ('archived', 'published'),
    ('draft', 'proposed'),
    ('rejected', 'proposed'),
    ('proposed', 'published'),
    ('proposed', 'rejected');

create or replace function tezgah_product_status_moves() returns trigger
language plpgsql as $$
begin
    if new.status = old.status then
        return new;
    end if;

    if not exists (
        select 1 from tezgah_product_status_move
        where was = old.status and became = new.status
    ) then
        raise exception 'a product cannot go from % to %', old.status, new.status
            using errcode = 'check_violation';
    end if;

    return new;
end
$$;

create trigger product_status_moves
    before update of status on product
    for each row execute function tezgah_product_status_moves();

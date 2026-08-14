-- The catalogue: what is for sale, how it varies, and how it is filed.

set lock_timeout = '5s';
set statement_timeout = '10min';

-- `scope` arrives with tezgah_register, so every unique constraint that starts
-- with it is added after the call rather than in the create table.

create table product_type (
    id          uuid primary key,
    value       text not null,
    metadata    jsonb not null default '{}'::jsonb,
    deleted_at  timestamptz
);
call tezgah_register('product_type');
create unique index product_type_scope_value_key
    on product_type (scope, value) where deleted_at is null;

create table product_collection (
    id          uuid primary key,
    handle      text not null,
    title       text not null,
    metadata    jsonb not null default '{}'::jsonb,
    deleted_at  timestamptz
);
call tezgah_register('product_collection');
create unique index product_collection_scope_handle_key
    on product_collection (scope, handle) where deleted_at is null;

create table product_tag (
    id          uuid primary key,
    value       text not null,
    metadata    jsonb not null default '{}'::jsonb,
    deleted_at  timestamptz
);
call tezgah_register('product_tag');
create unique index product_tag_scope_value_key
    on product_tag (scope, value) where deleted_at is null;

-- `mpath` is the dot-joined ids from the root down to and including this row.
-- It is maintained by trigger, and it is what makes "everything under this
-- category" one index scan instead of a recursive walk.
create table product_category (
    id            uuid primary key,
    parent_id     uuid references product_category (id) on delete restrict,
    mpath         text not null default '',
    name          text not null,
    handle        text not null,
    description   text not null default '',
    rank          integer not null default 0,
    is_active     boolean not null default false,
    is_internal   boolean not null default false,
    metadata      jsonb not null default '{}'::jsonb,
    deleted_at    timestamptz,
    constraint product_category_rank_valid check (rank >= 0)
);
call tezgah_register('product_category');
create unique index product_category_scope_handle_key
    on product_category (scope, handle) where deleted_at is null;
create index product_category_scope_parent_idx
    on product_category (scope, parent_id) where deleted_at is null;
create index product_category_scope_mpath_idx
    on product_category (scope, mpath text_pattern_ops);

-- Rebuilds `mpath` and refuses a parent that is already a descendant. A plain
-- check constraint cannot see the other rows, so the cycle is caught here.
create or replace function tezgah_product_category_path() returns trigger
language plpgsql as $$
declare
    parent_path text := '';
begin
    if new.parent_id is not null then
        if new.parent_id = new.id then
            raise exception 'a category cannot be its own parent';
        end if;
        select mpath into parent_path
            from product_category where id = new.parent_id;
        if parent_path is null then
            raise exception 'parent category % does not exist', new.parent_id;
        end if;
        if parent_path like '%' || new.id::text || '%' then
            raise exception 'category % is already above %', new.parent_id, new.id;
        end if;
        new.mpath := parent_path || '.' || new.id::text;
    else
        new.mpath := new.id::text;
    end if;

    if tg_op = 'UPDATE' and new.mpath is distinct from old.mpath then
        update product_category
            set mpath = new.mpath || substring(mpath from length(old.mpath) + 1)
            where mpath like old.mpath || '.%';
    end if;

    return new;
end
$$;

create trigger product_category_path
    before insert or update of parent_id on product_category
    for each row execute function tezgah_product_category_path();

create table product (
    id                    uuid primary key,
    handle                text not null,
    title                 text not null,
    subtitle              text,
    description           text,
    status                text not null default 'draft',
    thumbnail_url         text,
    is_discountable       boolean not null default true,
    product_type_id       uuid references product_type (id) on delete set null,
    product_collection_id uuid references product_collection (id) on delete set null,
    weight                numeric(20, 6),
    length                numeric(20, 6),
    height                numeric(20, 6),
    width                 numeric(20, 6),
    material              text,
    hs_code               text,
    origin_country        char(2),
    external_id           text,
    metadata              jsonb not null default '{}'::jsonb,
    deleted_at            timestamptz,
    constraint product_status_valid
        check (status in ('draft', 'published', 'archived'))
);
call tezgah_register('product');
create unique index product_scope_handle_key
    on product (scope, handle) where deleted_at is null;
create index product_scope_status_idx
    on product (scope, status, id) where deleted_at is null;
create index product_scope_type_idx
    on product (scope, product_type_id) where deleted_at is null;
create index product_scope_collection_idx
    on product (scope, product_collection_id) where deleted_at is null;

create table product_variant (
    id                 uuid primary key,
    product_id         uuid not null references product (id) on delete cascade,
    title              text not null,
    sku                text,
    barcode            text,
    ean                text,
    upc                text,
    weight             numeric(20, 6),
    length             numeric(20, 6),
    height             numeric(20, 6),
    width              numeric(20, 6),
    material           text,
    hs_code            text,
    origin_country     char(2),
    mid_code           text,
    manages_inventory  boolean not null default true,
    allows_backorder   boolean not null default false,
    rank               integer not null default 0,
    metadata           jsonb not null default '{}'::jsonb,
    deleted_at         timestamptz,
    constraint product_variant_rank_valid check (rank >= 0)
);
call tezgah_register('product_variant');
create index product_variant_scope_product_idx
    on product_variant (scope, product_id) where deleted_at is null;
create unique index product_variant_scope_sku_key
    on product_variant (scope, sku) where sku is not null and deleted_at is null;
create unique index product_variant_scope_barcode_key
    on product_variant (scope, barcode) where barcode is not null and deleted_at is null;

create table product_option (
    id          uuid primary key,
    product_id  uuid not null references product (id) on delete cascade,
    title       text not null,
    rank        integer not null default 0,
    metadata    jsonb not null default '{}'::jsonb,
    deleted_at  timestamptz,
    constraint product_option_rank_valid check (rank >= 0)
);
call tezgah_register('product_option');
create index product_option_scope_product_idx
    on product_option (scope, product_id) where deleted_at is null;
create unique index product_option_scope_product_title_key
    on product_option (scope, product_id, title) where deleted_at is null;

create table product_option_value (
    id          uuid primary key,
    option_id   uuid not null references product_option (id) on delete cascade,
    value       text not null,
    rank        integer not null default 0,
    metadata    jsonb not null default '{}'::jsonb,
    deleted_at  timestamptz,
    constraint product_option_value_rank_valid check (rank >= 0)
);
call tezgah_register('product_option_value');
create index product_option_value_scope_option_idx
    on product_option_value (scope, option_id) where deleted_at is null;
create unique index product_option_value_scope_option_value_key
    on product_option_value (scope, option_id, value) where deleted_at is null;
-- Carried so the pivot's own option_id can be checked against the value's.
create unique index product_option_value_id_option_key
    on product_option_value (id, option_id);

-- One row per axis of one variant. `option_id` is repeated from the value so
-- that "Size twice on the same variant" is a unique violation rather than
-- something the application has to notice.
create table product_variant_option_value (
    id               uuid primary key,
    variant_id       uuid not null references product_variant (id) on delete cascade,
    option_id        uuid not null references product_option (id) on delete cascade,
    option_value_id  uuid not null references product_option_value (id) on delete restrict,
    foreign key (option_value_id, option_id)
        references product_option_value (id, option_id) on delete restrict
);
call tezgah_register('product_variant_option_value');
create unique index product_variant_option_value_scope_variant_option_key
    on product_variant_option_value (scope, variant_id, option_id);
create index product_variant_option_value_scope_value_idx
    on product_variant_option_value (scope, option_value_id);
create index product_variant_option_value_scope_option_idx
    on product_variant_option_value (scope, option_id);
-- Covers the composite foreign key, which the two single-column indexes do not.
create index product_variant_option_value_value_option_idx
    on product_variant_option_value (option_value_id, option_id);

create table product_image (
    id          uuid primary key,
    product_id  uuid not null references product (id) on delete cascade,
    url         text not null,
    alt_text    text,
    rank        integer not null default 0,
    metadata    jsonb not null default '{}'::jsonb,
    constraint product_image_rank_valid check (rank >= 0)
);
call tezgah_register('product_image');
create index product_image_scope_product_idx on product_image (scope, product_id, rank);

create table product_tag_link (
    id          uuid primary key,
    product_id  uuid not null references product (id) on delete cascade,
    tag_id      uuid not null references product_tag (id) on delete cascade
);
call tezgah_register('product_tag_link');
create unique index product_tag_link_scope_product_tag_key
    on product_tag_link (scope, product_id, tag_id);
create index product_tag_link_scope_tag_idx on product_tag_link (scope, tag_id);

create table product_category_link (
    id           uuid primary key,
    product_id   uuid not null references product (id) on delete cascade,
    category_id  uuid not null references product_category (id) on delete cascade
);
call tezgah_register('product_category_link');
create unique index product_category_link_scope_product_category_key
    on product_category_link (scope, product_id, category_id);
create index product_category_link_scope_category_idx
    on product_category_link (scope, category_id);

create table product_sales_channel (
    id                uuid primary key,
    product_id        uuid not null references product (id) on delete cascade,
    sales_channel_id  uuid not null references sales_channel (id) on delete cascade
);
call tezgah_register('product_sales_channel');
create unique index product_sales_channel_scope_product_channel_key
    on product_sales_channel (scope, product_id, sales_channel_id);
create index product_sales_channel_scope_channel_idx
    on product_sales_channel (scope, sales_channel_id);

-- The product row holds the shop's own language; every other one is a row here.
create table product_translation (
    id           uuid primary key,
    product_id   uuid not null references product (id) on delete cascade,
    locale       text not null,
    title        text not null,
    subtitle     text,
    description  text,
    handle       text,
    constraint product_translation_locale_valid
        check (locale ~ '^[a-z]{2,3}(-[A-Za-z0-9]{2,8})*$')
);
call tezgah_register('product_translation');
create unique index product_translation_scope_product_locale_key
    on product_translation (scope, product_id, locale);
create index product_translation_scope_product_idx
    on product_translation (scope, product_id);
create unique index product_translation_scope_locale_handle_key
    on product_translation (scope, locale, handle) where handle is not null;

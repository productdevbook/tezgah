set lock_timeout = '3s';
set statement_timeout = '120s';

-- Selling a file is three objects and no more: what a variant carries, what an
-- order line bought, and what was actually downloaded. tezgah stores a key and
-- a count. It never streams a byte, signs a URL or talks to object storage —
-- `content_key` is whatever the host's storage calls the thing, and only the
-- host can turn one into bytes.

-- A variant may carry several files — a book as epub, mobi and pdf — so this is
-- a table rather than four columns on `product_variant`.
create table digital_content (
    id             uuid primary key,
    variant_id     uuid not null,
    content_key    text not null,
    name           text not null,
    max_downloads  integer,
    valid_days     integer,
    auto_grant     boolean not null default true,
    rank           integer not null default 0,
    deleted_at     timestamptz,
    metadata       jsonb,
    constraint digital_content_max_downloads_valid
        check (max_downloads is null or max_downloads > 0),
    constraint digital_content_valid_days_valid
        check (valid_days is null or valid_days > 0),
    constraint digital_content_rank_valid check (rank >= 0)
);
call tezgah_register('digital_content');

-- Partial on `deleted_at`: a file withdrawn and later put back under the same
-- key is the ordinary case, and the entitlements already granted carry their
-- own frozen copy of the key, so nothing they hold depends on this row.
create unique index digital_content_key
    on digital_content (scope, variant_id, content_key) where deleted_at is null;
create index digital_content_variant_idx
    on digital_content (scope, variant_id, rank) where deleted_at is null;

call tezgah_fk('digital_content', 'variant_id', 'product_variant', 'cascade', true);

-- What somebody bought, frozen at the moment the money arrived. `content_key`,
-- `max_downloads` and `expires_at` are copies rather than joins, the way
-- `fulfillment_lot` copies the lot code and `order_line_item` the title:
-- changing the file on the variant next year must not change what a customer
-- already paid for.
--
-- One row per (line, content), not per unit: quantity on a digital line is a
-- licence count, and `max_downloads` is what bounds the copies.
create table order_entitlement (
    id                  uuid primary key,
    order_id            uuid not null,
    order_line_item_id  uuid not null,
    digital_content_id  uuid not null,
    customer_id         uuid,
    content_key         text not null,
    max_downloads       integer,
    granted_at          timestamptz not null default now(),
    expires_at          timestamptz,
    download_count      integer not null default 0,
    revoked_at          timestamptz,
    revoked_reason      text,
    constraint order_entitlement_download_count_valid
        check (download_count >= 0
               and (max_downloads is null or download_count <= max_downloads)),
    constraint order_entitlement_max_downloads_valid
        check (max_downloads is null or max_downloads > 0),
    constraint order_entitlement_revoked_reason_valid
        check (revoked_reason is null or revoked_at is not null)
);
call tezgah_register('order_entitlement');

-- What makes a redelivered webhook grant once rather than twice, against two
-- deliveries arriving at the same time rather than one after the other.
create unique index order_entitlement_line_content_key
    on order_entitlement (scope, order_line_item_id, digital_content_id);
create index order_entitlement_order_idx on order_entitlement (scope, order_id);
create index order_entitlement_customer_idx
    on order_entitlement (scope, customer_id, created_at, id)
    where customer_id is not null;

call tezgah_fk('order_entitlement', 'order_id', 'order', 'restrict', true);
call tezgah_fk('order_entitlement', 'order_line_item_id', 'order_line_item', 'restrict', true);
call tezgah_fk('order_entitlement', 'digital_content_id', 'digital_content', 'restrict', true);
call tezgah_fk('order_entitlement', 'customer_id', 'customer', 'set null', true);

-- The bearer thing a download link carries. Only the hash is stored, the way
-- `order_transfer.token_hash` and `gift_card.code_hash` are: a leaked table is
-- not a pile of working links.
create table entitlement_token (
    id                    uuid primary key,
    order_entitlement_id  uuid not null,
    token_hash            text not null,
    expires_at            timestamptz not null,
    revoked_at            timestamptz
);
call tezgah_register('entitlement_token');

create unique index entitlement_token_hash_key on entitlement_token (scope, token_hash);
create index entitlement_token_entitlement_idx
    on entitlement_token (scope, order_entitlement_id);

call tezgah_fk('entitlement_token', 'order_entitlement_id', 'order_entitlement', 'cascade', true);

-- Who took what, when, and whether they were let. This is the table a
-- chargeback is answered from, which is why it is evidence and why a refused
-- attempt is written down as carefully as a served one.
create table entitlement_access (
    id                    uuid primary key,
    order_entitlement_id  uuid not null,
    entitlement_token_id  uuid,
    at                    timestamptz not null default now(),
    ip                    text,
    user_agent            text,
    outcome               text not null,
    constraint entitlement_access_outcome_valid
        check (outcome in ('served', 'refused'))
);
call tezgah_register('entitlement_access');

create index entitlement_access_entitlement_idx
    on entitlement_access (scope, order_entitlement_id, at);

call tezgah_fk('entitlement_access', 'order_entitlement_id', 'order_entitlement', 'restrict', true);
call tezgah_fk('entitlement_access', 'entitlement_token_id', 'entitlement_token', 'set null', true);

insert into tezgah_scoped_fk_table (name) values
    ('digital_content'),
    ('order_entitlement'),
    ('entitlement_token'),
    ('entitlement_access')
on conflict do nothing;

insert into tezgah_evidence_table (name) values ('entitlement_access')
on conflict do nothing;

-- The fulfilment ladder, one rung lower. `location_id` was `not null`, so
-- anything delivered without a warehouse — a file, a service, a code — had to
-- invent a fake one or bypass the ladder and write `order_item`'s counters
-- directly, which puts a second writer on counters only `fulfilment` touches.
-- The invariant that matters is kept by the check: a parcel still cannot leave
-- a shelf nobody named. (#105)
alter table fulfillment alter column location_id drop not null;
alter table fulfillment drop constraint if exists fulfillment_location_id_valid;
alter table fulfillment
    add constraint fulfillment_location_id_valid
        check (location_id is not null or requires_shipping = false);

-- Which price set answers for a variant, and which for a shipping option.
--
-- One table per owner rather than one polymorphic table: a real foreign key on
-- both sides, which a `(owner_type, owner_id)` pair cannot have.

set lock_timeout = '3s';
set statement_timeout = '60s';

create table product_variant_price_set (
    id           uuid primary key,
    variant_id   uuid not null references product_variant (id) on delete cascade,
    price_set_id uuid not null references price_set (id) on delete cascade
);
call tezgah_register('product_variant_price_set');

create unique index product_variant_price_set_variant_key
    on product_variant_price_set (scope, variant_id);
create index product_variant_price_set_price_set_idx
    on product_variant_price_set (scope, price_set_id);

create table shipping_option_price_set (
    id                 uuid primary key,
    shipping_option_id uuid not null references shipping_option (id) on delete cascade,
    price_set_id       uuid not null references price_set (id) on delete cascade
);
call tezgah_register('shipping_option_price_set');

create unique index shipping_option_price_set_option_key
    on shipping_option_price_set (scope, shipping_option_id);
create index shipping_option_price_set_price_set_idx
    on shipping_option_price_set (scope, price_set_id);

set lock_timeout = '3s';
set statement_timeout = '60s';

-- #175: `product` and `inventory_item` already carry `external_id` for a
-- second import run to match an existing row against; the other four
-- catalogue models never got it, so re-importing a type, a collection, a tag
-- or a category always makes another one. Nullable and unindexed, exactly
-- `product.external_id`'s own shape — two systems' ids can collide, so a
-- uniqueness constraint here would be enforcing a coincidence.

alter table product_type add column external_id text;
alter table product_collection add column external_id text;
alter table product_tag add column external_id text;
alter table product_category add column external_id text;

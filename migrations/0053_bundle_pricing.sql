set lock_timeout = '3s';
set statement_timeout = '60s';

-- `variant_inventory_item` (0007) already lets one variant consume several
-- inventory items, and stock for a bundle is solved by that alone. What is
-- missing is the bundle's identity as a composition of *products*, so a
-- storefront can list what is inside and a return can value one piece of it.
-- This is the display and pricing counterpart to the inventory-level table.
create table product_bundle_component (
    id                    uuid primary key,
    bundle_variant_id     uuid not null,
    component_variant_id  uuid not null,
    quantity              integer not null default 1,
    sort_order            integer not null default 0,
    constraint product_bundle_component_quantity_valid check (quantity > 0),
    constraint product_bundle_component_not_itself check (bundle_variant_id <> component_variant_id)
);
call tezgah_register('product_bundle_component');

call tezgah_fk('product_bundle_component', 'bundle_variant_id', 'product_variant', 'cascade', true);
call tezgah_fk('product_bundle_component', 'component_variant_id', 'product_variant', 'restrict', true);

create unique index product_bundle_component_key
    on product_bundle_component (scope, bundle_variant_id, component_variant_id);

insert into tezgah_evidence_table (name) values ('product_bundle_component')
on conflict do nothing;

insert into tezgah_scoped_fk_table (name) values ('product_bundle_component')
on conflict do nothing;

-- How a bundle's own price is decided. `null` is not a bundle at all, so a
-- variant that merely happens to appear as somebody else's component is
-- unaffected. `'fixed'` prices the bundle like any other variant, through its
-- own price set — nothing new is needed for that case, only the label.
-- `'components'` prices it as the sum of what its components resolve to,
-- less `bundle_discount_percent`; `src/pricing.rs` is where that sum is
-- worked out and allocated back across the components with `Money::allocate`,
-- because a discount divided across lines and rounded per line is the exact
-- failure `README.md` opens with.
alter table product_variant
    add column bundle_price_mode text,
    add column bundle_discount_percent numeric(5, 2);

alter table product_variant
    add constraint product_variant_bundle_price_mode_valid check (
        bundle_price_mode is null or bundle_price_mode in ('fixed', 'components')
    ) not valid;
alter table product_variant validate constraint product_variant_bundle_price_mode_valid;

alter table product_variant
    add constraint product_variant_bundle_discount_percent_valid check (
        bundle_discount_percent is null
        or (bundle_discount_percent >= 0 and bundle_discount_percent <= 100)
    ) not valid;
alter table product_variant validate constraint product_variant_bundle_discount_percent_valid;

alter table product_variant
    add constraint product_variant_bundle_discount_percent_mode_valid check (
        bundle_discount_percent is null or bundle_price_mode = 'components'
    ) not valid;
alter table product_variant
    validate constraint product_variant_bundle_discount_percent_mode_valid;

-- A bundle line and the lines standing in for its components, so a partial
-- return can value one component and a storefront can render them together.
-- `on delete cascade`: a parent line only ever disappears with the whole cart
-- or order row it belongs to, and a component line orphaned from its parent
-- is not a line anybody asked for.
alter table cart_line_item add column parent_line_item_id uuid;
call tezgah_fk('cart_line_item', 'parent_line_item_id', 'cart_line_item', 'cascade', true);

alter table order_line_item add column parent_line_item_id uuid;
call tezgah_fk('order_line_item', 'parent_line_item_id', 'order_line_item', 'cascade', true);

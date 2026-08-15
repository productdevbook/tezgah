set lock_timeout = '3s';
set statement_timeout = '60s';

-- `cart_line_item_variant_key` (0009) is one-line-per-variant-per-cart, with
-- no regard for `parent_line_item_id` (0053). Two bundles sharing a
-- component, or a bundle sharing one with the same variant bought loose,
-- name the same `(cart_id, variant_id)` and merge into a line that cannot
-- say which bundle it belongs to. Splitting the index on whether a line has
-- a parent keeps today's rule for ordinary lines exactly as it was — a
-- parent-less line still merges on `(cart_id, variant_id)` alone — and gives
-- a bundle's child its own rule: unique per parent as well as per variant, so
-- a shared component gets one line per bundle instead of one for all of them.
drop index cart_line_item_variant_key;

create unique index cart_line_item_variant_key
    on cart_line_item (scope, cart_id, variant_id)
    where variant_id is not null and parent_line_item_id is null;

create unique index cart_line_item_bundle_component_key
    on cart_line_item (scope, cart_id, variant_id, parent_line_item_id)
    where variant_id is not null and parent_line_item_id is not null;

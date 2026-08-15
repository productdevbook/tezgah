set lock_timeout = '3s';
set statement_timeout = '60s';

-- `cart_line_item_variant_key` (0056) is one-line-per-variant-per-cart for a
-- parent-less line, with no regard for `selling_plan_id`. A subscription line
-- and an ordinary line for the same variant name the same
-- `(cart_id, variant_id)` and merge into one line that cannot say which of
-- the two it is, or silently sums their quantities. The same split 0056 made
-- on `parent_line_item_id` applies here: a plan-less line keeps today's rule,
-- and a plan line is unique on its plan as well as its variant, so the two
-- never collide with each other no matter which is added first.
drop index cart_line_item_variant_key;

create unique index cart_line_item_variant_key
    on cart_line_item (scope, cart_id, variant_id)
    where variant_id is not null
      and parent_line_item_id is null
      and selling_plan_id is null;

create unique index cart_line_item_plan_key
    on cart_line_item (scope, cart_id, variant_id, selling_plan_id)
    where variant_id is not null
      and parent_line_item_id is null
      and selling_plan_id is not null;

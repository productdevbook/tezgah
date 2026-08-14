set lock_timeout = '3s';
set statement_timeout = '120s';

-- #100 read `requires_shipping` off `bool_or(inventory_item.requires_shipping)`
-- over a variant's inventory links, coalesced to false where there are none.
-- Correct where a shop tracks stock — a variant consuming no inventory item is
-- exactly a digital one — and wrong where it does not: such a shop links no
-- `inventory_item` to anything at all, so every one of its variants reported
-- false, and checkout stopped asking where the parcel goes.
--
-- The fact moves to the catalogue, the way #111 put
-- `withdrawal_exclusion_reason` on the variant and #120 put `is_giftcard`
-- there: a product knows whether it is a physical thing independently of
-- whether anybody is counting its stock. `catalogue::line_facts` already
-- reads both in one statement; this is the same shape, not a third one.

alter table product_variant add column if not exists requires_shipping boolean;

do $$
declare
    s uuid;
begin
    for s in select id from tezgah_scope loop
        perform set_config('app.scope', s::text, true);

        -- A variant with a tracked inventory link keeps the fact #100 already
        -- worked out for it.
        update product_variant v
        set requires_shipping = linked.ships
        from (
            select vi.variant_id, bool_or(i.requires_shipping) as ships
            from variant_inventory_item vi
            join inventory_item i
              on i.scope = vi.scope and i.id = vi.inventory_item_id and i.deleted_at is null
            group by vi.variant_id
        ) linked
        where v.scope = s and v.id = linked.variant_id and v.requires_shipping is null;

        -- A variant with none is not thereby digital: it is a variant nobody
        -- ever told this column about. It is digital only where something
        -- else already says so.
        update product_variant v
        set requires_shipping = not (
            v.is_giftcard
            or v.withdrawal_exclusion_reason in ('digital_unsealed', 'digital_delivered')
            is true
            or exists (
                select 1 from digital_content dc
                where dc.scope = v.scope and dc.variant_id = v.id and dc.deleted_at is null
            )
        )
        where v.scope = s and v.requires_shipping is null;
    end loop;

    perform set_config('app.scope', '', true);
end
$$;

alter table product_variant alter column requires_shipping set default true;
alter table product_variant alter column requires_shipping set not null;

-- #100's line still defaults `cart_line_item.requires_shipping` to true, which
-- is harmless while `cart::add_line` is the only writer and reads the variant
-- fresh every time.

-- A tax rate rule could narrow to a product, its type, its collection, or a
-- shipping option — never to the one variant actually being sold, so a
-- variant-specific rate never reached a line. A subscription renewal felt it
-- hardest: it recurs monthly, on a total nobody re-checks.
alter table tax_rate_rule drop constraint tax_rate_rule_reference_valid;
alter table tax_rate_rule
    add constraint tax_rate_rule_reference_valid check (
        reference in ('product', 'product_type', 'product_collection', 'shipping_option', 'variant')
    ) not valid;
alter table tax_rate_rule validate constraint tax_rate_rule_reference_valid;

-- A period whose start equals its end is not a period. The isolation seeder
-- wrote `now()` into both ends and `>=` let it through; the constraint was the
-- part that should never have been the compromise.
alter table subscription drop constraint subscription_period_valid;
alter table subscription
    add constraint subscription_period_valid
        check (current_period_end > current_period_start) not valid;
alter table subscription validate constraint subscription_period_valid;

alter table subscription_order drop constraint subscription_order_period_valid;
alter table subscription_order
    add constraint subscription_order_period_valid
        check (period_end > period_start) not valid;
alter table subscription_order validate constraint subscription_order_period_valid;

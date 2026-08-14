set lock_timeout = '3s';
set statement_timeout = '120s';

-- 0026 gave every registered table `unique (scope, id)` and `tezgah_fk` knows
-- how to rewrite a key as `(scope, col) references parent (scope, id)`, but the
-- sweep stopped at the order and money spine. Thirty-three keys were left
-- single-column: the workflow tables, store, customer, the payment-provider
-- links, `return_reason`'s parent, the whole of the fulfilment configuration
-- and `fulfillment` itself, tax and promotion.
--
-- Postgres checks a foreign key with row-level security bypassed, so a bare key
-- is a hole whatever the policies say: one shop's row may name another's,
-- invisibly to every read and entirely real to the constraint, and `cascade`
-- and `set null` then fire across the boundary. Two of these are already on
-- `tezgah_evidence_table` — `fulfillment_label.fulfillment_id` and
-- `campaign_budget.campaign_id` — which is the schema saying the rows must
-- survive a delete while still letting them point at another tenant.
--
-- The delete action each key already carries is kept: this pass is about which
-- columns the key names, not about what a delete does.

-- The runner's own history.
call tezgah_fk('workflow_step', 'run_id', 'workflow_run', 'cascade', true);
call tezgah_fk('workflow_dead_letter', 'run_id', 'workflow_run', 'cascade', true);

-- Where a shop sells, and the credentials a storefront presents.
call tezgah_fk('region_country', 'region_id', 'region', 'set null', true);
call tezgah_fk('store', 'default_region_id', 'region', 'set null', true);
call tezgah_fk('store', 'default_sales_channel_id', 'sales_channel', 'set null', true);
call tezgah_fk('publishable_key_sales_channel', 'publishable_key_id',
               'publishable_key', 'cascade', true);
call tezgah_fk('publishable_key_sales_channel', 'sales_channel_id',
               'sales_channel', 'cascade', true);

-- Who is buying, and where they live.
call tezgah_fk('customer_address', 'customer_id', 'customer', 'cascade', true);
call tezgah_fk('customer_group_customer', 'customer_group_id', 'customer_group', 'cascade', true);
call tezgah_fk('customer_group_customer', 'customer_id', 'customer', 'cascade', true);

-- The provider a stored account and a delivered webhook belong to.
call tezgah_fk('account_holder', 'payment_provider_id', 'payment_provider', 'restrict', true);
call tezgah_fk('payment_webhook_event', 'payment_provider_id',
               'payment_provider', 'restrict', true);

call tezgah_fk('return_reason', 'parent_return_reason_id', 'return_reason', 'restrict', true);

-- Fulfilment configuration, and the parcel itself: a parcel could name another
-- shop's stock location, shipping option or provider.
call tezgah_fk('service_zone', 'fulfillment_set_id', 'fulfillment_set', 'cascade', true);
call tezgah_fk('geo_zone', 'service_zone_id', 'service_zone', 'cascade', true);
call tezgah_fk('shipping_option', 'service_zone_id', 'service_zone', 'cascade', true);
call tezgah_fk('shipping_option', 'shipping_profile_id', 'shipping_profile', 'restrict', true);
call tezgah_fk('shipping_option', 'provider_id', 'fulfillment_provider', 'restrict', true);
call tezgah_fk('shipping_option', 'shipping_option_type_id',
               'shipping_option_type', 'set null', true);
call tezgah_fk('shipping_option_rule', 'shipping_option_id', 'shipping_option', 'cascade', true);
call tezgah_fk('fulfillment', 'location_id', 'stock_location', 'restrict', true);
call tezgah_fk('fulfillment', 'shipping_option_id', 'shipping_option', 'set null', true);
call tezgah_fk('fulfillment', 'provider_id', 'fulfillment_provider', 'restrict', true);
call tezgah_fk('fulfillment_label', 'fulfillment_id', 'fulfillment', 'restrict', true);

call tezgah_fk('tax_region', 'parent_id', 'tax_region', 'cascade', true);
call tezgah_fk('tax_rate', 'tax_region_id', 'tax_region', 'cascade', true);
call tezgah_fk('tax_rate_rule', 'tax_rate_id', 'tax_rate', 'cascade', true);

call tezgah_fk('campaign_budget', 'campaign_id', 'campaign', 'restrict', true);
call tezgah_fk('promotion', 'campaign_id', 'campaign', 'set null', true);
call tezgah_fk('application_method', 'promotion_id', 'promotion', 'cascade', true);
call tezgah_fk('promotion_rule', 'promotion_id', 'promotion', 'cascade', true);
call tezgah_fk('promotion_target_rule', 'application_method_id',
               'application_method', 'cascade', true);
call tezgah_fk('promotion_buy_rule', 'application_method_id',
               'application_method', 'cascade', true);

-- Which physical lot went into which parcel: the join a recall is answered
-- through. Deleting the fulfilment item took the answer to "who received the
-- batch we are pulling" with it, and said nothing.
call tezgah_fk('fulfillment_lot', 'fulfillment_item_id', 'fulfillment_item', 'restrict', true);

insert into tezgah_evidence_table (name) values ('fulfillment_lot')
on conflict do nothing;

-- One issued invoice per order per version.
--
-- 0028 keyed uniqueness on the document — the serial and the authority's
-- identifier — which refuses the same document twice and permits a second sale
-- document for one sale: an integrator asked twice allocates a fresh serial and
-- a fresh identifier, and both indexes let it through. It was worst at
-- `requested`, where `external_id` is still null and the partial index over it
-- does not apply at all, which is the stage a retry actually happens at.
--
-- `cancelled` and `rejected` are excluded so a refused document can be
-- reissued. Nothing else is: correcting an invoice that stands is a credit
-- note, not a second invoice.
create unique index order_invoice_one_per_sale_key
    on order_invoice (scope, order_id, order_version)
    where kind = 'invoice' and status not in ('cancelled', 'rejected');

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

-- With the remainder converted there is no registered table left holding a
-- single-column key, so this raising is the proof the sweep is finished rather
-- than merely further along.
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
    );

    if bare is not null then
        raise exception 'these keys would still cross a scope: %', bare;
    end if;
end
$$;

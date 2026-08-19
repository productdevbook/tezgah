set lock_timeout = '3s';
set statement_timeout = '60s';

-- #184: which shopper (if any) a shipping option is for. A property of the
-- option itself, set once when a shop configures it, rather than a value a
-- caller passes into the rule-evaluation context the way `sales_channel_id`
-- or `item_total` are — a return-only carrier does not change checkout to
-- checkout, so nothing is gained by asking every caller to say so again.
alter table shipping_option
    add column is_return boolean not null default false,
    add column enabled_in_store boolean not null default true;

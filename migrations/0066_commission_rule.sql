set lock_timeout = '3s';
set statement_timeout = '60s';

-- What a seller-scope keeps for itself and what it owes the marketplace, per
-- category. Follows `price_rule`/`promotion_rule`: a rule table, scoped,
-- resolved by matching one attribute rather than a fourth shape of its own.
-- The match here is narrower than either — a category or nothing, never an
-- operator over a set of values — because a seller's commission is a single
-- rate per category, not a condition a cart is checked against. So there is
-- one row per category and, separately, one default row for everything else,
-- the way `campaign_budget` holds one row per campaign: `on conflict (scope,
-- campaign_id) do update` rather than `price_rule`'s general attribute/value/
-- operator engine, which this has no use for.
create table commission_rule (
    id            uuid primary key,
    category_id   uuid,
    kind          text not null,
    value         numeric(20, 6) not null,
    currency_code char(3),
    constraint commission_rule_kind_valid
        check (kind in ('fixed', 'percentage')),
    constraint commission_rule_value_not_negative
        check (value >= 0),
    constraint commission_rule_percentage_bounded
        check (kind <> 'percentage' or value <= 100),
    constraint commission_rule_fixed_needs_currency
        check (kind <> 'fixed' or currency_code is not null),
    constraint commission_rule_currency_code_valid
        check (currency_code is null
               or (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'))
);
call tezgah_register('commission_rule');

-- One rule per category, and at most one default (`category_id is null`) —
-- resolution never has to rank candidates the way `price_rule` does, because
-- there is never more than one to choose from.
create unique index commission_rule_category_key
    on commission_rule (scope, category_id)
    where category_id is not null;
create unique index commission_rule_default_key
    on commission_rule (scope)
    where category_id is null;

-- A category still named by a rule cannot be removed out from under it; the
-- host retires the rule first. Not `cascade`: silently losing a commission
-- rate is a revenue fact, not cleanup.
call tezgah_fk('commission_rule', 'category_id', 'product_category', 'restrict', true);

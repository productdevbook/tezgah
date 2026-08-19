set lock_timeout = '3s';
set statement_timeout = '60s';

-- Two more budget kinds: a cap that spans every promotion a campaign runs,
-- kept per customer (or whatever `attribute` names) rather than in
-- aggregate. `campaign_budget."limit"` is what each is capped at — the same
-- column `spend`/`usage` already use, now read per row of the table below
-- instead of once for the whole campaign.
alter table campaign_budget
    drop constraint campaign_budget_type_valid;

alter table campaign_budget
    add constraint campaign_budget_type_valid
        check (type in ('spend', 'usage', 'use_by_attribute', 'spend_by_attribute')),
    add column attribute text,
    add constraint campaign_budget_attribute_valid
        check (type not in ('use_by_attribute', 'spend_by_attribute') or attribute is not null);

-- One row per attribute value a claim resolved to — a customer id today, or
-- whatever else a host's claiming context can name. `used <= "limit"` is the
-- same atomic claim `campaign_budget` and `promotion_usage` already make;
-- `"limit"` is `campaign_budget."limit"` copied in on this row's first claim,
-- the way `promotion_usage."limit"` copies `promotion.customer_usage_limit` —
-- a cap lowered afterwards does not reach back into a row already opened.
create table campaign_budget_usage (
    id                 uuid primary key,
    campaign_budget_id uuid not null,
    attribute_value    text not null,
    used               numeric(20, 6) not null default 0,
    "limit"            numeric(20, 6),
    constraint campaign_budget_usage_used_valid
        check (used >= 0),
    constraint campaign_budget_usage_limit_valid
        check ("limit" is null or "limit" >= 0),
    constraint campaign_budget_usage_within_limit_valid
        check ("limit" is null or used <= "limit")
);
call tezgah_register('campaign_budget_usage');

create unique index campaign_budget_usage_key
    on campaign_budget_usage (scope, campaign_budget_id, attribute_value);

call tezgah_fk('campaign_budget_usage', 'campaign_budget_id', 'campaign_budget', 'cascade', true);

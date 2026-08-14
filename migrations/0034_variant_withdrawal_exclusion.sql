set lock_timeout = '3s';
set statement_timeout = '120s';

-- `order_line_item.withdrawal_exclusion_reason` existed and nothing could ever
-- write it, so every line claimed a right of withdrawal the shop may not owe.
-- The answer belongs to what is being sold, and the finest thing a line names
-- is a variant: one product can be sold sealed and unsealed, downloaded and
-- posted, and those are different answers.
--
-- Null is the ordinary case — the line may be sent back — which is what every
-- row already here is.

alter table product_variant
    add column if not exists withdrawal_exclusion_reason text;

alter table product_variant
    drop constraint if exists product_variant_withdrawal_exclusion_valid;
alter table product_variant
    add constraint product_variant_withdrawal_exclusion_valid
        check (withdrawal_exclusion_reason is null or withdrawal_exclusion_reason in (
            'custom_made', 'hygiene', 'perishable', 'digital_unsealed',
            'digital_delivered', 'periodical', 'service_started', 'other'
        ));

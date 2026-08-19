set lock_timeout = '3s';
set statement_timeout = '60s';

-- #194: a customer can ask for their saved-card reference to be forgotten,
-- and `subscription.account_holder_id` is `on delete restrict` — so the row
-- has to survive. Scrubbed the way `customer_address` is: the columns that
-- carry PII or the provider's own token emptied, this marking when.
alter table account_holder
    add column deleted_at timestamptz;

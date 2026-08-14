set lock_timeout = '3s';
set statement_timeout = '120s';

-- Every amount is `numeric(20, 6)`, so six is the finest a currency can be
-- rounded to and anything past it would round to a column that cannot hold it.
-- The floor matters more than the ceiling: `u32::try_from` on a negative
-- exponent is what used to fall back to two decimal places without saying so.

alter table currency
    drop constraint if exists currency_exponent_valid;
alter table currency
    add constraint currency_exponent_valid check (exponent between 0 and 6);

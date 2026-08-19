set lock_timeout = '3s';
set statement_timeout = '60s';

-- A third allocation: the value lands on the cheapest units of the target,
-- up to `max_quantity`, the way `buy_get` already picks its free units —
-- rather than every matched line (`each`) or one amount split across all of
-- them (`across`). Meaningless against the whole order, which has no units
-- to sort by price, so the combination is refused here as well as in Rust.
alter table application_method
    drop constraint application_method_allocation_valid;

alter table application_method
    add constraint application_method_allocation_valid
        check (allocation is null or allocation in ('each', 'across', 'once')),
    add constraint application_method_once_target_valid
        check (allocation <> 'once' or target_type <> 'order');

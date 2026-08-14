set lock_timeout = '3s';
set statement_timeout = '60s';

-- Steps that may run at once share a group. `ordering` stays the whole truth
-- about compensation: unwinding walks back through it, group or no group.
alter table workflow_step add column group_ordering integer;

update workflow_step set group_ordering = ordering where group_ordering is null;

alter table workflow_step alter column group_ordering set not null;

create index workflow_step_group_idx on workflow_step (run_id, group_ordering);

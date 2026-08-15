set lock_timeout = '3s';
set statement_timeout = '60s';

-- #134: a step whose provider needs the shopper to do something is neither
-- done nor skipped. It ran, and the run must not report itself finished nor
-- unwind while it is still open, so the run and its step both need a state
-- for it rather than being flattened into one of the existing ones.

alter table workflow_run
    drop constraint if exists workflow_run_state_valid;
alter table workflow_run
    add constraint workflow_run_state_valid
        check (state in ('running', 'compensating', 'waiting', 'done', 'reverted', 'failed'));

alter table workflow_step
    drop constraint if exists workflow_step_state_valid;
alter table workflow_step
    add constraint workflow_step_state_valid
        check (state in (
            'pending', 'invoking', 'done', 'skipped', 'waiting',
            'compensating', 'reverted', 'failed'
        ));

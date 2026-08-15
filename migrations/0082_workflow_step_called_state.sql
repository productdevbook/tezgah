set lock_timeout = '3s';
set statement_timeout = '60s';

-- #158: a step split into prepare/call/record commits `prepare` before the
-- provider is asked, so a crash between the call and `record` must not look
-- like `invoking` — whose lease-expiry reclaim silently permits a second
-- call. `called` is that state: on record it is written by no step yet, and
-- the runner's reclaim resets it to itself, never to `pending`.
alter table workflow_step
    drop constraint if exists workflow_step_state_valid;
alter table workflow_step
    add constraint workflow_step_state_valid
        check (state in (
            'pending', 'invoking', 'called', 'done', 'skipped', 'waiting',
            'compensating', 'reverted', 'failed'
        ));

set lock_timeout = '3s';
set statement_timeout = '60s';

-- A run of a workflow. `transaction_key` is what makes asking twice safe: the
-- same key returns the run already in flight rather than starting a second.
create table workflow_run (
    id               uuid primary key,
    name             text not null,
    transaction_key  text not null,
    state            text not null default 'running',
    input            jsonb not null default '{}'::jsonb,
    output           jsonb,
    failure          text,
    finished_at      timestamptz,

    constraint workflow_run_state_valid check (
        state in ('running', 'compensating', 'done', 'reverted', 'failed')
    )
);

call tezgah_register('workflow_run');

create unique index workflow_run_key_key on workflow_run (scope, name, transaction_key);

-- Runs waiting to be picked up. Partial, because the finished ones are most of
-- the table within a week and none of them is a candidate.
create index workflow_run_pending_idx on workflow_run (scope, state)
    where state in ('running', 'compensating');

-- One row per step per run, holding both directions: what invoking returned,
-- and what compensating will need. Compensation takes its own input rather
-- than the step's output, because undoing a charge needs the charge id even
-- when the step returned nothing.
create table workflow_step (
    id                uuid primary key,
    run_id            uuid not null references workflow_run (id) on delete cascade,
    name              text not null,
    ordering          integer not null,
    state             text not null default 'pending',
    attempts          integer not null default 0,
    max_attempts      integer not null default 3,
    output            jsonb,
    compensate_input  jsonb,
    failure           text,
    run_after         timestamptz not null default now(),
    lease_until       timestamptz,
    locked_by         text,

    constraint workflow_step_state_valid check (
        state in (
            'pending', 'invoking', 'done', 'skipped',
            'compensating', 'reverted', 'failed'
        )
    ),
    constraint workflow_step_attempts_valid check (attempts >= 0 and attempts <= max_attempts)
);

call tezgah_register('workflow_step');

create unique index workflow_step_order_key on workflow_step (run_id, ordering);
create index workflow_step_run_idx on workflow_step (run_id, ordering);

-- Claimed work: due, not leased, and not yet settled.
create index workflow_step_due_idx on workflow_step (run_after)
    where state in ('pending', 'compensating');

-- Runs that gave up. Kept rather than deleted: a workflow that could neither
-- finish nor unwind is the one somebody has to look at.
create table workflow_dead_letter (
    id          uuid primary key,
    run_id      uuid not null references workflow_run (id) on delete cascade,
    step_name   text not null,
    failure     text not null,
    state       jsonb not null
);

call tezgah_register('workflow_dead_letter');

create index workflow_dead_letter_run_idx on workflow_dead_letter (run_id);

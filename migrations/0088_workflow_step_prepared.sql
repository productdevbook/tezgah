set lock_timeout = '3s';
set statement_timeout = '60s';

-- #158 step 3/4: a `ReachingStep`'s `prepare` commits before `call` asks
-- anything, and a later pass — this worker's own resume, or a fresh driver
-- after a crash — has to reconstruct the same `Prepared` (its idempotency
-- key and whatever `call` needs) without re-running `prepare`. Stored
-- separately from `compensate_input`: that column is what `prepare` hands
-- `compensate` to undo, and is deliberately smaller than what `call` needs.
alter table workflow_step
    add column prepared jsonb;

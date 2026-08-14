set lock_timeout = '3s';
set statement_timeout = '60s';

-- A session that authorised and was then cancelled still authorised, and the
-- moment it happened is worth keeping. The check only exists to stop a session
-- that never authorised from carrying a time, so `canceled` belongs in it.
alter table payment_session
    drop constraint payment_session_authorized_at_valid;

alter table payment_session
    add constraint payment_session_authorized_at_valid
        check (authorized_at is null or status in ('authorized', 'captured', 'canceled'));

set lock_timeout = '3s';
set statement_timeout = '60s';

-- Every other entity a shop tags with its own data has this column; a
-- promotion and its campaign did not.
alter table promotion add column metadata jsonb;
alter table campaign add column metadata jsonb;

set lock_timeout = '3s';
set statement_timeout = '60s';

-- 0067 left this out because `tests/isolation.rs`'s seeder read a check
-- correlating two columns as a claim about a third's own domain, and that
-- claim fought `payout_line_reference_valid` for which literal `reference`
-- may hold. The seeder now tells the two kinds of check apart (#143), so the
-- rule belongs here rather than only in `write_line` and `create_payout`.
--
-- Unlike a bundle naming itself as its own component, a payout line breaking
-- this has no safe repair: deleting it would change the very balance the
-- rule protects. `not valid` then `validate` is the whole of this migration;
-- a host whose data fails to validate has a real ledger problem, and this is
-- the only honest way to surface it.
alter table payout_line
    add constraint payout_line_reference_shape
    check (
        (reference <> 'paid_out' and order_id is not null and payout_id is null)
        or
        (reference = 'paid_out' and order_id is null and payout_id is not null)
    ) not valid;

alter table payout_line
    validate constraint payout_line_reference_shape;

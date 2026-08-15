set lock_timeout = '3s';
set statement_timeout = '60s';

-- Who a seller-scope's orders paid, why, and whether the host has actually
-- moved the money. The shape `order_transaction` already has: appended, never
-- updated, signed so a reversal is a second row rather than an edit to the
-- first, and a running balance is `sum(amount)` rather than a column
-- something has to keep in step.
--
-- `payout_line` carries the accruals — a captured order's seller share and
-- the marketplace's commission, and their negative mirrors when an order is
-- refunded. `payout_id` on those stays null forever: they are facts about
-- what was earned, not about a transfer. `payout` is what the host writes
-- when it has actually paid a seller through kasapay or whatever moved the
-- money — never tezgah calling out to do it — and the matching `payout_line`
-- row (`reference = 'paid_out'`, `payout_id` set, `amount` negative) is what
-- brings the running balance back down by what left the shop. A seller
-- refunded after being paid is not a special case: the refund's negative row
-- lands the same way any other one does, and the balance simply goes
-- negative until a later payout's amount is reduced by it — there is no
-- separate debt table, because the ledger is already one.
create table payout (
    id            uuid primary key,
    amount        numeric(20, 6) not null,
    currency_code char(3) not null,
    reference     text not null,
    reference_id  uuid not null,
    metadata      jsonb,
    constraint payout_amount_positive check (amount > 0),
    constraint payout_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$')
);
call tezgah_register('payout');

create unique index payout_reference_key
    on payout (scope, reference, reference_id);

-- `order_id` is null for exactly one reference: `paid_out` is a fact about
-- the scope's whole balance, not about any one order — a payout usually
-- covers several — so it is not forced to point at one arbitrarily. Every
-- other reference is an order's own earning or its reversal and always names
-- one.
create table payout_line (
    id            uuid primary key,
    order_id      uuid,
    payout_id     uuid,
    amount        numeric(20, 6) not null,
    currency_code char(3) not null,
    reference     text not null,
    reference_id  uuid,
    metadata      jsonb,
    constraint payout_line_amount_nonzero check (amount <> 0),
    constraint payout_line_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'),
    constraint payout_line_reference_valid check (reference in (
        'seller_share',
        'commission',
        'refund_seller_share',
        'refund_commission',
        'paid_out'
    ))
    -- Not enforced here: that `order_id` and `payout_id` are set on exactly
    -- opposite references. A `check` correlating them to a specific literal
    -- of `reference` reads, to `tests/isolation.rs`'s generic seeder, as a
    -- second claim about what `reference` itself must hold — it takes any
    -- `column = 'literal'` inside a check as the accepted value for that
    -- column, and a second one here would fight `payout_line_reference_valid`
    -- for which literal wins. `write_line` and `create_payout` are the only
    -- writers and both hold the shape without a database check behind them.
);
call tezgah_register('payout_line');

create index payout_line_order_idx
    on payout_line (scope, order_id) where order_id is not null;
create index payout_line_payout_idx
    on payout_line (scope, payout_id) where payout_id is not null;

-- Redeliverable by (order, reference, its capture or refund id) — a webhook
-- replaying the same capture must not double the seller's earnings, the same
-- guarantee `order_transaction_reference_key` gives the ledger it mirrors.
create unique index payout_line_idempotency_key
    on payout_line (scope, order_id, reference, reference_id)
    where reference_id is not null and order_id is not null;

-- The `paid_out` mirror: idempotency keyed on the payout's own reference
-- rather than an order, since `order_id` is null on this path.
create unique index payout_line_paid_out_idempotency_key
    on payout_line (scope, reference_id)
    where reference = 'paid_out';

-- Evidence: what a seller was owed and what left the shop must survive an
-- order being deleted, the same reasoning `order_transaction` was given in
-- 0025 — so `restrict`, not `cascade`.
call tezgah_fk('payout_line', 'order_id', 'order', 'restrict', true);
call tezgah_fk('payout_line', 'payout_id', 'payout', 'restrict', true);

insert into tezgah_evidence_table (name) values
    ('payout'),
    ('payout_line')
on conflict do nothing;

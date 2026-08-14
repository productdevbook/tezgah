-- Two instruments, deliberately not one table.
--
-- A gift card is a bearer token: whoever holds the code spends it, it expires,
-- and it belongs to nobody in particular. Store credit is a named customer's
-- balance and expires when the shop says so. Medusa keeps them apart for the
-- same reason, and merging them would mean either giving a customer's balance a
-- code somebody could forward or giving a bearer token an owner it does not
-- have.
--
-- Both keep an append-only ledger beside the balance column. The column is what
-- a conditional update decrements under contention; the ledger is what the
-- balance has to equal, and `tests/credit.rs` says so.

set lock_timeout = '3s';
set statement_timeout = '60s';

-- The code is never stored, only its hash — the same reason `order_transfer`
-- keeps a `token_hash`: a leaked table is then not a pile of spendable cards.
create table gift_card (
    id              uuid primary key,
    code_hash       text not null,
    initial_balance numeric(20, 6) not null,
    balance         numeric(20, 6) not null,
    currency_code   char(3) not null,
    issued_order_id uuid,
    customer_id     uuid,
    expires_at      timestamptz,
    disabled_at     timestamptz,
    metadata        jsonb,
    constraint gift_card_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'),
    constraint gift_card_initial_balance_valid check (initial_balance >= 0),
    constraint gift_card_balance_valid check (balance >= 0)
);
call tezgah_register('gift_card');

create unique index gift_card_code_hash_key on gift_card (scope, code_hash);
create index gift_card_customer_id_idx on gift_card (scope, customer_id)
    where customer_id is not null;
create index gift_card_issued_order_id_idx on gift_card (scope, issued_order_id)
    where issued_order_id is not null;
create index gift_card_expires_at_idx on gift_card (scope, expires_at)
    where expires_at is not null;

call tezgah_fk('gift_card', 'issued_order_id', 'order', 'set null', true);
call tezgah_fk('gift_card', 'customer_id', 'customer', 'set null', true);

-- One movement. `amount` is signed the way the balance moves: an issue and a
-- refund add, a redemption takes away. The balance column is the sum of these.
create table gift_card_transaction (
    id                    uuid primary key,
    gift_card_id          uuid not null,
    kind                  text not null,
    amount                numeric(20, 6) not null,
    currency_code         char(3) not null,
    order_id              uuid,
    payment_collection_id uuid,
    reason                text,
    created_by            text,
    metadata              jsonb,
    constraint gift_card_transaction_kind_valid
        check (kind in ('issue', 'redeem', 'refund', 'adjust')),
    constraint gift_card_transaction_amount_valid check (amount <> 0),
    constraint gift_card_transaction_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$')
);
call tezgah_register('gift_card_transaction');

create index gift_card_transaction_card_idx
    on gift_card_transaction (scope, gift_card_id, created_at);
create index gift_card_transaction_collection_idx
    on gift_card_transaction (scope, payment_collection_id)
    where payment_collection_id is not null;

call tezgah_fk('gift_card_transaction', 'gift_card_id', 'gift_card', 'restrict', true);
call tezgah_fk('gift_card_transaction', 'order_id', 'order', 'set null', true);
call tezgah_fk('gift_card_transaction', 'payment_collection_id', 'payment_collection',
               'set null', true);

-- One customer's balance in one currency. Two currencies are two balances:
-- nothing in this schema has ever let them blur.
create table store_credit (
    id              uuid primary key,
    customer_id     uuid not null,
    currency_code   char(3) not null,
    balance         numeric(20, 6) not null default 0,
    disabled_at     timestamptz,
    metadata        jsonb,
    constraint store_credit_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'),
    constraint store_credit_balance_valid check (balance >= 0)
);
call tezgah_register('store_credit');

create unique index store_credit_customer_currency_key
    on store_credit (scope, customer_id, currency_code);

call tezgah_fk('store_credit', 'customer_id', 'customer', 'restrict', true);

create table store_credit_transaction (
    id                    uuid primary key,
    store_credit_id       uuid not null,
    kind                  text not null,
    amount                numeric(20, 6) not null,
    currency_code         char(3) not null,
    order_id              uuid,
    payment_collection_id uuid,
    reason                text,
    created_by            text,
    metadata              jsonb,
    constraint store_credit_transaction_kind_valid
        check (kind in ('issue', 'redeem', 'refund', 'adjust')),
    constraint store_credit_transaction_amount_valid check (amount <> 0),
    constraint store_credit_transaction_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$')
);
call tezgah_register('store_credit_transaction');

create index store_credit_transaction_account_idx
    on store_credit_transaction (scope, store_credit_id, created_at);
create index store_credit_transaction_collection_idx
    on store_credit_transaction (scope, payment_collection_id)
    where payment_collection_id is not null;

call tezgah_fk('store_credit_transaction', 'store_credit_id', 'store_credit', 'restrict', true);
call tezgah_fk('store_credit_transaction', 'order_id', 'order', 'set null', true);
call tezgah_fk('store_credit_transaction', 'payment_collection_id', 'payment_collection',
               'set null', true);

-- What the shopper said they would pay with, before any money moves. The model
-- is `promotion_usage`: the balance is claimed inside the checkout that reserves
-- the stock, not counted when the provider answers.
--
-- Exactly one of `gift_card_id` and `store_credit_id` is set, and that is not a
-- check constraint on purpose: a constraint tying two columns together stops
-- `tests/isolation.rs` seeding the table, and the rule is enforced where the
-- row is written.
create table cart_credit (
    id              uuid primary key,
    cart_id         uuid not null,
    gift_card_id    uuid,
    store_credit_id uuid,
    amount          numeric(20, 6) not null,
    currency_code   char(3) not null,
    constraint cart_credit_amount_valid check (amount > 0),
    constraint cart_credit_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$')
);
call tezgah_register('cart_credit');

create unique index cart_credit_gift_card_key
    on cart_credit (scope, cart_id, gift_card_id)
    where gift_card_id is not null;
create unique index cart_credit_store_credit_key
    on cart_credit (scope, cart_id, store_credit_id)
    where store_credit_id is not null;

call tezgah_fk('cart_credit', 'cart_id', 'cart', 'cascade', true);
call tezgah_fk('cart_credit', 'gift_card_id', 'gift_card', 'restrict', true);
call tezgah_fk('cart_credit', 'store_credit_id', 'store_credit', 'restrict', true);

insert into tezgah_scoped_fk_table (name) values
    ('gift_card'),
    ('gift_card_transaction'),
    ('store_credit'),
    ('store_credit_transaction'),
    ('cart_credit')
on conflict do nothing;

-- A ledger is the one thing that cannot be reconstructed once the row it hangs
-- off is gone.
insert into tezgah_evidence_table (name) values
    ('gift_card_transaction'),
    ('store_credit_transaction')
on conflict do nothing;

-- How much of a collection an instrument carried. Derived by
-- `payment::recompute` from the two ledgers rather than written by whoever
-- redeemed, so it cannot drift from them.
alter table payment_collection
    add column if not exists credit_amount numeric(20, 6) not null default 0;

alter table payment_collection
    drop constraint if exists payment_collection_credit_amount_valid;
alter table payment_collection
    add constraint payment_collection_credit_amount_valid check (credit_amount >= 0);

-- A gift card redeemed against an order is money the shop already holds, so the
-- order it paid for is captured rather than waiting on a card. 0022 wrote this
-- function before there was anything to put in a credit line.
create or replace function tezgah_order_payment_status(p_scope uuid, p_order uuid)
returns void language plpgsql as $$
declare
    owed        numeric;
    authorized  numeric;
    captured    numeric;
    refunded    numeric;
    became      text;
begin
    select (s.totals->>'total')::numeric into owed
    from order_summary s
    where s.scope = p_scope and s.order_id = p_order
    order by s.version desc
    limit 1;
    owed := coalesce(owed, 0);

    select
        coalesce(sum(amount) filter (where reference = 'payment'), 0),
        coalesce(sum(amount) filter (where reference in ('capture', 'manual', 'credit_line')), 0),
        coalesce(-sum(amount) filter (where reference in ('refund', 'order_return',
                                                          'order_exchange', 'order_claim')), 0)
    into authorized, captured, refunded
    from order_transaction
    where scope = p_scope and order_id = p_order;

    became := case
        when refunded > 0 and refunded >= captured then 'refunded'
        when refunded > 0 then 'partially_refunded'
        when captured > 0 and captured >= owed then 'captured'
        when captured > 0 then 'partially_captured'
        when authorized > 0 and authorized >= owed then 'authorized'
        when authorized > 0 then 'partially_authorized'
        else 'not_paid'
    end;

    update "order"
    set payment_status = became
    where scope = p_scope and id = p_order and payment_status is distinct from became;
end
$$;

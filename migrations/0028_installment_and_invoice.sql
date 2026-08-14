set lock_timeout = '3s';
set statement_timeout = '60s';

-- What the basket came to and what the card was charged are two numbers, and
-- until now the schema had one. A card instalment plan ("taksit") adds a
-- bank-set difference — "vade farkı" in Turkey, `parcelamento` in Brazil — on
-- top of the order total, so the provider authorises more than the shop asked
-- for and the amount guard in `payment.rs` read a correct sale as fraud.
--
-- No instalment count, ceiling or sector rule is written down here on purpose:
-- which counts a card may be split into is set by a regulator and changes
-- several times a year, and a library that hard-codes today's table is wrong
-- by next quarter. What is stored is what the provider said it accepted.

alter table payment_session
    add column installment_count integer;

alter table payment
    add column installment_count integer,
    add column surcharge_amount numeric(20, 6) not null default 0,
    add column surcharge_bearer text,
    add column installment_campaign text;

-- Who pays for the plan: the shopper, on top of the basket, or the shop, out
-- of the settlement ("6 taksit, faizsiz"). The two move money in opposite
-- directions and only one of them changes what the card is charged.
alter table payment
    add constraint payment_surcharge_bearer_valid check (
        surcharge_bearer is null or surcharge_bearer in ('merchant', 'customer')
    );

alter table payment_collection
    add column installment_count integer,
    -- Customer-borne, so it is added to what the card is charged.
    add column surcharge_amount numeric(20, 6) not null default 0,
    -- Merchant-borne, so the card is charged the basket and the shop receives
    -- less. Recorded rather than added, which is why it is a second column.
    add column merchant_surcharge_amount numeric(20, 6) not null default 0,
    -- Null on every collection written before this migration; readers take
    -- `amount` when it is null rather than a backfill that forced row-level
    -- security would have silently matched nothing (see 0024).
    add column charged_amount numeric(20, 6);

create index payment_collection_installment_idx
    on payment_collection (scope, installment_count)
    where installment_count is not null;

-- The document a tax authority issued about this order. tezgah does not make
-- one — no UBL, no integrator, no PDF — it holds the reference so a second
-- request cannot produce a second invoice for one sale, and so a credit note
-- has something to reverse.
--
-- `external_id` is the identifier the authority assigns and `number` the
-- human-readable serial: in Turkey an e-Arşiv document carries an ETTN uuid
-- alongside its serial, and they are different things.
create table order_invoice (
    id                  uuid primary key,
    order_id            uuid not null,
    order_version       integer not null default 1,
    kind                text not null default 'invoice',
    number              text not null,
    external_id         text,
    provider            text,
    status              text not null default 'requested',
    issued_at           timestamptz,
    document_url        text,
    total_amount        numeric(20, 6) not null default 0,
    currency_code       char(3) not null,
    replaces_invoice_id uuid,
    metadata            jsonb,
    constraint order_invoice_kind_valid
        check (kind in ('invoice', 'credit_note')),
    constraint order_invoice_status_valid
        check (status in ('requested', 'issued', 'accepted', 'rejected', 'cancelled')),
    constraint order_invoice_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'),
    constraint order_invoice_total_amount_valid check (total_amount >= 0)
);
call tezgah_register('order_invoice');

call tezgah_fk('order_invoice', 'order_id', 'order', 'restrict', true);
call tezgah_fk('order_invoice', 'replaces_invoice_id', 'order_invoice', 'restrict', true);

-- The whole point: asking an integrator twice for the same document lands once.
create unique index order_invoice_number_key
    on order_invoice (scope, order_id, kind, number);
create unique index order_invoice_external_id_key
    on order_invoice (scope, external_id)
    where external_id is not null;
create index order_invoice_order_id_idx
    on order_invoice (scope, order_id, kind);

insert into tezgah_evidence_table (name) values ('order_invoice')
on conflict do nothing;

insert into tezgah_scoped_fk_table (name) values ('order_invoice')
on conflict do nothing;

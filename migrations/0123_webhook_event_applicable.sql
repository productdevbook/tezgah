set lock_timeout = '3s';
set statement_timeout = '60s';

-- A recorded callback could not be acted on later.
--
-- `record_webhook` was handed a `WebhookEvent` carrying what the provider
-- said — which session, which kind of thing happened, and for how much — and
-- stored only the raw payload beside the provider's own event id. So an event
-- that arrived and was not applied in the same breath could never be applied
-- at all: `unprocessed()` handed back rows nothing knew how to act on, and
-- reading the payload again would need the provider knowledge tezgah
-- deliberately does not have.
--
-- Nullable, because every row written before this one has no answer for them
-- and inventing one would be worse than admitting it. `apply_webhook` refuses
-- a row with no `kind`, and says why.
alter table payment_webhook_event
    add column kind               text,
    add column payment_session_id uuid,
    add column amount             numeric(20, 6),
    add column currency_code      text;

-- Composite, through `tezgah_fk`, and not a plain `references`. Postgres
-- checks a foreign key with row security bypassed, so a single-column key on
-- a scoped table would let one shop's callback point at another shop's
-- session — `tests/schema.rs` and `tests/isolation.rs` both said so, the
-- second by inserting one and watching it be accepted.
call tezgah_fk('payment_webhook_event', 'payment_session_id', 'payment_session', 'set null', true);

-- No index written here: `tezgah_fk` makes `(scope, payment_session_id)`
-- itself when nothing already covers the pair, which is exactly the index a
-- key with no index behind it would have wanted — and writing a second one
-- would take this table's lock for the build, which is what
-- `tests/migration_lock.rs` refuses in a migration that has not opted out of
-- its transaction.

-- The six the crate models. `other` is one of them on purpose: a provider
-- says a great many things, and "recorded, acknowledged, ignored" is an
-- answer rather than an omission.
alter table payment_webhook_event
    add constraint payment_webhook_event_kind_valid
    check (kind is null or kind in
        ('authorized', 'captured', 'refunded', 'canceled', 'failed', 'other'));

-- An amount needs a currency to mean anything, and neither is required.
alter table payment_webhook_event
    add constraint payment_webhook_event_amount_needs_currency
    check ((amount is null) = (currency_code is null));

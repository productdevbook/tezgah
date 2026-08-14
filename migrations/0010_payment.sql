set lock_timeout = '3s';
set statement_timeout = '60s';

-- A provider is named by a stable code the host's `PaymentProvider`
-- implementation answers to; the uuid is what the rest of the schema points at,
-- so renaming the code never orphans a payment.
create table payment_provider (
    id          uuid primary key,
    code        text not null,
    is_enabled  boolean not null default true,
    metadata    jsonb,
    constraint payment_provider_code_valid check (code <> '')
);
call tezgah_register('payment_provider');

create unique index payment_provider_code_key on payment_provider (scope, code);

-- What is owed, and the running record of what has been promised, taken and
-- given back against it. The three amounts are kept, not derived, because the
-- provider is the authority on them and its answer has to be storable even
-- when it disagrees with the sum of the rows here.
create table payment_collection (
    id                  uuid primary key,
    currency_code       char(3) not null,
    amount              numeric(20, 6) not null,
    authorized_amount   numeric(20, 6),
    captured_amount     numeric(20, 6),
    refunded_amount     numeric(20, 6),
    status              text not null default 'not_paid',
    completed_at        timestamptz,
    metadata            jsonb,
    constraint payment_collection_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'),
    constraint payment_collection_amount_valid check (amount >= 0),
    constraint payment_collection_authorized_amount_valid
        check (authorized_amount is null or authorized_amount >= 0),
    constraint payment_collection_captured_amount_valid
        check (captured_amount is null or captured_amount >= 0),
    constraint payment_collection_refunded_amount_valid
        check (refunded_amount is null or refunded_amount >= 0),
    constraint payment_collection_status_valid check (status in (
        'not_paid',
        'awaiting',
        'partially_authorized',
        'authorized',
        'partially_captured',
        'captured',
        'partially_refunded',
        'refunded',
        'canceled',
        'failed'
    ))
);
call tezgah_register('payment_collection');

create index payment_collection_status_idx on payment_collection (scope, status);
create index payment_collection_currency_code_idx
    on payment_collection (scope, currency_code);

-- One attempt at one provider. `data` is the provider's own state — an intent
-- id, a redirect url — opaque here on purpose: tezgah does not model Stripe.
create table payment_session (
    id                      uuid primary key,
    payment_collection_id   uuid not null
        references payment_collection (id) on delete cascade,
    payment_provider_id     uuid not null
        references payment_provider (id) on delete restrict,
    currency_code           char(3) not null,
    amount                  numeric(20, 6) not null,
    status                  text not null default 'pending',
    data                    jsonb not null default '{}'::jsonb,
    context                 jsonb,
    authorized_at           timestamptz,
    metadata                jsonb,
    constraint payment_session_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'),
    constraint payment_session_amount_valid check (amount >= 0),
    constraint payment_session_status_valid check (status in (
        'pending',
        'requires_more',
        'authorized',
        'captured',
        'canceled',
        'error'
    )),
    constraint payment_session_authorized_at_valid
        check (authorized_at is null or status in ('authorized', 'captured'))
);
call tezgah_register('payment_session');

create index payment_session_payment_collection_id_idx
    on payment_session (scope, payment_collection_id);
create index payment_session_payment_provider_id_idx
    on payment_session (scope, payment_provider_id);
create index payment_session_status_idx on payment_session (scope, status);

-- Money the provider has agreed to hold. `amount` is what was authorised;
-- what was actually taken and given back is the sum of `capture` and `refund`,
-- never a counter updated in place — a counter is what two writers race on.
create table payment (
    id                      uuid primary key,
    payment_collection_id   uuid not null
        references payment_collection (id) on delete restrict,
    payment_session_id      uuid
        references payment_session (id) on delete restrict,
    payment_provider_id     uuid not null
        references payment_provider (id) on delete restrict,
    currency_code           char(3) not null,
    amount                  numeric(20, 6) not null,
    data                    jsonb,
    captured_at             timestamptz,
    canceled_at             timestamptz,
    metadata                jsonb,
    constraint payment_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'),
    constraint payment_amount_valid check (amount > 0),
    constraint payment_canceled_valid
        check (canceled_at is null or captured_at is null)
);
call tezgah_register('payment');

create index payment_payment_collection_id_idx on payment (scope, payment_collection_id);
create index payment_payment_provider_id_idx on payment (scope, payment_provider_id);
create unique index payment_payment_session_key
    on payment (scope, payment_session_id)
    where payment_session_id is not null;

-- Capture and refund are rows rather than columns because both happen in parts:
-- three shipments off one authorisation are three captures, and the ledger has
-- to say which one was for what. Neither is ever deleted; both are accounting.
create table capture (
    id          uuid primary key,
    payment_id  uuid not null references payment (id) on delete restrict,
    amount      numeric(20, 6) not null,
    currency_code char(3) not null,
    created_by  text,
    metadata    jsonb,
    constraint capture_amount_valid check (amount > 0),
    constraint capture_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$')
);
call tezgah_register('capture');

create index capture_payment_id_idx on capture (scope, payment_id);

create table refund_reason (
    id          uuid primary key,
    code        text not null,
    label       text not null,
    description text,
    metadata    jsonb
);
call tezgah_register('refund_reason');

create unique index refund_reason_code_key on refund_reason (scope, code);

-- The reason may be retired while the refunds that cited it stay; losing the
-- reason must not lose the money, so the reference goes null rather than
-- taking the row with it.
create table refund (
    id                  uuid primary key,
    payment_id          uuid not null references payment (id) on delete restrict,
    refund_reason_id    uuid references refund_reason (id) on delete set null,
    amount              numeric(20, 6) not null,
    currency_code       char(3) not null,
    note                text,
    created_by          text,
    metadata            jsonb,
    constraint refund_amount_valid check (amount > 0),
    constraint refund_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$')
);
call tezgah_register('refund');

create index refund_payment_id_idx on refund (scope, payment_id);
create index refund_refund_reason_id_idx on refund (scope, refund_reason_id);

-- The customer as the provider knows them — a Stripe customer, an iyzico card
-- holder. `external_id` is theirs, and it is what makes a saved card reusable.
create table account_holder (
    id                  uuid primary key,
    payment_provider_id uuid not null
        references payment_provider (id) on delete restrict,
    customer_id         uuid,
    external_id         text not null,
    email               text,
    data                jsonb not null default '{}'::jsonb,
    metadata            jsonb
);
call tezgah_register('account_holder');

create index account_holder_payment_provider_id_idx
    on account_holder (scope, payment_provider_id);
create index account_holder_customer_id_idx
    on account_holder (scope, customer_id)
    where customer_id is not null;
create unique index account_holder_external_key
    on account_holder (scope, payment_provider_id, external_id);

-- Written before the event is acted on, never after. A provider retries until
-- it is acknowledged, so the same `event_id` arrives more than once; the unique
-- constraint is what turns the second arrival into a conflict instead of a
-- second capture. `processed_at` null means received and not yet acted on,
-- which is a state a crash can be resumed from.
create table payment_webhook_event (
    id                  uuid primary key,
    payment_provider_id uuid not null
        references payment_provider (id) on delete restrict,
    event_id            text not null,
    event_type          text not null,
    payload             jsonb not null,
    received_at         timestamptz not null default now(),
    processed_at        timestamptz,
    attempts            integer not null default 0,
    last_error          text,
    constraint payment_webhook_event_attempts_valid check (attempts >= 0)
);
call tezgah_register('payment_webhook_event');

create unique index payment_webhook_event_provider_event_key
    on payment_webhook_event (scope, payment_provider_id, event_id);
create index payment_webhook_event_unprocessed_idx
    on payment_webhook_event (scope, received_at)
    where processed_at is null;

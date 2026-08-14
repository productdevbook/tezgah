set lock_timeout = '3s';
set statement_timeout = '60s';

-- An address is copied onto the order, never referenced: the customer moving
-- house must not rewrite where a parcel was sent two years ago.
create table order_address (
    id              uuid primary key,
    customer_id     uuid,
    company         text,
    first_name      text,
    last_name       text,
    address_1       text,
    address_2       text,
    city            text,
    country_code    char(2),
    province        text,
    postal_code     text,
    phone           text,
    metadata        jsonb,
    constraint order_address_country_code_valid
        check (country_code is null
            or (country_code = upper(country_code) and country_code ~ '^[A-Z]{2}$'))
);
call tezgah_register('order_address');

create index order_address_customer_id_idx
    on order_address (scope, customer_id)
    where customer_id is not null;

-- `order` is a reserved word and stays the table's name anyway: it is the word
-- the whole domain is written in, every other table here is named after it, and
-- quoting it is a syntax error when forgotten rather than silent wrongness.
--
-- `version` is the order as a whole: an accepted edit writes version + 1 and
-- the rows describing the previous one are still there to be read.
create table "order" (
    id                      uuid primary key,
    display_id              bigint,
    region_id               uuid references region (id) on delete restrict,
    sales_channel_id        uuid references sales_channel (id) on delete restrict,
    customer_id             uuid,
    shipping_address_id     uuid references order_address (id) on delete restrict,
    billing_address_id      uuid references order_address (id) on delete restrict,
    payment_collection_id   uuid references payment_collection (id) on delete restrict,
    email                   text,
    currency_code           char(3) not null,
    locale                  text,
    version                 integer not null default 1,
    status                  text not null default 'pending',
    payment_status          text not null default 'not_paid',
    fulfillment_status      text not null default 'not_fulfilled',
    is_draft                boolean not null default false,
    no_notification         boolean,
    metadata                jsonb,
    completed_at            timestamptz,
    canceled_at             timestamptz,
    constraint order_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'),
    constraint order_version_valid check (version >= 1),
    constraint order_status_valid check (status in (
        'draft',
        'pending',
        'requires_action',
        'completed',
        'canceled',
        'archived'
    )),
    constraint order_payment_status_valid check (payment_status in (
        'not_paid',
        'awaiting',
        'authorized',
        'partially_authorized',
        'partially_captured',
        'captured',
        'partially_refunded',
        'refunded',
        'canceled',
        'requires_action'
    )),
    constraint order_fulfillment_status_valid check (fulfillment_status in (
        'not_fulfilled',
        'partially_fulfilled',
        'fulfilled',
        'partially_shipped',
        'shipped',
        'partially_delivered',
        'delivered',
        'partially_returned',
        'returned',
        'canceled'
    )),
    constraint order_canceled_valid
        check (canceled_at is null or status = 'canceled')
);
call tezgah_register('order');

create unique index order_display_id_key
    on "order" (scope, display_id)
    where display_id is not null;
create index order_customer_id_idx on "order" (scope, customer_id);
create index order_region_id_idx on "order" (scope, region_id);
create index order_sales_channel_id_idx on "order" (scope, sales_channel_id);
create index order_payment_collection_id_idx on "order" (scope, payment_collection_id);
create index order_shipping_address_id_idx on "order" (scope, shipping_address_id);
create index order_billing_address_id_idx on "order" (scope, billing_address_id);
create index order_status_idx on "order" (scope, status, created_at desc);

-- What was bought, as it read at the moment of buying. The variant may be
-- edited or withdrawn afterwards and this row must not change with it, so the
-- title, sku and options are copied and the reference is allowed to go null.
create table order_line_item (
    id                      uuid primary key,
    order_id                uuid not null references "order" (id) on delete restrict,
    variant_id              uuid references product_variant (id) on delete set null,
    product_id              uuid references product (id) on delete set null,
    title                   text not null,
    subtitle                text,
    thumbnail               text,
    product_title           text,
    product_handle          text,
    product_description     text,
    variant_title           text,
    variant_sku             text,
    variant_barcode         text,
    variant_option_values   jsonb,
    unit_price              numeric(20, 6) not null,
    compare_at_unit_price   numeric(20, 6),
    currency_code           char(3) not null,
    requires_shipping       boolean not null default true,
    is_tax_inclusive        boolean not null default false,
    is_discountable         boolean not null default true,
    is_custom_price         boolean not null default false,
    is_giftcard             boolean not null default false,
    metadata                jsonb,
    constraint order_line_item_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'),
    constraint order_line_item_unit_price_valid check (unit_price >= 0),
    constraint order_line_item_compare_at_unit_price_valid
        check (compare_at_unit_price is null or compare_at_unit_price >= 0)
);
call tezgah_register('order_line_item');

create index order_line_item_order_id_idx on order_line_item (scope, order_id);
create index order_line_item_variant_id_idx
    on order_line_item (scope, variant_id)
    where variant_id is not null;
create index order_line_item_product_id_idx
    on order_line_item (scope, product_id)
    where product_id is not null;

-- The bridge between an order version and a line item, and the only place a
-- quantity lives. One row per (order, version, item): editing an order writes a
-- fresh set of rows at the new version and touches none of the old ones, so
-- what the order looked like before the edit is still readable in full.
--
-- The quantity columns are a ledger of one item's journey — ordered, fulfilled,
-- shipped, delivered, asked back, received back, dismissed, written off — and
-- none of them may exceed what was ordered.
create table order_item (
    id                          uuid primary key,
    order_id                    uuid not null references "order" (id) on delete restrict,
    order_line_item_id          uuid not null
        references order_line_item (id) on delete restrict,
    version                     integer not null default 1,
    unit_price                  numeric(20, 6),
    compare_at_unit_price       numeric(20, 6),
    currency_code               char(3) not null,
    quantity                    integer not null,
    fulfilled_quantity          integer not null default 0,
    shipped_quantity            integer not null default 0,
    delivered_quantity          integer not null default 0,
    return_requested_quantity   integer not null default 0,
    return_received_quantity    integer not null default 0,
    return_dismissed_quantity   integer not null default 0,
    written_off_quantity        integer not null default 0,
    metadata                    jsonb,
    constraint order_item_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'),
    constraint order_item_version_valid check (version >= 1),
    constraint order_item_quantity_valid check (quantity > 0),
    constraint order_item_fulfilled_quantity_valid
        check (fulfilled_quantity between 0 and quantity),
    constraint order_item_shipped_quantity_valid
        check (shipped_quantity between 0 and fulfilled_quantity),
    constraint order_item_delivered_quantity_valid
        check (delivered_quantity between 0 and shipped_quantity),
    constraint order_item_return_requested_quantity_valid
        check (return_requested_quantity between 0 and quantity),
    constraint order_item_return_received_quantity_valid
        check (return_received_quantity between 0 and quantity),
    constraint order_item_return_dismissed_quantity_valid
        check (return_dismissed_quantity between 0 and return_received_quantity),
    constraint order_item_written_off_quantity_valid
        check (written_off_quantity between 0 and quantity)
);
call tezgah_register('order_item');

create index order_item_order_id_version_idx on order_item (scope, order_id, version);
create index order_item_order_line_item_id_idx on order_item (scope, order_line_item_id);
create unique index order_item_version_key
    on order_item (scope, order_id, version, order_line_item_id);

-- Versioned like the items: what shipping cost on version 1 stays readable
-- after version 2 changed the address and the price with it.
create table order_shipping_method (
    id                  uuid primary key,
    order_id            uuid not null references "order" (id) on delete restrict,
    version             integer not null default 1,
    name                text not null,
    description         text,
    shipping_option_id  uuid,
    amount              numeric(20, 6) not null,
    currency_code       char(3) not null,
    is_tax_inclusive    boolean not null default false,
    is_custom_amount    boolean not null default false,
    data                jsonb,
    metadata            jsonb,
    constraint order_shipping_method_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'),
    constraint order_shipping_method_amount_valid check (amount >= 0),
    constraint order_shipping_method_version_valid check (version >= 1)
);
call tezgah_register('order_shipping_method');

create index order_shipping_method_order_id_version_idx
    on order_shipping_method (scope, order_id, version);

-- The totals of one version of one order, computed once and written once. The
-- rule the rest of the domain leans on is that these totals are the sum of the
-- rows at the same (order_id, version) and nothing else; a summary is inserted
-- beside a version, never edited, so a disagreement is a bug that can be seen
-- rather than one that has already overwritten the evidence.
create table order_summary (
    id          uuid primary key,
    order_id    uuid not null references "order" (id) on delete restrict,
    version     integer not null default 1,
    currency_code char(3) not null,
    totals      jsonb not null,
    constraint order_summary_version_valid check (version >= 1),
    constraint order_summary_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$')
);
call tezgah_register('order_summary');

create unique index order_summary_order_id_version_key
    on order_summary (scope, order_id, version);

-- Every movement of money against the order, signed: a capture is positive, a
-- refund negative, and the order's payment status is the sum rather than a
-- field somebody remembered to set. `reference` names what caused the movement
-- and `reference_id` the row in whichever domain owns it, so there is no
-- foreign key here on purpose.
--
-- The unique index on (order_id, reference, reference_id) is the whole point:
-- writing the same capture twice fails instead of paying twice.
create table order_transaction (
    id              uuid primary key,
    order_id        uuid not null references "order" (id) on delete restrict,
    version         integer not null default 1,
    amount          numeric(20, 6) not null,
    currency_code   char(3) not null,
    reference       text,
    reference_id    uuid,
    metadata        jsonb,
    constraint order_transaction_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'),
    constraint order_transaction_amount_valid check (amount <> 0),
    constraint order_transaction_reference_valid check (
        reference is null or reference in (
            'capture',
            'refund',
            'payment',
            'credit_line',
            'order_return',
            'order_exchange',
            'order_claim',
            'manual'
        )
    ),
    constraint order_transaction_reference_id_valid
        check ((reference is null) = (reference_id is null))
);
call tezgah_register('order_transaction');

create index order_transaction_order_id_version_idx
    on order_transaction (scope, order_id, version);
create index order_transaction_reference_idx
    on order_transaction (scope, reference, reference_id)
    where reference is not null;
create unique index order_transaction_reference_key
    on order_transaction (scope, order_id, reference, reference_id)
    where reference is not null;

-- Money owed to or by the customer that is not a payment: store credit taken
-- off an exchange, a goodwill line on a claim. Kept apart from the ledger
-- because it settles differently, and versioned so an edit cannot rewrite it.
create table order_credit_line (
    id              uuid primary key,
    order_id        uuid not null references "order" (id) on delete restrict,
    version         integer not null default 1,
    amount          numeric(20, 6) not null,
    currency_code   char(3) not null,
    reference       text,
    reference_id    uuid,
    metadata        jsonb,
    constraint order_credit_line_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'),
    constraint order_credit_line_amount_valid check (amount <> 0),
    constraint order_credit_line_reference_id_valid
        check ((reference is null) = (reference_id is null))
);
call tezgah_register('order_credit_line');

create index order_credit_line_order_id_version_idx
    on order_credit_line (scope, order_id, version);

create table return_reason (
    id                      uuid primary key,
    parent_return_reason_id uuid references return_reason (id) on delete restrict,
    value                   text not null,
    label                   text not null,
    description             text,
    metadata                jsonb
);
call tezgah_register('return_reason');

create unique index return_reason_value_key on return_reason (scope, value);
create index return_reason_parent_return_reason_id_idx
    on return_reason (scope, parent_return_reason_id);

-- `order_return` rather than `return`: the word is reserved, and the prefix is
-- what its siblings `order_exchange` and `order_claim` already carry.
-- `order_version` records which version of the order was being returned from.
create table order_return (
    id              uuid primary key,
    order_id        uuid not null references "order" (id) on delete restrict,
    order_version   integer not null,
    display_id      bigint,
    status          text not null default 'open',
    location_id     uuid,
    refund_amount   numeric(20, 6),
    currency_code   char(3) not null,
    no_notification boolean,
    created_by      text,
    requested_at    timestamptz,
    received_at     timestamptz,
    canceled_at     timestamptz,
    metadata        jsonb,
    constraint order_return_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'),
    constraint order_return_refund_amount_valid
        check (refund_amount is null or refund_amount >= 0),
    constraint order_return_order_version_valid check (order_version >= 1),
    constraint order_return_status_valid check (status in (
        'requested',
        'open',
        'partially_received',
        'received',
        'canceled'
    ))
);
call tezgah_register('order_return');

create index order_return_order_id_idx on order_return (scope, order_id);
create index order_return_status_idx on order_return (scope, status);
create unique index order_return_display_id_key
    on order_return (scope, display_id)
    where display_id is not null;

create table order_return_item (
    id                  uuid primary key,
    order_return_id     uuid not null references order_return (id) on delete restrict,
    order_line_item_id  uuid not null references order_line_item (id) on delete restrict,
    return_reason_id    uuid references return_reason (id) on delete set null,
    quantity            integer not null,
    received_quantity   integer not null default 0,
    damaged_quantity    integer not null default 0,
    note                text,
    metadata            jsonb,
    constraint order_return_item_quantity_valid check (quantity > 0),
    constraint order_return_item_received_quantity_valid
        check (received_quantity between 0 and quantity),
    constraint order_return_item_damaged_quantity_valid
        check (damaged_quantity between 0 and received_quantity)
);
call tezgah_register('order_return_item');

create index order_return_item_order_return_id_idx
    on order_return_item (scope, order_return_id);
create index order_return_item_order_line_item_id_idx
    on order_return_item (scope, order_line_item_id);
create index order_return_item_return_reason_id_idx
    on order_return_item (scope, return_reason_id);

-- A return and the items going back out, settling as one: `difference_due` is
-- what is still owed either way once both halves are priced.
create table order_exchange (
    id              uuid primary key,
    order_id        uuid not null references "order" (id) on delete restrict,
    order_return_id uuid references order_return (id) on delete restrict,
    order_version   integer not null,
    display_id      bigint,
    difference_due  numeric(20, 6),
    currency_code   char(3) not null,
    allow_backorder boolean not null default false,
    no_notification boolean,
    created_by      text,
    canceled_at     timestamptz,
    metadata        jsonb,
    constraint order_exchange_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'),
    constraint order_exchange_order_version_valid check (order_version >= 1)
);
call tezgah_register('order_exchange');

create index order_exchange_order_id_idx on order_exchange (scope, order_id);
create index order_exchange_order_return_id_idx
    on order_exchange (scope, order_return_id);
create unique index order_exchange_display_id_key
    on order_exchange (scope, display_id)
    where display_id is not null;

create table order_exchange_item (
    id                  uuid primary key,
    order_exchange_id   uuid not null references order_exchange (id) on delete restrict,
    order_line_item_id  uuid not null references order_line_item (id) on delete restrict,
    quantity            integer not null,
    note                text,
    metadata            jsonb,
    constraint order_exchange_item_quantity_valid check (quantity > 0)
);
call tezgah_register('order_exchange_item');

create index order_exchange_item_order_exchange_id_idx
    on order_exchange_item (scope, order_exchange_id);
create index order_exchange_item_order_line_item_id_idx
    on order_exchange_item (scope, order_line_item_id);

-- Something arrived damaged or never arrived: replaced or refunded, and the
-- return that may come with it.
create table order_claim (
    id              uuid primary key,
    order_id        uuid not null references "order" (id) on delete restrict,
    order_return_id uuid references order_return (id) on delete restrict,
    order_version   integer not null,
    display_id      bigint,
    claim_type      text not null,
    refund_amount   numeric(20, 6),
    currency_code   char(3) not null,
    no_notification boolean,
    created_by      text,
    canceled_at     timestamptz,
    metadata        jsonb,
    constraint order_claim_currency_code_valid
        check (currency_code = upper(currency_code) and currency_code ~ '^[A-Z]{3}$'),
    constraint order_claim_order_version_valid check (order_version >= 1),
    constraint order_claim_refund_amount_valid
        check (refund_amount is null or refund_amount >= 0),
    constraint order_claim_claim_type_valid check (claim_type in ('refund', 'replace'))
);
call tezgah_register('order_claim');

create index order_claim_order_id_idx on order_claim (scope, order_id);
create index order_claim_order_return_id_idx on order_claim (scope, order_return_id);
create unique index order_claim_display_id_key
    on order_claim (scope, display_id)
    where display_id is not null;

-- `is_additional_item` says which side of the claim the row is on: false is
-- what went wrong, true is what is being sent instead.
create table order_claim_item (
    id                  uuid primary key,
    order_claim_id      uuid not null references order_claim (id) on delete restrict,
    order_line_item_id  uuid not null references order_line_item (id) on delete restrict,
    reason              text,
    quantity            integer not null,
    is_additional_item  boolean not null default false,
    images              jsonb,
    note                text,
    metadata            jsonb,
    constraint order_claim_item_quantity_valid check (quantity > 0),
    constraint order_claim_item_reason_valid check (
        reason is null or reason in (
            'missing_item',
            'wrong_item',
            'production_failure',
            'other'
        )
    )
);
call tezgah_register('order_claim_item');

create index order_claim_item_order_claim_id_idx
    on order_claim_item (scope, order_claim_id);
create index order_claim_item_order_line_item_id_idx
    on order_claim_item (scope, order_line_item_id);

-- One mechanism for every way an order changes after it is placed. A change is
-- requested, then confirmed or declined; only confirming applies its actions
-- and raises the order's version.
create table order_change (
    id                  uuid primary key,
    order_id            uuid not null references "order" (id) on delete restrict,
    order_return_id     uuid references order_return (id) on delete restrict,
    order_exchange_id   uuid references order_exchange (id) on delete restrict,
    order_claim_id      uuid references order_claim (id) on delete restrict,
    version             integer not null,
    change_type         text not null,
    status              text not null default 'pending',
    description         text,
    internal_note       text,
    created_by          text,
    requested_by        text,
    requested_at        timestamptz,
    confirmed_by        text,
    confirmed_at        timestamptz,
    declined_by         text,
    declined_at         timestamptz,
    declined_reason     text,
    canceled_by         text,
    canceled_at         timestamptz,
    no_notification     boolean,
    metadata            jsonb,
    constraint order_change_version_valid check (version >= 1),
    constraint order_change_change_type_valid
        check (change_type in ('edit', 'return', 'exchange', 'claim')),
    constraint order_change_status_valid
        check (status in ('pending', 'requested', 'confirmed', 'declined', 'canceled')),
    constraint order_change_confirmed_valid
        check ((status = 'confirmed') = (confirmed_at is not null)),
    constraint order_change_declined_valid
        check ((status = 'declined') = (declined_at is not null))
);
call tezgah_register('order_change');

create index order_change_order_id_version_idx on order_change (scope, order_id, version);
create index order_change_status_idx on order_change (scope, status);
create index order_change_order_return_id_idx on order_change (scope, order_return_id);
create index order_change_order_exchange_id_idx on order_change (scope, order_exchange_id);
create index order_change_order_claim_id_idx on order_change (scope, order_claim_id);
create unique index order_change_pending_key
    on order_change (scope, order_id)
    where status in ('pending', 'requested');

-- What the change actually does, one row per act, in `ordering`. `applied` is
-- set when the act has been carried into the order's next version, so replaying
-- a half-applied change picks up where it stopped rather than doing it twice.
create table order_change_action (
    id                  uuid primary key,
    order_change_id     uuid references order_change (id) on delete restrict,
    order_id            uuid not null references "order" (id) on delete restrict,
    order_return_id     uuid references order_return (id) on delete restrict,
    order_exchange_id   uuid references order_exchange (id) on delete restrict,
    order_claim_id      uuid references order_claim (id) on delete restrict,
    version             integer,
    ordering            integer not null,
    action              text not null,
    reference           text,
    reference_id        uuid,
    details             jsonb not null default '{}'::jsonb,
    amount              numeric(20, 6),
    currency_code       char(3),
    internal_note       text,
    applied             boolean not null default false,
    constraint order_change_action_version_valid check (version is null or version >= 1),
    constraint order_change_action_ordering_valid check (ordering >= 0),
    constraint order_change_action_currency_code_valid
        check ((amount is null) = (currency_code is null)),
    constraint order_change_action_reference_id_valid
        check ((reference is null) = (reference_id is null)),
    constraint order_change_action_action_valid check (action in (
        'ITEM_ADD',
        'ITEM_UPDATE',
        'ITEM_REMOVE',
        'RETURN_ITEM',
        'RECEIVE_RETURN_ITEM',
        'RECEIVE_DAMAGED_RETURN_ITEM',
        'DISMISS_RETURN_ITEM',
        'WRITE_OFF_ITEM',
        'SHIPPING_ADD',
        'SHIPPING_UPDATE',
        'SHIPPING_REMOVE',
        'FULFILL_ITEM',
        'SHIP_ITEM',
        'DELIVER_ITEM',
        'CANCEL_ITEM_FULFILLMENT',
        'CANCEL_RETURN_ITEM',
        'CREDIT_LINE_ADD',
        'TRANSFER_CUSTOMER',
        'UPDATE_ORDER_PROPERTIES'
    ))
);
call tezgah_register('order_change_action');

create index order_change_action_order_id_idx on order_change_action (scope, order_id);
create index order_change_action_order_return_id_idx
    on order_change_action (scope, order_return_id);
create index order_change_action_order_exchange_id_idx
    on order_change_action (scope, order_exchange_id);
create index order_change_action_order_claim_id_idx
    on order_change_action (scope, order_claim_id);
create index order_change_action_reference_idx
    on order_change_action (scope, reference, reference_id)
    where reference is not null;
create unique index order_change_action_ordering_key
    on order_change_action (scope, order_change_id, ordering)
    where order_change_id is not null;

set lock_timeout = '3s';
set statement_timeout = '120s';

-- `order_line_item.is_giftcard` existed and both construction sites wrote
-- `false`, so no line could ever say it was one and the rule hanging off it —
-- a gift card carries no tax, because the tax is due on what the card buys —
-- never fired. A shop selling a 50 card charged tax on the 50 and again on
-- what it bought.
--
-- The answer belongs to what is being sold, and the finest thing a line names
-- is a variant, the same place #111 put the withdrawal exclusion. False is the
-- ordinary case, which is what every row already here is.

alter table product_variant
    add column if not exists is_giftcard boolean not null default false;

-- Which line of which order printed this card, so a redelivered webhook prints
-- nothing the second time. One card per unit bought: three cards on a line of
-- three, and the ordinal is which of the three.
alter table gift_card
    add column if not exists issued_line_item_id uuid;
alter table gift_card
    add column if not exists issued_line_ordinal integer;

alter table gift_card
    drop constraint if exists gift_card_issued_line_valid;
alter table gift_card
    add constraint gift_card_issued_line_valid
        check ((issued_line_item_id is null) = (issued_line_ordinal is null)
               and (issued_line_ordinal is null or issued_line_ordinal >= 1));

create unique index if not exists gift_card_issued_line_key
    on gift_card (scope, issued_line_item_id, issued_line_ordinal)
    where issued_line_item_id is not null;

-- Restrict rather than set null: nulling the line would leave the ordinal
-- behind, and a card that forgot which line printed it could be printed again.
call tezgah_fk('gift_card', 'issued_line_item_id', 'order_line_item', 'restrict', true);

-- `order_change` is a proposal: a set of acts with an ordering, an `applied`
-- flag, and a status that can be confirmed, declined or canceled. A parcel
-- leaving the building is none of those things — it has happened, and no
-- version of the order can decline it. So these eight never had a writer and
-- never should have one: fulfilment is answered by `fulfillment` and its
-- `packed_at`, `shipped_at`, `delivered_at`, `canceled_at` and `created_by`,
-- and an order changing hands by `order_transfer` and its states.
alter table order_change_action
    drop constraint if exists order_change_action_action_valid;
alter table order_change_action
    add constraint order_change_action_action_valid check (action in (
        'ITEM_ADD',
        'ITEM_UPDATE',
        'ITEM_REMOVE',
        'RETURN_ITEM',
        'RECEIVE_RETURN_ITEM',
        'RECEIVE_DAMAGED_RETURN_ITEM',
        'DISMISS_RETURN_ITEM',
        'WRITE_OFF_ITEM',
        'SHIPPING_ADD',
        'SHIPPING_REMOVE',
        'CREDIT_LINE_ADD'
    ));

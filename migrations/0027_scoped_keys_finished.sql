set lock_timeout = '3s';
set statement_timeout = '60s';

-- `fulfillment_item.line_item_id` holds an `order_item` id, not an
-- `order_line_item` one: the column was named in 0012, before the order tables
-- existed, and `fulfilment.rs` has always joined it to `order_item`. 0026 read
-- the name and keyed it to the wrong parent.
alter table fulfillment_item
    drop constraint if exists fulfillment_item_line_item_id_fkey;

alter table fulfillment_item
    add constraint fulfillment_item_line_item_id_fkey
        foreign key (scope, line_item_id) references order_item (scope, id)
        on delete restrict not valid;

-- The keys left single-column on tables the schema test holds to a scoped key:
-- one scoped reference on a table is not enough, because the unscoped ones
-- beside it still cross the boundary.
call tezgah_fk('fulfillment_item', 'fulfillment_id', 'fulfillment', 'restrict', true);
call tezgah_fk('fulfillment_item', 'inventory_item_id', 'inventory_item', 'set null', true);
call tezgah_fk('payment', 'payment_provider_id', 'payment_provider', 'restrict', true);
call tezgah_fk('payment_session', 'payment_provider_id', 'payment_provider', 'restrict', true);
call tezgah_fk('promotion_usage', 'promotion_id', 'promotion', 'restrict', true);
call tezgah_fk('refund', 'refund_reason_id', 'refund_reason', 'set null', true);

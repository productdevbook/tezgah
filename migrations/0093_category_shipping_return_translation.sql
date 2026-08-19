set lock_timeout = '3s';
set statement_timeout = '60s';

-- #173: the three translated entities a shopper actually reads on the way to
-- checkout. Shaped exactly like `product_translation` (0005) — a real table
-- with a real foreign key per entity, not one polymorphic table keyed by
-- `(reference, reference_id, locale)`: that shape cannot carry a composite
-- scoped key, so nothing would stop a `reference_id` naming another tenant's
-- row (#91, #148).

create table product_category_translation (
    id           uuid primary key,
    category_id  uuid not null references product_category (id) on delete cascade,
    locale       text not null,
    name         text not null,
    description  text,
    constraint product_category_translation_locale_valid
        check (locale ~ '^[a-z]{2,3}(-[A-Za-z0-9]{2,8})*$')
);
call tezgah_register('product_category_translation');
create unique index product_category_translation_scope_category_locale_key
    on product_category_translation (scope, category_id, locale);
create index product_category_translation_scope_category_idx
    on product_category_translation (scope, category_id);

create table shipping_option_translation (
    id                  uuid primary key,
    shipping_option_id  uuid not null references shipping_option (id) on delete cascade,
    locale              text not null,
    name                text not null,
    constraint shipping_option_translation_locale_valid
        check (locale ~ '^[a-z]{2,3}(-[A-Za-z0-9]{2,8})*$')
);
call tezgah_register('shipping_option_translation');
create unique index shipping_option_translation_scope_option_locale_key
    on shipping_option_translation (scope, shipping_option_id, locale);
create index shipping_option_translation_scope_option_idx
    on shipping_option_translation (scope, shipping_option_id);

create table return_reason_translation (
    id                uuid primary key,
    return_reason_id  uuid not null references return_reason (id) on delete cascade,
    locale            text not null,
    label             text not null,
    description       text,
    constraint return_reason_translation_locale_valid
        check (locale ~ '^[a-z]{2,3}(-[A-Za-z0-9]{2,8})*$')
);
call tezgah_register('return_reason_translation');
create unique index return_reason_translation_scope_reason_locale_key
    on return_reason_translation (scope, return_reason_id, locale);
create index return_reason_translation_scope_reason_idx
    on return_reason_translation (scope, return_reason_id);

call tezgah_fk('product_category_translation', 'category_id', 'product_category', 'cascade', true);
call tezgah_fk('shipping_option_translation', 'shipping_option_id', 'shipping_option', 'cascade', true);
call tezgah_fk('return_reason_translation', 'return_reason_id', 'return_reason', 'cascade', true);

insert into tezgah_scoped_fk_table (name)
values ('product_category_translation'), ('shipping_option_translation'),
       ('return_reason_translation')
on conflict do nothing;

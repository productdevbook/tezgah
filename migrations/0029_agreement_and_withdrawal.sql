-- What the buyer was shown, that they accepted it, and when the right to walk
-- away expires. A distance seller carries the burden of proof for three years,
-- so the text itself is kept rather than a key into a template somebody may
-- edit tomorrow.

set lock_timeout = '3s';
set statement_timeout = '60s';

-- One rendering of one document, in one language, as it read the day it was
-- published. `body` is the whole text: a foreign key into an editable template
-- proves nothing, because editing the template destroys the evidence for every
-- order that ever pointed at it.
create table agreement_version (
    id              uuid primary key,
    kind            text not null,
    locale          text not null,
    body            text not null,
    body_hash       text not null,
    effective_from  timestamptz not null default now(),
    metadata        jsonb,
    constraint agreement_version_kind_valid
        check (kind in ('pre_contract', 'distance_sale', 'other'))
);
call tezgah_register('agreement_version');

create index agreement_version_kind_idx
    on agreement_version (scope, kind, locale, effective_from desc);
create index agreement_version_body_hash_idx on agreement_version (scope, body_hash);

-- Written once. An update that would change what the text said is refused by
-- the database rather than by whoever remembers the rule.
create or replace function tezgah_agreement_frozen() returns trigger
language plpgsql as $$
begin
    if tg_op = 'DELETE' then
        raise exception 'an agreement version is evidence and cannot be deleted';
    end if;

    if new.kind is distinct from old.kind
        or new.locale is distinct from old.locale
        or new.body is distinct from old.body
        or new.body_hash is distinct from old.body_hash
        or new.effective_from is distinct from old.effective_from
    then
        raise exception 'an agreement version is written once; publish another';
    end if;

    return new;
end
$$;

create trigger agreement_version_frozen
    before update or delete on agreement_version
    for each row execute function tezgah_agreement_frozen();

-- That this order's buyer accepted that version, and what the shop knew about
-- them at the moment they did. One acceptance per kind per order.
create table order_agreement (
    id                      uuid primary key,
    order_id                uuid not null,
    agreement_version_id    uuid not null,
    kind                    text not null,
    body_hash               text not null,
    accepted_at             timestamptz not null default now(),
    ip                      text,
    user_agent              text,
    metadata                jsonb,
    constraint order_agreement_kind_valid
        check (kind in ('pre_contract', 'distance_sale', 'other'))
);
call tezgah_register('order_agreement');

call tezgah_fk('order_agreement', 'order_id', 'order', 'restrict', true);
call tezgah_fk('order_agreement', 'agreement_version_id', 'agreement_version', 'restrict', true);

create unique index order_agreement_kind_key on order_agreement (scope, order_id, kind);

insert into tezgah_scoped_fk_table (name) values ('order_agreement') on conflict do nothing;
insert into tezgah_evidence_table (name) values
    ('order_agreement'),
    ('agreement_version')
on conflict do nothing;

-- Whether this line could be walked away from, decided when it was bought.
-- The exemptions move — telephones, tablets and computers came back inside the
-- Turkish list on 1 January 2026 — so what mattered is what the rule was that
-- day, not what deriving it today would say.
alter table order_line_item
    add column if not exists withdrawal_eligible boolean not null default true,
    add column if not exists withdrawal_exclusion_reason text;

alter table order_line_item
    drop constraint if exists order_line_item_withdrawal_exclusion_reason_valid;
alter table order_line_item
    add constraint order_line_item_withdrawal_exclusion_reason_valid
        check (withdrawal_exclusion_reason is null or withdrawal_exclusion_reason in (
            'custom_made',
            'hygiene',
            'perishable',
            'digital_unsealed',
            'digital_delivered',
            'periodical',
            'service_started',
            'other'
        ));

-- The withdrawal half of a return: when the buyer said so, when the goods came
-- back, and by when the money is owed. The deadline to *open* one is not here:
-- it follows from the delivery, which can still move, and a stored copy would
-- be the one thing that did not.
alter table order_return
    add column if not exists notified_at timestamptz,
    add column if not exists goods_returned_at timestamptz,
    add column if not exists refund_due_by timestamptz;

create index if not exists order_return_notified_at_idx
    on order_return (scope, notified_at)
    where notified_at is not null;

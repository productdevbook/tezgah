set lock_timeout = '3s';
set statement_timeout = '60s';

-- 0053 left this out because its own seeder could not satisfy it: two
-- columns naming the same parent table got the same seeded row. The seeder
-- now seeds a second row of that parent for the second reference instead
-- (`tests/isolation.rs`), so the constraint belongs here, not only in
-- `pricing::add_bundle_component`. A row already sitting on the wrong side
-- of it is a bundle listing itself as its own component, which has no valid
-- reading, so it is removed rather than kept for a validate step to reject
-- forever.
do $$
declare
    s uuid;
begin
    for s in select id from tezgah_scope loop
        perform set_config('app.scope', s::text, true);

        delete from product_bundle_component
         where bundle_variant_id = component_variant_id;
    end loop;

    perform set_config('app.scope', '', true);
end
$$;

alter table product_bundle_component
    add constraint product_bundle_component_not_self
    check (bundle_variant_id <> component_variant_id) not valid;

alter table product_bundle_component
    validate constraint product_bundle_component_not_self;

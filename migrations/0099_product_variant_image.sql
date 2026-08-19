set lock_timeout = '3s';
set statement_timeout = '60s';

-- #174: `product_image` only ever names a product, so a product sold in
-- several colours has one shared gallery and picking a variant cannot change
-- what is shown. A pivot, not a nullable `variant_id` on `product_image`: an
-- image can be one variant's alone or several's at once — a "front view"
-- shot worn by both the red and the blue variant is one row here twice, not
-- a second copy of the image.

create table product_variant_image (
    id          uuid primary key,
    variant_id  uuid not null references product_variant (id) on delete cascade,
    image_id    uuid not null references product_image (id) on delete cascade
);
call tezgah_register('product_variant_image');
create unique index product_variant_image_scope_variant_image_key
    on product_variant_image (scope, variant_id, image_id);
create index product_variant_image_scope_image_idx
    on product_variant_image (scope, image_id);

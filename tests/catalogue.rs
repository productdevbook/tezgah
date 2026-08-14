//! The catalogue against a real Postgres: what it stores, what it refuses, and
//! what it will not show somebody else.

mod common;

use common::Shop;
use tezgah::catalogue::{
    self, Combination, NewCategory, NewProduct, NewVariant, ProductFilter, ProductStatus,
    ProductTranslation, VariantPlan,
};
use tezgah::id::{CategoryId, ProductId};
use tezgah::page::Paging;

fn draft(handle: &str, title: &str) -> NewProduct {
    NewProduct {
        handle: handle.into(),
        title: title.into(),
        ..NewProduct::default()
    }
}

#[tokio::test]
async fn a_product_is_written_read_back_and_published() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let made = catalogue::create_product(&mut tx, &ctx, draft("kilim", "A kilim"))
        .await
        .expect("a product");
    assert_eq!(made.status, ProductStatus::Draft);
    assert!(shop.host.audited("product"));

    let read = catalogue::product(&mut tx, &ctx, made.id)
        .await
        .expect("to read it back");
    assert_eq!(read.title, "A kilim");

    let drafts = catalogue::products(
        &mut tx,
        &ctx,
        ProductFilter {
            status: Some(ProductStatus::Published),
            ..ProductFilter::default()
        },
        Paging::first(10),
    )
    .await
    .expect("to list");
    assert!(drafts.is_empty(), "a draft is not published");

    let published = catalogue::publish_product(&mut tx, &ctx, made.id)
        .await
        .expect("to publish");
    assert_eq!(published.status, ProductStatus::Published);
    assert!(shop.host.emitted("product.published"));

    let listed = catalogue::products(
        &mut tx,
        &ctx,
        ProductFilter {
            status: Some(ProductStatus::Published),
            ..ProductFilter::default()
        },
        Paging::first(10),
    )
    .await
    .expect("to list");
    assert_eq!(listed.len(), 1);

    catalogue::delete_product(&mut tx, &ctx, made.id)
        .await
        .expect("to delete");
    assert!(
        catalogue::product(&mut tx, &ctx, made.id)
            .await
            .expect_err("a deleted product is gone")
            .is_not_found()
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_product_without_a_title_is_refused() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let err = catalogue::create_product(&mut tx, &ctx, draft("kilim", "   "))
        .await
        .expect_err("an empty title is refused");
    assert_eq!(err.code(), "invalid");

    let err = catalogue::create_product(&mut tx, &ctx, draft("a handle", "A kilim"))
        .await
        .expect_err("a handle with a space in it is refused");
    assert_eq!(err.code(), "invalid");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_handle_is_taken_only_once_per_scope() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    catalogue::create_product(&mut tx, &ctx, draft("kilim", "A kilim"))
        .await
        .expect("the first");
    let err = catalogue::create_product(&mut tx, &ctx, draft("kilim", "Another kilim"))
        .await
        .expect_err("the second is refused");
    assert!(err.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn somebody_elses_scope_sees_nothing_of_it() {
    let shop = Shop::open().await;

    let mut tx = shop.begin().await;
    let made = catalogue::create_product(&mut tx, &shop.ctx(), draft("kilim", "A kilim"))
        .await
        .expect("a product");
    tx.commit().await.expect("to commit");

    let theirs = shop.theirs();
    let mut tx = shop.begin_as(shop.elsewhere).await;

    assert!(
        catalogue::product(&mut tx, &theirs, made.id)
            .await
            .expect_err("not theirs")
            .is_not_found()
    );

    let page = catalogue::products(
        &mut tx,
        &theirs,
        ProductFilter::default(),
        Paging::first(10),
    )
    .await
    .expect("to list");
    assert!(page.is_empty(), "another scope's list is empty");

    // The same handle is free in another shop.
    catalogue::create_product(&mut tx, &theirs, draft("kilim", "Their kilim"))
        .await
        .expect("a handle is unique per scope, not per cluster");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_product_that_is_not_there_is_not_found() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let err = catalogue::product(&mut tx, &ctx, ProductId::new())
        .await
        .expect_err("nothing to find");
    assert!(err.is_not_found());
    assert_eq!(err.code(), "not_found");

    let err = catalogue::category(&mut tx, &ctx, CategoryId::new())
        .await
        .expect_err("nothing to find");
    assert!(err.is_not_found());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_sku_belongs_to_one_variant() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let product = catalogue::create_product(&mut tx, &ctx, draft("kilim", "A kilim"))
        .await
        .expect("a product");

    catalogue::create_variant(
        &mut tx,
        &ctx,
        product.id,
        NewVariant {
            title: "Small".into(),
            sku: Some("KIL-S".into()),
            ..NewVariant::default()
        },
    )
    .await
    .expect("the first");

    let err = catalogue::create_variant(
        &mut tx,
        &ctx,
        product.id,
        NewVariant {
            title: "Large".into(),
            sku: Some("KIL-S".into()),
            ..NewVariant::default()
        },
    )
    .await
    .expect_err("the same sku twice is refused");
    assert!(err.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn variants_are_generated_from_the_options_that_are_sold() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let product = catalogue::create_product(&mut tx, &ctx, draft("kilim", "A kilim"))
        .await
        .expect("a product");

    let size = catalogue::add_option(&mut tx, &ctx, product.id, "Size", 0)
        .await
        .expect("an option");
    let colour = catalogue::add_option(&mut tx, &ctx, product.id, "Colour", 1)
        .await
        .expect("an option");

    let small = catalogue::add_option_value(&mut tx, &ctx, size.id, "Small", 0)
        .await
        .expect("a value");
    let large = catalogue::add_option_value(&mut tx, &ctx, size.id, "Large", 1)
        .await
        .expect("a value");
    let red = catalogue::add_option_value(&mut tx, &ctx, colour.id, "Red", 0)
        .await
        .expect("a value");
    let blue = catalogue::add_option_value(&mut tx, &ctx, colour.id, "Blue", 1)
        .await
        .expect("a value");

    let made = catalogue::generate_variants(
        &mut tx,
        &ctx,
        product.id,
        VariantPlan {
            exclude: vec![Combination(vec![large.id, blue.id])],
            ..VariantPlan::default()
        },
    )
    .await
    .expect("to generate");

    assert_eq!(made.len(), 3, "four combinations less the one not sold");
    assert!(made.iter().all(|v| v.title.contains(" / ")));

    let one = made.first().expect("a variant");
    let chosen = catalogue::variant_options(&mut tx, &ctx, one.id)
        .await
        .expect("to read the combination");
    assert_eq!(chosen.len(), 2, "one value per option");

    // A second run has nothing left to make.
    let again = catalogue::generate_variants(
        &mut tx,
        &ctx,
        product.id,
        VariantPlan {
            exclude: vec![Combination(vec![large.id, blue.id])],
            ..VariantPlan::default()
        },
    )
    .await
    .expect("to generate");
    assert!(again.is_empty(), "what exists is not made twice");

    // Setting a combination by hand needs every axis.
    let spare = catalogue::create_variant(
        &mut tx,
        &ctx,
        product.id,
        NewVariant {
            title: "Large / Blue".into(),
            ..NewVariant::default()
        },
    )
    .await
    .expect("a variant");

    let err = catalogue::set_variant_options(&mut tx, &ctx, spare.id, &[large.id])
        .await
        .expect_err("half a combination is refused");
    assert_eq!(err.code(), "invalid");

    catalogue::set_variant_options(&mut tx, &ctx, spare.id, &[large.id, blue.id])
        .await
        .expect("the whole combination is accepted");

    let listed = catalogue::variants(&mut tx, &ctx, product.id, Paging::first(50))
        .await
        .expect("to list");
    assert_eq!(listed.len(), 4);

    let _ = (small, red);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_category_cannot_be_moved_inside_itself() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let rugs = catalogue::create_category(
        &mut tx,
        &ctx,
        NewCategory {
            name: "Rugs".into(),
            handle: "rugs".into(),
            ..NewCategory::default()
        },
    )
    .await
    .expect("a root");

    let kilims = catalogue::create_category(
        &mut tx,
        &ctx,
        NewCategory {
            parent_id: Some(rugs.id),
            name: "Kilims".into(),
            handle: "kilims".into(),
            ..NewCategory::default()
        },
    )
    .await
    .expect("a child");

    let small = catalogue::create_category(
        &mut tx,
        &ctx,
        NewCategory {
            parent_id: Some(kilims.id),
            name: "Small kilims".into(),
            handle: "small-kilims".into(),
            ..NewCategory::default()
        },
    )
    .await
    .expect("a grandchild");

    assert_eq!(small.depth(), 2);
    assert!(small.mpath.starts_with(&rugs.mpath));

    let err = catalogue::move_category(&mut tx, &ctx, rugs.id, Some(small.id))
        .await
        .expect_err("a cycle is refused");
    assert_eq!(err.code(), "invalid");

    let err = catalogue::move_category(&mut tx, &ctx, rugs.id, Some(rugs.id))
        .await
        .expect_err("its own parent is refused");
    assert_eq!(err.code(), "invalid");

    let subtree = catalogue::category_subtree(&mut tx, &ctx, rugs.id, Paging::first(50))
        .await
        .expect("to read the subtree");
    assert_eq!(subtree.len(), 3, "the root counts itself");

    // Moving the other way takes the descendants with it.
    catalogue::move_category(&mut tx, &ctx, kilims.id, None)
        .await
        .expect("to move to the top");
    let moved = catalogue::category(&mut tx, &ctx, small.id)
        .await
        .expect("still there");
    assert_eq!(moved.depth(), 1, "the grandchild followed its parent up");

    let err = catalogue::delete_category(&mut tx, &ctx, kilims.id)
        .await
        .expect_err("a category with children is kept");
    assert!(err.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_product_is_filed_and_found_by_where_it_is_filed() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let collection = catalogue::create_collection(&mut tx, &ctx, "Summer", "summer")
        .await
        .expect("a collection");
    let tag = catalogue::create_tag(&mut tx, &ctx, "handmade")
        .await
        .expect("a tag");
    let kind = catalogue::create_type(&mut tx, &ctx, "rug")
        .await
        .expect("a type");

    let rugs = catalogue::create_category(
        &mut tx,
        &ctx,
        NewCategory {
            name: "Rugs".into(),
            handle: "rugs".into(),
            ..NewCategory::default()
        },
    )
    .await
    .expect("a category");
    let kilims = catalogue::create_category(
        &mut tx,
        &ctx,
        NewCategory {
            parent_id: Some(rugs.id),
            name: "Kilims".into(),
            handle: "kilims".into(),
            ..NewCategory::default()
        },
    )
    .await
    .expect("a child category");

    let product = catalogue::create_product(
        &mut tx,
        &ctx,
        NewProduct {
            handle: "kilim".into(),
            title: "A kilim".into(),
            product_collection_id: Some(collection.id),
            product_type_id: Some(kind.id),
            ..NewProduct::default()
        },
    )
    .await
    .expect("a product");

    catalogue::tag_product(&mut tx, &ctx, product.id, tag.id)
        .await
        .expect("to tag it");
    catalogue::add_product_to_category(&mut tx, &ctx, product.id, kilims.id)
        .await
        .expect("to file it");

    for filter in [
        ProductFilter {
            collection: Some(collection.id),
            ..ProductFilter::default()
        },
        ProductFilter {
            product_type: Some(kind.id),
            ..ProductFilter::default()
        },
        ProductFilter {
            tag: Some(tag.id),
            ..ProductFilter::default()
        },
        ProductFilter {
            // The parent finds what is filed under its child.
            category: Some(rugs.id),
            ..ProductFilter::default()
        },
    ] {
        let page = catalogue::products(&mut tx, &ctx, filter, Paging::first(10))
            .await
            .expect("to list");
        assert_eq!(page.len(), 1);
    }

    assert_eq!(
        catalogue::product_tags(&mut tx, &ctx, product.id)
            .await
            .expect("its tags")
            .len(),
        1
    );
    assert_eq!(
        catalogue::product_categories(&mut tx, &ctx, product.id)
            .await
            .expect("its categories")
            .len(),
        1
    );

    catalogue::untag_product(&mut tx, &ctx, product.id, tag.id)
        .await
        .expect("to untag");
    assert!(
        catalogue::product_tags(&mut tx, &ctx, product.id)
            .await
            .expect("its tags")
            .is_empty()
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_locale_with_nothing_written_for_it_falls_back() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let product = catalogue::create_product(&mut tx, &ctx, draft("kilim", "A kilim"))
        .await
        .expect("a product");

    let fallen_back = catalogue::localised(&mut tx, &ctx, product.id, "tr")
        .await
        .expect("a reading");
    assert!(fallen_back.is_fallback);
    assert_eq!(fallen_back.title, "A kilim");
    assert_eq!(fallen_back.handle, "kilim");

    catalogue::put_translation(
        &mut tx,
        &ctx,
        product.id,
        ProductTranslation {
            product_id: product.id,
            locale: "tr".into(),
            title: "Bir kilim".into(),
            subtitle: None,
            description: None,
            handle: Some("bir-kilim".into()),
        },
    )
    .await
    .expect("a translation");

    let read = catalogue::localised(&mut tx, &ctx, product.id, "tr")
        .await
        .expect("a reading");
    assert!(!read.is_fallback);
    assert_eq!(read.title, "Bir kilim");
    assert_eq!(read.handle, "bir-kilim");

    // A region falls back to its bare language before it falls back to the row.
    let region = catalogue::localised(&mut tx, &ctx, product.id, "tr-TR")
        .await
        .expect("a reading");
    assert!(!region.is_fallback);
    assert_eq!(region.title, "Bir kilim");

    // Writing the same locale again replaces it rather than making a second.
    catalogue::put_translation(
        &mut tx,
        &ctx,
        product.id,
        ProductTranslation {
            product_id: product.id,
            locale: "tr".into(),
            title: "Bir başka kilim".into(),
            subtitle: None,
            description: None,
            handle: None,
        },
    )
    .await
    .expect("a translation");
    assert_eq!(
        catalogue::translations(&mut tx, &ctx, product.id)
            .await
            .expect("its translations")
            .len(),
        1
    );

    let err = catalogue::localised(&mut tx, &ctx, product.id, "not a locale")
        .await
        .expect_err("a locale that is not one is refused");
    assert_eq!(err.code(), "invalid");

    catalogue::remove_translation(&mut tx, &ctx, product.id, "tr")
        .await
        .expect("to remove it");
    assert!(
        catalogue::localised(&mut tx, &ctx, product.id, "tr")
            .await
            .expect("a reading")
            .is_fallback
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// Postgres aborts the whole transaction on a constraint violation, so a
/// conflict that was caught rather than decided leaves the caller with nothing
/// it can run afterwards — not even a second, different name.
#[tokio::test]
async fn a_duplicate_handle_is_refused_without_killing_the_transaction() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    catalogue::create_product(&mut tx, &ctx, draft("kilim", "A kilim"))
        .await
        .expect("a product");

    let refused = catalogue::create_product(&mut tx, &ctx, draft("kilim", "Another kilim"))
        .await
        .expect_err("one product per handle");
    assert!(refused.is_conflict());

    let offered = catalogue::create_product(&mut tx, &ctx, draft("kilim-2", "Another kilim"))
        .await
        .expect("the transaction to still take a different handle");
    assert_eq!(offered.handle, "kilim-2");

    let counted: i64 = sqlx::query_scalar("select count(*) from product where scope = $1")
        .bind(shop.here.0)
        .fetch_one(&mut *tx)
        .await
        .expect("the transaction to still be usable");
    assert_eq!(counted, 2);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// The same for a guarded update: the handle it was handed is already a
/// sibling's, and the refusal has to come from the statement rather than from
/// the index raising.
#[tokio::test]
async fn renaming_onto_a_taken_handle_is_refused_without_killing_the_transaction() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    catalogue::create_product(&mut tx, &ctx, draft("kilim", "A kilim"))
        .await
        .expect("a product");
    let other = catalogue::create_product(&mut tx, &ctx, draft("cicim", "A cicim"))
        .await
        .expect("another product");

    let refused = catalogue::update_product(
        &mut tx,
        &ctx,
        other.id,
        tezgah::catalogue::ProductPatch {
            handle: Some("kilim".into()),
            ..Default::default()
        },
    )
    .await
    .expect_err("that handle belongs to the other one");
    assert!(refused.is_conflict());

    let renamed = catalogue::update_product(
        &mut tx,
        &ctx,
        other.id,
        tezgah::catalogue::ProductPatch {
            handle: Some("cicim-2".into()),
            ..Default::default()
        },
    )
    .await
    .expect("the transaction to still take a free handle");
    assert_eq!(renamed.handle, "cicim-2");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

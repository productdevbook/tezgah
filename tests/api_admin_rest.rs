//! The back office's settings surface, against a real Postgres.
//!
//! What is worth asserting here is what the surface itself decides: the shop's
//! own row is readable and editable, a run's steps and its dead letters are
//! readable a page at a time, a promotion's rules can be added, listed and
//! taken away again — and none of it crosses a scope or survives a host that
//! refuses.

mod common;

use common::{Doorman, Shop};
use tezgah::api::admin_rest as admin;
use tezgah::id::PromotionId;
use tezgah::ports::{Actor, Ctx, Scope, Tx};
use uuid::Uuid;

async fn seed_store(tx: &mut Tx<'_>, scope: Scope, name: &str) {
    sqlx::query(
        "insert into store (id, scope, name, default_currency_code,
                            supported_currency_codes, supported_locales)
         values ($1, $2, $3, 'TRY', array['TRY']::char(3)[], array['tr'])",
    )
    .bind(Uuid::now_v7())
    .bind(scope.0)
    .bind(name)
    .execute(&mut **tx)
    .await
    .expect("a shop");
}

async fn seed_run(tx: &mut Tx<'_>, scope: Scope, key: &str, state: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into workflow_run (id, scope, name, transaction_key, state)
         values ($1, $2, 'checkout', $3, $4)",
    )
    .bind(id)
    .bind(scope.0)
    .bind(key)
    .bind(state)
    .execute(&mut **tx)
    .await
    .expect("a run");
    id
}

async fn promotion(tx: &mut Tx<'_>, ctx: &Ctx<'_>, code: &str) -> PromotionId {
    admin::create_promotion(
        tx,
        ctx,
        admin::CreatePromotion {
            code: code.into(),
            kind: tezgah::promotion::PromotionKind::Standard,
            status: tezgah::promotion::Status::Draft,
            is_automatic: false,
            campaign_id: None,
            usage_limit: None,
            customer_usage_limit: None,
        },
    )
    .await
    .expect("a promotion")
    .id
}

#[tokio::test]
async fn the_shop_reads_and_edits_its_own_settings() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_store(&mut tx, shop.here, "A shop").await;

    let read = admin::get_store(&mut tx, &ctx).await.expect("the shop");
    assert_eq!(read.name, "A shop");
    assert_eq!(read.default_currency_code, "TRY");
    assert_eq!(read.supported_locales, vec!["tr".to_owned()]);

    let edited = admin::update_store(
        &mut tx,
        &ctx,
        admin::UpdateStore {
            name: Some("A better shop".into()),
            supported_currency_codes: Some(vec!["TRY".into(), "EUR".into()]),
            ..admin::UpdateStore::default()
        },
    )
    .await
    .expect("to edit");

    assert_eq!(edited.name, "A better shop");
    assert_eq!(edited.supported_currency_codes.len(), 2);
    assert_eq!(
        edited.supported_locales,
        vec!["tr".to_owned()],
        "a field nobody sent is left alone"
    );

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn locales_are_read_back_as_they_were_set() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_store(&mut tx, shop.here, "A shop").await;

    let set = admin::set_locales(
        &mut tx,
        &ctx,
        admin::SetLocales {
            supported_locales: vec!["tr".into(), "en".into()],
        },
    )
    .await
    .expect("to set the locales");
    assert_eq!(set, vec!["tr".to_owned(), "en".to_owned()]);

    let read = admin::list_locales(&mut tx, &ctx)
        .await
        .expect("the locales");
    assert_eq!(read, set);

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn a_shop_is_not_visible_from_another_scope() {
    let shop = Shop::open().await;

    let mut ours = shop.begin().await;
    seed_store(&mut ours, shop.here, "Ours").await;
    ours.commit().await.expect("to commit");

    let theirs = shop.theirs();
    let mut tx = shop.begin_as(shop.elsewhere).await;

    let refused = admin::get_store(&mut tx, &theirs).await;
    assert!(
        refused.is_err_and(|err| err.is_not_found()),
        "another scope does not admit our shop exists"
    );

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn workflow_runs_are_listed_a_page_at_a_time_and_narrowed_by_state() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut other = shop.begin_as(shop.elsewhere).await;
    seed_run(&mut other, shop.elsewhere, "theirs", "done").await;
    other.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    seed_run(&mut tx, shop.here, "one", "done").await;
    seed_run(&mut tx, shop.here, "two", "done").await;
    seed_run(&mut tx, shop.here, "three", "failed").await;

    let first = admin::list_workflow_runs(
        &mut tx,
        &ctx,
        admin::ListWorkflowRuns {
            limit: Some(2),
            ..admin::ListWorkflowRuns::default()
        },
    )
    .await
    .expect("to list");

    assert_eq!(first.len(), 2);
    let next = first.next.clone().expect("a second page");

    let second = admin::list_workflow_runs(
        &mut tx,
        &ctx,
        admin::ListWorkflowRuns {
            after: Some(next),
            limit: Some(2),
            ..admin::ListWorkflowRuns::default()
        },
    )
    .await
    .expect("to list");

    assert_eq!(second.len(), 1, "three of ours and none of theirs");
    assert!(second.next.is_none());

    let failed = admin::list_workflow_runs(
        &mut tx,
        &ctx,
        admin::ListWorkflowRuns {
            state: Some("failed".into()),
            ..admin::ListWorkflowRuns::default()
        },
    )
    .await
    .expect("to list");
    assert_eq!(failed.len(), 1);
    assert_eq!(failed.items[0].state, "failed");

    let nonsense = admin::list_workflow_runs(
        &mut tx,
        &ctx,
        admin::ListWorkflowRuns {
            state: Some("sideways".into()),
            ..admin::ListWorkflowRuns::default()
        },
    )
    .await;
    assert!(nonsense.is_err(), "a state nothing writes is refused");

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn a_runs_steps_and_its_dead_letters_are_readable() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut other = shop.begin_as(shop.elsewhere).await;
    let elsewhere = seed_run(&mut other, shop.elsewhere, "theirs", "failed").await;
    sqlx::query(
        "insert into workflow_dead_letter (id, scope, run_id, step_name, failure, state)
         values ($1, $2, $3, 'charge', 'the refund was refused', '{}'::jsonb)",
    )
    .bind(Uuid::now_v7())
    .bind(shop.elsewhere.0)
    .bind(elsewhere)
    .execute(&mut *other)
    .await
    .expect("their dead letter");
    other.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let run = seed_run(&mut tx, shop.here, "one", "failed").await;

    for (at, name) in ["reserve", "charge"].into_iter().enumerate() {
        sqlx::query(
            "insert into workflow_step (id, scope, run_id, name, ordering, group_ordering)
             values ($1, $2, $3, $4, $5, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(shop.here.0)
        .bind(run)
        .bind(name)
        .bind(at as i32)
        .execute(&mut *tx)
        .await
        .expect("a step");
    }

    sqlx::query(
        "insert into workflow_dead_letter (id, scope, run_id, step_name, failure, state)
         values ($1, $2, $3, 'charge', 'the refund was refused', '{}'::jsonb)",
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .bind(run)
    .execute(&mut *tx)
    .await
    .expect("a dead letter");

    let steps =
        admin::list_workflow_run_steps(&mut tx, &ctx, tezgah::id::WorkflowRunId::from_uuid(run))
            .await
            .expect("the steps");
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].name, "reserve");
    assert_eq!(steps[1].ordering, 1);

    let dead = admin::list_workflow_dead_letters(&mut tx, &ctx, admin::List::default())
        .await
        .expect("the dead letters");
    assert_eq!(dead.len(), 1, "ours, and not the other scope's");
    assert_eq!(dead.items[0].step_name, "charge");

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn a_promotion_rule_is_added_listed_and_taken_away() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let id = promotion(&mut tx, &ctx, "WELCOME").await;

    let made = admin::add_promotion_rule(
        &mut tx,
        &ctx,
        id,
        "rules",
        admin::AddRule {
            attribute: "customer.email".into(),
            operator: tezgah::promotion::Operator::Eq,
            allowed_values: vec!["someone@example.com".into()],
            description: None,
        },
    )
    .await
    .expect("a rule");

    let listed = admin::list_promotion_rules(&mut tx, &ctx, id, "rules")
        .await
        .expect("to list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, made.id);

    admin::delete_promotion_rule(&mut tx, &ctx, id, "rules", made.id)
        .await
        .expect("to remove it");

    let after = admin::list_promotion_rules(&mut tx, &ctx, id, "rules")
        .await
        .expect("to list");
    assert!(after.is_empty());

    let again = admin::delete_promotion_rule(&mut tx, &ctx, id, "rules", made.id).await;
    assert!(
        again.is_err_and(|err| err.is_not_found()),
        "a rule that is gone is gone"
    );

    let nonsense = admin::list_promotion_rules(&mut tx, &ctx, id, "wishes").await;
    assert!(nonsense.is_err(), "there are three rule sets, not four");

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn another_scope_sees_none_of_our_promotion_rules() {
    let shop = Shop::open().await;

    let mut ours = shop.begin().await;
    let ctx = shop.ctx();
    let id = promotion(&mut ours, &ctx, "WELCOME").await;
    admin::add_promotion_rule(
        &mut ours,
        &ctx,
        id,
        "rules",
        admin::AddRule {
            attribute: "customer.email".into(),
            operator: tezgah::promotion::Operator::Eq,
            allowed_values: vec!["someone@example.com".into()],
            description: None,
        },
    )
    .await
    .expect("a rule");
    ours.commit().await.expect("to commit");

    let theirs = shop.theirs();
    let mut tx = shop.begin_as(shop.elsewhere).await;

    let listed = admin::list_promotion_rules(&mut tx, &theirs, id, "rules")
        .await
        .expect("to list");
    assert!(
        listed.is_empty(),
        "a rule belongs to the scope that made it"
    );

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn a_host_that_refuses_is_obeyed_by_every_new_reader() {
    let shop = Shop::open().await;
    let doorman = Doorman;
    let ctx = shop.ctx_as(Actor::Staff { id: Uuid::now_v7() }, &doorman);
    let mut tx = shop.begin().await;

    let run = tezgah::id::WorkflowRunId::new();
    let id = PromotionId::new();

    let mut allowed: Vec<&str> = Vec::new();

    macro_rules! denied {
        ($what:literal, $call:expr) => {
            match $call.await {
                Err(error) if error.is_denied() => {}
                _ => allowed.push($what),
            }
        };
    }

    denied!("GET /admin/stores", admin::get_store(&mut tx, &ctx));
    denied!(
        "POST /admin/stores",
        admin::update_store(&mut tx, &ctx, admin::UpdateStore::default())
    );
    denied!("GET /admin/locales", admin::list_locales(&mut tx, &ctx));
    denied!(
        "POST /admin/locales",
        admin::set_locales(&mut tx, &ctx, admin::SetLocales::default())
    );
    denied!(
        "GET /admin/workflows-executions",
        admin::list_workflow_runs(&mut tx, &ctx, admin::ListWorkflowRuns::default())
    );
    denied!(
        "GET /admin/workflows-executions/{id}/steps",
        admin::list_workflow_run_steps(&mut tx, &ctx, run)
    );
    denied!(
        "GET /admin/workflow-dead-letters",
        admin::list_workflow_dead_letters(&mut tx, &ctx, admin::List::default())
    );
    denied!(
        "GET /admin/promotions/{id}/{rule_type}",
        admin::list_promotion_rules(&mut tx, &ctx, id, "rules")
    );
    denied!(
        "DELETE /admin/promotions/{id}/{rule_type}/{rule_id}",
        admin::delete_promotion_rule(&mut tx, &ctx, id, "rules", Uuid::now_v7())
    );

    assert!(
        allowed.is_empty(),
        "these answered somebody the host refuses everything to: {allowed:?}"
    );

    drop(tx);
    shop.close().await;
}

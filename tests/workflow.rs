//! What the engine has to be true of, asked of a real database and a real
//! clock.
//!
//! The concurrency claims are made concurrently: the parallel steps prove they
//! overlap by meeting at a barrier neither can pass alone, and the two runs
//! contending for a lock are started together rather than one after the other.

mod common;

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as Memory};
use std::time::Duration;

use async_trait::async_trait;
use common::Shop;
use parking_lot::Mutex;
use serde_json::{Value, json};
use tezgah::ports::{Ctx, Tx};
use tezgah::workflow::{
    self, Answer, Failure, Outcome, Prepared, ReachingStep, State, Step, Workflow,
};

type Log = Arc<Mutex<Vec<String>>>;

fn log() -> Log {
    Arc::new(Mutex::new(Vec::new()))
}

fn seen(log: &Log) -> Vec<String> {
    log.lock().clone()
}

/// A step that writes down that it ran, and can be told to refuse either
/// direction.
struct Probe {
    name: &'static str,
    log: Log,
    fails: bool,
    fails_at_the_database: bool,
    undo_fails: bool,
    counted: Option<Arc<AtomicUsize>>,
}

impl Probe {
    fn new(name: &'static str, log: &Log) -> Self {
        Probe {
            name,
            log: log.clone(),
            fails: false,
            fails_at_the_database: false,
            undo_fails: false,
            counted: None,
        }
    }

    fn failing(name: &'static str, log: &Log) -> Self {
        Probe {
            fails: true,
            ..Probe::new(name, log)
        }
    }

    /// Refuses the way the database does, which is the failure whose text must
    /// not reach a row.
    fn refused_by_the_database(name: &'static str, log: &Log) -> Self {
        Probe {
            fails_at_the_database: true,
            ..Probe::new(name, log)
        }
    }

    fn unrevertable(name: &'static str, log: &Log) -> Self {
        Probe {
            undo_fails: true,
            ..Probe::new(name, log)
        }
    }

    fn counting(name: &'static str, log: &Log, count: &Arc<AtomicUsize>) -> Self {
        Probe {
            counted: Some(count.clone()),
            ..Probe::new(name, log)
        }
    }
}

#[async_trait]
impl Step for Probe {
    fn name(&self) -> &'static str {
        self.name
    }

    fn max_attempts(&self) -> i32 {
        3
    }

    async fn invoke(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _input: &Value,
    ) -> Result<Outcome, Failure> {
        self.log.lock().push(self.name.to_string());
        if let Some(count) = &self.counted {
            count.fetch_add(1, Memory::SeqCst);
        }
        if self.fails_at_the_database {
            return Err(Failure::Final(tezgah::Error::from(
                sqlx::Error::RowNotFound,
            )));
        }
        if self.fails {
            return Err(Failure::Final(tezgah::Error::conflict("it said no")));
        }
        Ok(Outcome::new(json!(self.name), json!(self.name)))
    }

    async fn compensate(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _kept: &Value,
    ) -> tezgah::Result<()> {
        self.log.lock().push(format!("undo {}", self.name));
        if self.undo_fails {
            return Err(tezgah::Error::conflict("it cannot be undone"));
        }
        Ok(())
    }
}

/// A step that decides it has nothing to do — the conditional case
/// `Outcome::Skipped` exists for — and would fail the test if `compensate`
/// were ever called on it.
struct Skips {
    name: &'static str,
    log: Log,
}

#[async_trait]
impl Step for Skips {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn invoke(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        input: &Value,
    ) -> Result<Outcome, Failure> {
        self.log.lock().push(self.name.to_string());
        Ok(Outcome::skipped(input.clone()))
    }

    async fn compensate(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _kept: &Value,
    ) -> tezgah::Result<()> {
        self.log.lock().push(format!("undo {}", self.name));
        Ok(())
    }
}

/// Hands the next step whatever it was given, so a test can see what the
/// engine carried.
struct Echo;

#[async_trait]
impl Step for Echo {
    fn name(&self) -> &'static str {
        "echo"
    }

    async fn invoke(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        input: &Value,
    ) -> Result<Outcome, Failure> {
        Ok(Outcome::new(input.clone(), Value::Null))
    }
}

/// Cannot finish alone: if the other step in its set is not running at the same
/// time, this waits until it gives up.
struct Meet {
    name: &'static str,
    gate: Arc<tokio::sync::Barrier>,
}

#[async_trait]
impl Step for Meet {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn invoke(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _input: &Value,
    ) -> Result<Outcome, Failure> {
        match tokio::time::timeout(Duration::from_secs(5), self.gate.wait()).await {
            Ok(_) => Ok(Outcome::new(json!(self.name), Value::Null)),
            Err(_) => Err(Failure::Final(tezgah::Error::conflict(
                "the other step never arrived",
            ))),
        }
    }
}

/// Holds the run open long enough for somebody else to want its lock.
struct Dawdle(Duration);

#[async_trait]
impl Step for Dawdle {
    fn name(&self) -> &'static str {
        "dawdle"
    }

    async fn invoke(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _input: &Value,
    ) -> Result<Outcome, Failure> {
        tokio::time::sleep(self.0).await;
        Ok(Outcome::nothing())
    }
}

/// Says the run got here, so a worker can be told to stop.
struct Wave(Arc<AtomicBool>);

#[async_trait]
impl Step for Wave {
    fn name(&self) -> &'static str {
        "wave"
    }

    async fn invoke(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _input: &Value,
    ) -> Result<Outcome, Failure> {
        self.0.store(true, Memory::SeqCst);
        Ok(Outcome::nothing())
    }
}

/// Stays inside its step long enough for every other driver to want it.
struct Linger {
    count: Arc<AtomicUsize>,
    how_long: Duration,
}

#[async_trait]
impl Step for Linger {
    fn name(&self) -> &'static str {
        "linger"
    }

    async fn invoke(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _input: &Value,
    ) -> Result<Outcome, Failure> {
        self.count.fetch_add(1, Memory::SeqCst);
        tokio::time::sleep(self.how_long).await;
        Ok(Outcome::nothing())
    }
}

async fn until_settled(pool: &sqlx::PgPool, ctx: &Ctx<'_>, id: tezgah::id::WorkflowRunId) {
    loop {
        if let Ok(run) = workflow::get(pool, ctx, id).await {
            if matches!(run.state, State::Done | State::Reverted | State::Failed) {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Three drivers, one run, at the same time: the inline `run()` a host calls in
/// its handler and the two `work()` loops it also has running.
#[tokio::test]
async fn one_step_runs_once_however_many_drivers_want_it() {
    let shop = Shop::open().await;
    let count = Arc::new(AtomicUsize::new(0));

    let flow = Workflow::new("contended").then(Linger {
        count: count.clone(),
        how_long: Duration::from_millis(500),
    });

    let ctx = shop.ctx();
    let id = workflow::start(&shop.pool, &ctx, &flow, "contended-1", json!({}))
        .await
        .expect("the run to be written");

    let known = [&flow];
    let (_, _, driven) = tokio::join!(
        workflow::work(
            &shop.pool,
            &ctx,
            &known,
            until_settled(&shop.pool, &ctx, id)
        ),
        workflow::work(
            &shop.pool,
            &ctx,
            &known,
            until_settled(&shop.pool, &ctx, id)
        ),
        workflow::run(&shop.pool, &ctx, &flow, "contended-1", json!({})),
    );

    until_settled(&shop.pool, &ctx, id).await;

    if let Ok(run) = &driven {
        assert_eq!(run.id, id, "run() started a second run for one key");
    }

    assert_eq!(
        count.load(Memory::SeqCst),
        1,
        "the step ran more than once with three drivers on it"
    );

    let attempts: i32 =
        sqlx::query_scalar("select attempts from workflow_step where run_id = $1 and ordering = 0")
            .bind(id.as_uuid())
            .fetch_one(&mut *shop.begin().await)
            .await
            .expect("the step row");
    assert_eq!(attempts, 1, "the step was invoked twice in the database");

    let state = workflow::get(&shop.pool, &ctx, id)
        .await
        .expect("to read the run back");
    assert_eq!(state.state, State::Done, "{:?}", state.failure);

    shop.close().await;
}

async fn once(flag: Arc<AtomicBool>) {
    while !flag.load(Memory::SeqCst) {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn steps_in_one_set_run_at_the_same_time() {
    let shop = Shop::open().await;
    let gate = Arc::new(tokio::sync::Barrier::new(2));

    let flow = Workflow::new("meeting")
        .parallel(vec![
            workflow::step(Meet {
                name: "left",
                gate: gate.clone(),
            }),
            workflow::step(Meet {
                name: "right",
                gate: gate.clone(),
            }),
        ])
        .then(Echo);

    let run = workflow::run(&shop.pool, &shop.ctx(), &flow, "meeting-1", json!({}))
        .await
        .expect("the run to be driven");

    assert_eq!(run.state, State::Done, "{:?}", run.failure);
    assert_eq!(
        run.output,
        json!(["left", "right"]),
        "the set's outputs reach the next step in the order they were declared"
    );

    shop.close().await;
}

#[tokio::test]
async fn a_set_with_one_bad_step_in_it_unwinds_the_whole_run() {
    let shop = Shop::open().await;
    let log = log();

    let flow = Workflow::new("half-parallel")
        .then(Probe::new("first", &log))
        .parallel(vec![
            workflow::step(Probe::new("left", &log)),
            workflow::step(Probe::failing("right", &log)),
        ])
        .then(Probe::new("last", &log));

    let run = workflow::run(&shop.pool, &shop.ctx(), &flow, "half-1", json!({}))
        .await
        .expect("the run to be driven");

    assert_eq!(run.state, State::Reverted);
    let trail = seen(&log);
    assert!(
        !trail.contains(&"last".to_string()),
        "a step after the failure ran: {trail:?}"
    );
    let undone: Vec<String> = trail
        .iter()
        .filter(|entry| entry.starts_with("undo"))
        .cloned()
        .collect();
    assert_eq!(
        undone,
        ["undo left", "undo first"],
        "compensation did not walk back through the run: {trail:?}"
    );

    shop.close().await;
}

#[tokio::test]
async fn a_nested_workflow_is_unwound_with_the_one_that_holds_it() {
    let shop = Shop::open().await;
    let log = log();

    let inner = Workflow::new("reserve")
        .then(Probe::new("hold", &log))
        .then(Probe::new("price", &log));

    let flow = Workflow::new("checkout")
        .then(Probe::new("open", &log))
        .nest(inner)
        .then(Probe::failing("charge", &log));

    let run = workflow::run(&shop.pool, &shop.ctx(), &flow, "nested-1", json!({}))
        .await
        .expect("the run to be driven");

    assert_eq!(run.state, State::Reverted);
    let undone: Vec<String> = seen(&log)
        .into_iter()
        .filter(|entry| entry.starts_with("undo"))
        .collect();
    assert_eq!(undone, ["undo price", "undo hold", "undo open"]);

    // One run and one set of steps: the nested workflow is not a run of its own.
    let runs: i64 = sqlx::query_scalar("select count(*) from workflow_run")
        .fetch_one(&mut *shop.begin().await)
        .await
        .expect("to count runs");
    assert_eq!(runs, 1);

    let steps: i64 = sqlx::query_scalar("select count(*) from workflow_step where run_id = $1")
        .bind(run.id.as_uuid())
        .fetch_one(&mut *shop.begin().await)
        .await
        .expect("to count steps");
    assert_eq!(steps, 4);

    shop.close().await;
}

#[tokio::test]
async fn the_same_key_twice_does_not_do_the_work_twice() {
    let shop = Shop::open().await;
    let log = log();
    let count = Arc::new(AtomicUsize::new(0));

    let flow = Workflow::new("once")
        .then(Probe::counting("charge", &log, &count))
        .then(Probe::new("record", &log));

    let first = workflow::run(&shop.pool, &shop.ctx(), &flow, "same", json!({}))
        .await
        .expect("the first run");
    let second = workflow::run(&shop.pool, &shop.ctx(), &flow, "same", json!({}))
        .await
        .expect("the second run");

    assert_eq!(first.id, second.id);
    assert_eq!(first.state, State::Done);
    assert_eq!(second.state, State::Done);
    assert_eq!(
        count.load(Memory::SeqCst),
        1,
        "the same key charged twice: {:?}",
        seen(&log)
    );

    shop.close().await;
}

#[tokio::test]
async fn a_worker_takes_back_a_step_whose_lease_ran_out() {
    let shop = Shop::open().await;
    let log = log();
    let arrived = Arc::new(AtomicBool::new(false));

    let flow = Workflow::new("abandoned")
        .then(Probe::new("first", &log))
        .then(Wave(arrived.clone()));

    let id = workflow::start(&shop.pool, &shop.ctx(), &flow, "abandoned-1", json!({}))
        .await
        .expect("the run to be written");

    // What a worker that died mid-step leaves behind.
    let mut tx = shop.begin().await;
    sqlx::query(
        "update workflow_step
         set state = 'invoking', attempts = 1, lease_until = now() - interval '1 hour',
             locked_by = 'a worker that is gone'
         where run_id = $1 and ordering = 0",
    )
    .bind(id.as_uuid())
    .execute(&mut *tx)
    .await
    .expect("to abandon the step");
    tx.commit().await.expect("to commit");

    workflow::work(&shop.pool, &shop.ctx(), &[&flow], once(arrived.clone()))
        .await
        .expect("the worker to stop cleanly");

    let run = workflow::get(&shop.pool, &shop.ctx(), id)
        .await
        .expect("to read the run back");
    assert_eq!(run.state, State::Done, "{:?}", run.failure);
    assert_eq!(seen(&log), ["first"]);

    shop.close().await;
}

#[tokio::test]
async fn two_runs_on_one_key_do_not_interleave() {
    let shop = Shop::open().await;
    let cart = uuid::Uuid::now_v7();

    let flow = Workflow::new("guarded")
        .locked_by(|input| {
            input
                .get("cart")
                .and_then(Value::as_str)
                .and_then(|text| text.parse().ok())
        })
        .lock_wait(Duration::from_millis(200))
        .then(Dawdle(Duration::from_secs(2)));

    let ctx = shop.ctx();
    let input = json!({ "cart": cart.to_string() });

    let (first, second) = tokio::join!(
        workflow::run(&shop.pool, &ctx, &flow, "guarded-1", input.clone()),
        workflow::run(&shop.pool, &ctx, &flow, "guarded-2", input.clone()),
    );

    let (won, lost) = match (first, second) {
        (Ok(won), Err(lost)) | (Err(lost), Ok(won)) => (won, lost),
        other => panic!("one run should have been turned away: {other:?}"),
    };

    assert_eq!(won.state, State::Done);
    assert!(
        lost.is_conflict(),
        "the second run failed for the wrong reason: {lost:?}"
    );

    shop.close().await;
}

#[tokio::test]
async fn a_run_that_cannot_be_undone_is_written_down() {
    let shop = Shop::open().await;
    let log = log();

    let flow = Workflow::new("stuck")
        .then(Probe::unrevertable("charge", &log))
        .then(Probe::failing("ship", &log));

    let run = workflow::run(&shop.pool, &shop.ctx(), &flow, "stuck-1", json!({}))
        .await
        .expect("the run to be driven");

    assert_eq!(run.state, State::Failed);

    let (step, failure): (String, String) =
        sqlx::query_as("select step_name, failure from workflow_dead_letter where run_id = $1")
            .bind(run.id.as_uuid())
            .fetch_one(&mut *shop.begin().await)
            .await
            .expect("a dead letter for the step that could not be undone");

    assert_eq!(step, "charge");
    assert!(failure.contains("undone"), "{failure}");

    shop.close().await;
}

/// `report()` is for a log. What a workflow row keeps is served on the admin
/// surface, so it is `Display` — which says a query was refused and not which
/// query, which table or which constraint.
#[tokio::test]
async fn a_failed_step_stores_no_database_message() {
    let shop = Shop::open().await;
    let log = log();

    let flow = Workflow::new("leaky")
        .then(Probe::new("first", &log))
        .then(Probe::refused_by_the_database("write", &log));

    let run = workflow::run(&shop.pool, &shop.ctx(), &flow, "leaky-1", json!({}))
        .await
        .expect("the run to be driven");

    assert_eq!(run.state, State::Reverted);

    let stored: Vec<String> = sqlx::query_scalar(
        "select failure from workflow_step where run_id = $1 and failure is not null",
    )
    .bind(run.id.as_uuid())
    .fetch_all(&mut *shop.begin().await)
    .await
    .expect("the step's failure");

    let inside = sqlx::Error::RowNotFound.to_string();
    for failure in stored.iter().chain(run.failure.iter()) {
        assert!(
            failure.contains("the database refused a query"),
            "{failure}"
        );
        assert!(
            !failure.contains(&inside),
            "the database's own words were served: {failure}"
        );
    }

    let told = tezgah::Error::from(sqlx::Error::RowNotFound);
    assert!(
        told.report().contains(&inside),
        "report() stopped saying what happened"
    );

    shop.close().await;
}

/// A step that decided there was nothing to do is recorded as `skipped`,
/// carries its input forward to the next step, and is not one the run walks
/// back through when it unwinds later.
#[tokio::test]
async fn a_skipped_step_is_not_compensated() {
    let shop = Shop::open().await;
    let log = log();

    let flow = Workflow::new("conditional")
        .then(Skips {
            name: "maybe_reserve",
            log: log.clone(),
        })
        .then(Probe::failing("charge", &log));

    let run = workflow::run(&shop.pool, &shop.ctx(), &flow, "skip-1", json!({}))
        .await
        .expect("the run to be driven");

    assert_eq!(run.state, State::Reverted);

    let undone: Vec<String> = seen(&log)
        .into_iter()
        .filter(|entry| entry.starts_with("undo"))
        .collect();
    assert_eq!(
        undone,
        Vec::<String>::new(),
        "a skipped step wrote nothing, so unwinding it is not called"
    );

    let mut tx = shop.begin().await;
    let steps = workflow::steps(&mut tx, &shop.ctx(), run.id)
        .await
        .expect("to read the steps back");
    let skipped = steps
        .iter()
        .find(|s| s.name == "maybe_reserve")
        .expect("the skipped step's row");
    assert_eq!(skipped.state, "skipped");
    assert_eq!(
        skipped.attempts, 1,
        "a step that decided it had nothing to do succeeds on its first try"
    );

    shop.close().await;
}

/// A fake [`ReachingStep`]: no provider, just a log of which phase ran and a
/// marker written to `workflow_run.output` so `call` can prove it is reading
/// what `prepare` actually committed, not merely what ran before it in
/// process order.
struct FakeReaching {
    log: Log,
    run_id: tezgah::id::WorkflowRunId,
    pool: sqlx::PgPool,
}

#[async_trait]
impl ReachingStep for FakeReaching {
    fn name(&self) -> &'static str {
        "fake-reaching"
    }

    async fn prepare(
        &self,
        tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _input: &Value,
    ) -> Result<Prepared, Failure> {
        self.log.lock().push("prepare".to_string());
        sqlx::query("update workflow_run set output = $2 where id = $1")
            .bind(self.run_id.as_uuid())
            .bind(json!("prepared"))
            .execute(&mut **tx)
            .await
            .map_err(|err| Failure::Final(tezgah::Error::from(err)))?;
        Ok(Prepared {
            idempotency_key: "fake-key".to_string(),
            data: Value::Null,
        })
    }

    async fn call(&self, ctx: &Ctx<'_>, _prepared: &Prepared) -> Result<Answer, Failure> {
        self.log.lock().push("call".to_string());
        // A fresh connection, scoped and rolled back rather than reusing
        // `prepare`'s transaction — so seeing the row here proves `prepare`
        // committed, not just that these ran in sequence in one process.
        let mut probe = self
            .pool
            .begin()
            .await
            .map_err(|err| Failure::Final(tezgah::Error::from(err)))?;
        sqlx::query("select set_config('app.scope', $1, true)")
            .bind(ctx.scope.0.to_string())
            .execute(&mut *probe)
            .await
            .map_err(|err| Failure::Final(tezgah::Error::from(err)))?;
        let visible: Option<Value> =
            sqlx::query_scalar("select output from workflow_run where id = $1")
                .bind(self.run_id.as_uuid())
                .fetch_one(&mut *probe)
                .await
                .map_err(|err| Failure::Final(tezgah::Error::from(err)))?;
        let _ = probe.rollback().await;
        assert_eq!(
            visible,
            Some(json!("prepared")),
            "call must see prepare's own commit, not an open transaction"
        );
        Ok(Answer(json!("called")))
    }

    async fn record(
        &self,
        tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _prepared: &Prepared,
        answer: Answer,
    ) -> Result<Outcome, Failure> {
        self.log.lock().push("record".to_string());
        sqlx::query("update workflow_run set output = $2 where id = $1")
            .bind(self.run_id.as_uuid())
            .bind(&answer.0)
            .execute(&mut **tx)
            .await
            .map_err(|err| Failure::Final(tezgah::Error::from(err)))?;
        Ok(Outcome::new(answer.0, Value::Null))
    }
}

/// #158: the three phases of a [`ReachingStep`] run in order, and `call` sees
/// what `prepare` wrote only because the runner commits `prepare`'s
/// transaction first. Driven by hand — the runner does not dispatch
/// `ReachingStep` yet, only `Step` — which is exactly what "landed, unused"
/// means at this stage of #158's migration.
#[tokio::test]
async fn reaching_step_phases_run_in_order_and_prepare_commits_before_call() {
    let shop = Shop::open().await;
    let log = log();

    // A one-step `Step` workflow, just to get a real `workflow_run` row to
    // use as the marker: nothing about it is driven with `run`/`work`.
    let flow = Workflow::new("fake-reaching-flow").then(Skips {
        name: "placeholder",
        log: log.clone(),
    });
    let id = workflow::start(&shop.pool, &shop.ctx(), &flow, "fake-reaching-1", json!({}))
        .await
        .expect("the run row to exist");

    let fake = FakeReaching {
        log: log.clone(),
        run_id: id,
        pool: shop.pool.clone(),
    };

    let ctx = shop.ctx();
    let mut prepare_tx = shop.begin().await;
    let prepared = fake
        .prepare(&mut prepare_tx, &ctx, &json!({}))
        .await
        .expect("prepare to succeed");
    prepare_tx
        .commit()
        .await
        .expect("prepare's transaction to commit");

    let answer = fake.call(&ctx, &prepared).await.expect("call to succeed");

    let mut record_tx = shop.begin().await;
    let outcome = fake
        .record(&mut record_tx, &ctx, &prepared, answer)
        .await
        .expect("record to succeed");
    record_tx
        .commit()
        .await
        .expect("record's transaction to commit");

    assert_eq!(seen(&log), ["prepare", "call", "record"]);
    match outcome {
        Outcome::Ran { output, .. } => assert_eq!(output, json!("called")),
        other => panic!("record should have produced Outcome::Ran: {other:?}"),
    }

    shop.close().await;
}

/// A fake [`ReachingStep`] driven by the real runner rather than by hand.
/// `run_id` is filled in by the test right after [`workflow::start`], before
/// [`workflow::run`] on the same key actually drives it — the run has to
/// exist for the test to know its id, and `call` needs the id to check what
/// `prepare` committed from a connection of its own.
struct DrivenReaching {
    log: Log,
    run_id: Arc<Mutex<Option<tezgah::id::WorkflowRunId>>>,
    pool: sqlx::PgPool,
}

impl DrivenReaching {
    fn id(&self) -> tezgah::id::WorkflowRunId {
        self.run_id
            .lock()
            .as_ref()
            .copied()
            .expect("the test to have filled in the run id before driving")
    }
}

#[async_trait]
impl ReachingStep for DrivenReaching {
    fn name(&self) -> &'static str {
        "driven-reaching"
    }

    async fn prepare(
        &self,
        tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _input: &Value,
    ) -> Result<Prepared, Failure> {
        self.log.lock().push("prepare".to_string());
        sqlx::query("update workflow_run set output = $2 where id = $1")
            .bind(self.id().as_uuid())
            .bind(json!("prepared"))
            .execute(&mut **tx)
            .await
            .map_err(|err| Failure::Final(tezgah::Error::from(err)))?;
        Ok(Prepared {
            idempotency_key: "driven-key".to_string(),
            data: Value::Null,
        })
    }

    async fn call(&self, ctx: &Ctx<'_>, _prepared: &Prepared) -> Result<Answer, Failure> {
        self.log.lock().push("call".to_string());
        // A fresh connection, rolled back rather than reusing `prepare`'s
        // transaction — seeing the row here proves `prepare` committed and
        // nothing still holds it open while the provider is asked.
        let mut probe = self
            .pool
            .begin()
            .await
            .map_err(|err| Failure::Final(tezgah::Error::from(err)))?;
        sqlx::query("select set_config('app.scope', $1, true)")
            .bind(ctx.scope.0.to_string())
            .execute(&mut *probe)
            .await
            .map_err(|err| Failure::Final(tezgah::Error::from(err)))?;
        let visible: Option<Value> =
            sqlx::query_scalar("select output from workflow_run where id = $1")
                .bind(self.id().as_uuid())
                .fetch_one(&mut *probe)
                .await
                .map_err(|err| Failure::Final(tezgah::Error::from(err)))?;
        let _ = probe.rollback().await;
        assert_eq!(
            visible,
            Some(json!("prepared")),
            "call must see prepare's own commit, not an open transaction"
        );
        Ok(Answer(json!("called")))
    }

    async fn record(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _prepared: &Prepared,
        answer: Answer,
    ) -> Result<Outcome, Failure> {
        self.log.lock().push("record".to_string());
        Ok(Outcome::new(answer.0, Value::Null))
    }
}

/// #158 step 3/4: the runner now drives a [`ReachingStep`] for real —
/// `prepare`, `call` and `record` run in that order through
/// [`workflow::run`], and `call` proves it holds no transaction by reading
/// `prepare`'s commit from a connection of its own.
#[tokio::test]
async fn a_reaching_step_is_driven_through_all_three_phases() {
    let shop = Shop::open().await;
    let log = log();
    let run_id = Arc::new(Mutex::new(None));

    let flow = Workflow::new("reaching-driven").reaching_step(DrivenReaching {
        log: log.clone(),
        run_id: run_id.clone(),
        pool: shop.pool.clone(),
    });

    let id = workflow::start(
        &shop.pool,
        &shop.ctx(),
        &flow,
        "reaching-driven-1",
        json!({}),
    )
    .await
    .expect("the run row to exist before it is driven");

    // `run_id` is a second handle onto the same cell `DrivenReaching` reads,
    // so this reaches the instance the runner will actually drive.
    *run_id.lock() = Some(id);

    let run = workflow::run(
        &shop.pool,
        &shop.ctx(),
        &flow,
        "reaching-driven-1",
        json!({}),
    )
    .await
    .expect("the run to be driven");

    assert_eq!(run.state, State::Done, "{:?}", run.failure);
    assert_eq!(seen(&log), ["prepare", "call", "record"]);
    assert_eq!(run.output, json!("called"));

    shop.close().await;
}

/// A `called` row's lease running out does not put it back to `pending` —
/// that would let a second driver call the provider again. It stays
/// `called`, only the lease and the lock clear.
#[tokio::test]
async fn a_called_step_stays_called_when_its_lease_runs_out() {
    let shop = Shop::open().await;
    let log = log();

    let flow = Workflow::new("called-lease").then(Probe::new("first", &log));
    let id = workflow::start(&shop.pool, &shop.ctx(), &flow, "called-lease-1", json!({}))
        .await
        .expect("the run to be written");

    // What a step split into prepare/call/record leaves behind mid-call: a
    // separate connection commits this, the way `prepare` would.
    let mut tx = shop.begin().await;
    sqlx::query(
        "update workflow_step
         set state = 'called', attempts = 1, lease_until = now() - interval '1 hour',
             locked_by = 'a worker mid-call'
         where run_id = $1 and ordering = 0",
    )
    .bind(id.as_uuid())
    .execute(&mut *tx)
    .await
    .expect("to write the called row");
    tx.commit().await.expect("to commit on its own connection");

    let recovered = workflow::recover(&shop.pool, &shop.ctx())
        .await
        .expect("recover to run");
    assert_eq!(recovered, 1, "recover should have touched the called row");

    let mut tx = shop.begin().await;
    let (state, lease_until, locked_by): (
        String,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<String>,
    ) = sqlx::query_as(
        "select state, lease_until, locked_by from workflow_step
             where run_id = $1 and ordering = 0",
    )
    .bind(id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("to read the row back");
    tx.commit().await.expect("to commit");

    assert_eq!(
        state, "called",
        "a called row's lease running out must not reset it to pending"
    );
    assert!(lease_until.is_none(), "the lease should have cleared");
    assert!(locked_by.is_none(), "the lock should have cleared");

    shop.close().await;
}

/// A `called` row whose lease has expired is claimable by a fresh driver —
/// and the row it reads back still says `called`, not `pending`, so a
/// claimant that only checks state cannot mistake it for one that never ran.
#[tokio::test]
async fn a_lease_expired_called_step_is_claimable_and_still_reads_called() {
    let shop = Shop::open().await;
    let log = log();

    let flow = Workflow::new("called-claim").then(Probe::new("first", &log));
    let id = workflow::start(&shop.pool, &shop.ctx(), &flow, "called-claim-1", json!({}))
        .await
        .expect("the run to be written");

    let mut tx = shop.begin().await;
    sqlx::query(
        "update workflow_step
         set state = 'called', attempts = 1, lease_until = now() - interval '1 hour',
             locked_by = 'a worker mid-call'
         where run_id = $1 and ordering = 0",
    )
    .bind(id.as_uuid())
    .execute(&mut *tx)
    .await
    .expect("to write the called row");
    tx.commit().await.expect("to commit");

    // `invoke`'s own claim-and-run query does not yet recognise `called` (no
    // `Step` produces it today), so a worker that takes this row cannot
    // finish it — it loops. Bounding the wait is enough to see whether the
    // row got claimed at all.
    let _ = tokio::time::timeout(
        Duration::from_millis(700),
        workflow::work(
            &shop.pool,
            &shop.ctx(),
            &[&flow],
            std::future::pending::<()>(),
        ),
    )
    .await;

    let mut tx = shop.begin().await;
    let (state, locked_by): (String, Option<String>) = sqlx::query_as(
        "select state, locked_by from workflow_step where run_id = $1 and ordering = 0",
    )
    .bind(id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("to read the row back");
    tx.commit().await.expect("to commit");

    assert_eq!(
        state, "called",
        "a claimed called row must still read called, not pending: the \
         claimant asks what happened rather than assuming nothing did"
    );
    assert_ne!(
        locked_by,
        Some("a worker mid-call".to_string()),
        "the fresh driver should have taken the lease"
    );
    assert!(
        seen(&log).is_empty(),
        "the driver must not have invoked the step as if it were pending"
    );

    shop.close().await;
}

/// Writes a `called` row by hand — what a crash between `call` and `record`
/// leaves behind — with a `prepared` envelope the runner can read back, the
/// same shape [`workflow::run`]'s own `prepare` phase writes.
async fn seed_called(shop: &Shop, run_id: tezgah::id::WorkflowRunId, idempotency_key: &str) {
    let mut tx = shop.begin().await;
    sqlx::query(
        "update workflow_step
         set state = 'called', attempts = 0,
             prepared = $2, compensate_input = 'null'::jsonb
         where run_id = $1 and ordering = 0",
    )
    .bind(run_id.as_uuid())
    .bind(json!({ "idempotency_key": idempotency_key, "data": Value::Null }))
    .execute(&mut *tx)
    .await
    .expect("to seed the called row");
    tx.commit().await.expect("to commit");
}

/// A fake [`ReachingStep`] whose `resume` is distinct from `call`, so a test
/// can tell which one the runner reached for a row that starts `called`.
struct AskOnResume {
    log: Log,
}

#[async_trait]
impl ReachingStep for AskOnResume {
    fn name(&self) -> &'static str {
        "ask-on-resume"
    }

    async fn prepare(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _input: &Value,
    ) -> Result<Prepared, Failure> {
        self.log.lock().push("prepare".to_string());
        Ok(Prepared {
            idempotency_key: "ask-key".to_string(),
            data: Value::Null,
        })
    }

    async fn call(&self, _ctx: &Ctx<'_>, _prepared: &Prepared) -> Result<Answer, Failure> {
        self.log.lock().push("call".to_string());
        Ok(Answer(json!("called-fresh")))
    }

    async fn resume(&self, _ctx: &Ctx<'_>, _prepared: &Prepared) -> Result<Answer, Failure> {
        self.log.lock().push("resume".to_string());
        Ok(Answer(json!("asked")))
    }

    async fn record(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _prepared: &Prepared,
        answer: Answer,
    ) -> Result<Outcome, Failure> {
        self.log.lock().push("record".to_string());
        Ok(Outcome::new(answer.0, Value::Null))
    }
}

/// #158: a row a crash left `called` is resumed by asking what happened, not
/// by calling `prepare`/`call` again — the driver that finds a `called` row
/// never runs either.
#[tokio::test]
async fn a_called_row_is_resumed_by_asking_not_calling_again() {
    let shop = Shop::open().await;
    let log = log();

    let flow = Workflow::new("resumed-by-asking").reaching_step(AskOnResume { log: log.clone() });
    let id = workflow::start(
        &shop.pool,
        &shop.ctx(),
        &flow,
        "resumed-by-asking-1",
        json!({}),
    )
    .await
    .expect("the run row to exist");

    seed_called(&shop, id, "ask-key").await;

    let run = workflow::run(
        &shop.pool,
        &shop.ctx(),
        &flow,
        "resumed-by-asking-1",
        json!({}),
    )
    .await
    .expect("the run to be driven");

    assert_eq!(run.state, State::Done, "{:?}", run.failure);
    assert_eq!(
        seen(&log),
        ["resume", "record"],
        "a called row must be resumed, not prepared or called again"
    );
    assert_eq!(run.output, json!("asked"));

    shop.close().await;
}

/// A fake [`ReachingStep`] whose `resume` can never answer — standing in for
/// a `lookup` that came back `Err`, which #158's kasapay note says is not
/// the same as `Ok(None)`.
struct UnanswerableResume {
    log: Log,
}

#[async_trait]
impl ReachingStep for UnanswerableResume {
    fn name(&self) -> &'static str {
        "unanswerable-resume"
    }

    fn max_attempts(&self) -> i32 {
        1
    }

    async fn prepare(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _input: &Value,
    ) -> Result<Prepared, Failure> {
        self.log.lock().push("prepare".to_string());
        Ok(Prepared {
            idempotency_key: "unanswerable-key".to_string(),
            data: Value::Null,
        })
    }

    async fn call(&self, _ctx: &Ctx<'_>, _prepared: &Prepared) -> Result<Answer, Failure> {
        self.log.lock().push("call".to_string());
        Ok(Answer(json!("called")))
    }

    async fn resume(&self, _ctx: &Ctx<'_>, _prepared: &Prepared) -> Result<Answer, Failure> {
        self.log.lock().push("resume".to_string());
        Err(Failure::Retry(tezgah::Error::provider(
            "fake",
            "the question went unanswered",
        )))
    }

    async fn record(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _prepared: &Prepared,
        answer: Answer,
    ) -> Result<Outcome, Failure> {
        self.log.lock().push("record".to_string());
        Ok(Outcome::new(answer.0, Value::Null))
    }

    async fn compensate(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _kept: &Value,
    ) -> tezgah::Result<()> {
        self.log.lock().push("undo".to_string());
        Ok(())
    }
}

/// #158 / kasapay#122: `resume` going unanswered leaves the row `called` and
/// unwinds the run through compensation — it is never read as "nothing was
/// taken", which is what a second authorisation would come from.
#[tokio::test]
async fn an_unanswered_resume_is_never_read_as_nothing_happened() {
    let shop = Shop::open().await;
    let log = log();

    let flow =
        Workflow::new("unanswered-resume").reaching_step(UnanswerableResume { log: log.clone() });
    let id = workflow::start(
        &shop.pool,
        &shop.ctx(),
        &flow,
        "unanswered-resume-1",
        json!({}),
    )
    .await
    .expect("the run row to exist");

    seed_called(&shop, id, "unanswerable-key").await;

    let run = workflow::run(
        &shop.pool,
        &shop.ctx(),
        &flow,
        "unanswered-resume-1",
        json!({}),
    )
    .await
    .expect("the run to be driven, and to report a failure rather than panic");

    assert_eq!(run.state, State::Reverted, "{:?}", run.failure);
    assert_eq!(
        seen(&log),
        ["resume", "undo"],
        "an unanswered resume must never reach record — that would be \
         treating the question going unanswered as nothing having happened"
    );

    shop.close().await;
}

/// A fake [`ReachingStep`] that never overrides `resume`, so a `called` row
/// is resumed by replaying `call` — the default, correct only when the
/// provider itself guarantees the same idempotency key never authorises
/// twice. `authorized` is that guarantee, modelled: it only grows the first
/// time a key is seen, exactly as a real provider's ledger would.
struct ReplayedByDefault {
    log: Log,
    authorized: Arc<Mutex<HashSet<String>>>,
}

#[async_trait]
impl ReachingStep for ReplayedByDefault {
    fn name(&self) -> &'static str {
        "replayed-by-default"
    }

    async fn prepare(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _input: &Value,
    ) -> Result<Prepared, Failure> {
        self.log.lock().push("prepare".to_string());
        Ok(Prepared {
            idempotency_key: "replay-key".to_string(),
            data: Value::Null,
        })
    }

    async fn call(&self, _ctx: &Ctx<'_>, prepared: &Prepared) -> Result<Answer, Failure> {
        self.log.lock().push("call".to_string());
        self.authorized
            .lock()
            .insert(prepared.idempotency_key.clone());
        Ok(Answer(json!("authorized")))
    }

    async fn record(
        &self,
        _tx: &mut Tx<'_>,
        _ctx: &Ctx<'_>,
        _prepared: &Prepared,
        answer: Answer,
    ) -> Result<Outcome, Failure> {
        self.log.lock().push("record".to_string());
        Ok(Outcome::new(answer.0, Value::Null))
    }
}

/// kasapay#122: a provider with no `lookup_by_order` is resumed by replaying
/// the same idempotency key — safe because the provider guarantees a second
/// call with it returns the original rather than opening a second one. This
/// is that guarantee holding: the row starts `called` as if `call` had
/// already reached the provider once, and replaying it does not grow the
/// provider's own ledger a second time.
#[tokio::test]
async fn replaying_the_default_resume_produces_one_authorisation() {
    let shop = Shop::open().await;
    let log = log();
    let authorized = Arc::new(Mutex::new(HashSet::new()));
    // What the provider already holds from the call whose answer was lost.
    authorized.lock().insert("replay-key".to_string());

    let flow = Workflow::new("replayed-resume").reaching_step(ReplayedByDefault {
        log: log.clone(),
        authorized: authorized.clone(),
    });
    let id = workflow::start(
        &shop.pool,
        &shop.ctx(),
        &flow,
        "replayed-resume-1",
        json!({}),
    )
    .await
    .expect("the run row to exist");

    seed_called(&shop, id, "replay-key").await;

    let run = workflow::run(
        &shop.pool,
        &shop.ctx(),
        &flow,
        "replayed-resume-1",
        json!({}),
    )
    .await
    .expect("the run to be driven");

    assert_eq!(run.state, State::Done, "{:?}", run.failure);
    assert_eq!(
        seen(&log),
        ["call", "record"],
        "the default resume replays call, not prepare"
    );
    assert_eq!(
        authorized.lock().len(),
        1,
        "replaying the same idempotency key must not authorise twice"
    );

    shop.close().await;
}

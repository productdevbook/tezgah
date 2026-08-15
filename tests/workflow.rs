//! What the engine has to be true of, asked of a real database and a real
//! clock.
//!
//! The concurrency claims are made concurrently: the parallel steps prove they
//! overlap by meeting at a barrier neither can pass alone, and the two runs
//! contending for a lock are started together rather than one after the other.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as Memory};
use std::time::Duration;

use async_trait::async_trait;
use common::Shop;
use parking_lot::Mutex;
use serde_json::{Value, json};
use tezgah::ports::{Ctx, Tx};
use tezgah::workflow::{self, Failure, Outcome, State, Step, Workflow};

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

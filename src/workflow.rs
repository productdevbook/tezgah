//! Running something that cannot be one transaction, and undoing it if a later
//! part fails.
//!
//! Checkout reserves stock, asks a provider for money, writes an order and
//! opens a fulfilment. The provider is not in the database, so there is no
//! transaction that covers all four: by the time the card is charged, the
//! earlier writes are committed and visible.
//!
//! A [`Workflow`] is a list of [`Step`]s, each of which says how to undo what
//! it did. The engine runs them in order, and when one fails it walks back
//! through the ones that succeeded, calling their compensation. Nothing is
//! left half-done for somebody to find in the morning.
//!
//! Unlike the rest of the crate, the engine opens its own transactions: one
//! per step, so a step that finished stays finished when a later one does not.
//! Single operations still run in the caller's transaction.
//!
//! # Idempotency
//!
//! A run is identified by `(name, transaction_key)`. Starting the same key
//! twice picks the existing run up where it stopped rather than doing anything
//! again, which is what makes a retried request safe.
//!
//! # Compensation takes its own input
//!
//! A step's compensation is given what the step chose to keep, not what it
//! returned. Undoing a charge needs the charge's id even when the step returned
//! nothing worth showing anybody.
//!
//! # Steps that do not have to wait for each other
//!
//! [`Workflow::parallel`] takes a set of steps that run at once. The next step
//! begins when all of them have finished, and is handed their outputs as an
//! array in the order they were declared. Unwinding is unaffected: it walks
//! back through every step that succeeded, one at a time, latest first, inside
//! a parallel set as much as outside one.
//!
//! # Workflows inside workflows
//!
//! [`Workflow::nest`] folds another workflow's steps into this one. They share
//! the run, the transaction key and the unwinding, so a nested workflow is one
//! unit rather than a run of its own to reconcile afterwards.
//!
//! # One at a time on the same thing
//!
//! [`Workflow::locked_by`] derives a Postgres advisory lock from the input, so
//! two checkouts of one cart cannot interleave. A run that cannot take the lock
//! within [`Workflow::lock_wait`] fails rather than queueing behind it.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::id::WorkflowRunId;
use crate::ports::{Ctx, Tx};

/// How long a claim on a step is good for. A step still working extends it.
pub const LEASE: Duration = Duration::seconds(300);

/// How long [`run`] waits for a lock before giving up, unless told otherwise.
pub const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// How long a [`work`] loop sleeps when it found nothing to do.
pub const IDLE: std::time::Duration = std::time::Duration::from_millis(250);

/// What a step kept: what to hand the next step, and what its own compensation
/// will need.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub output: Value,
    pub compensate_input: Value,
}

impl Outcome {
    /// Nothing to pass on and nothing to undo.
    pub fn nothing() -> Self {
        Outcome {
            output: Value::Null,
            compensate_input: Value::Null,
        }
    }

    pub fn new(output: Value, compensate_input: Value) -> Self {
        Outcome {
            output,
            compensate_input,
        }
    }
}

/// How a step failed, which decides whether it is tried again.
#[derive(Debug)]
pub enum Failure {
    /// The provider timed out, the row was locked, the network blinked. Tried
    /// again until the attempt ceiling.
    Retry(Error),
    /// The card was declined, the stock is gone, the rule says no. Not tried
    /// again; the run unwinds.
    Final(Error),
}

impl Failure {
    fn error(self) -> Error {
        match self {
            Failure::Retry(err) | Failure::Final(err) => err,
        }
    }

    fn retryable(&self) -> bool {
        matches!(self, Failure::Retry(_))
    }
}

/// Retry or not is decided by the type rather than by looking for words in a
/// message: translating an error should never change what the engine does.
#[async_trait]
pub trait Step: Send + Sync {
    fn name(&self) -> &'static str;

    /// How many times invoking may be attempted before the run unwinds.
    fn max_attempts(&self) -> i32 {
        3
    }

    async fn invoke(
        &self,
        tx: &mut Tx<'_>,
        ctx: &Ctx<'_>,
        input: &Value,
    ) -> std::result::Result<Outcome, Failure>;

    /// Undo what [`Step::invoke`] did, given what it kept.
    ///
    /// The default does nothing, which is right for a step that only read. A
    /// step that wrote and does not override this is a bug the tests look for.
    async fn compensate(&self, _tx: &mut Tx<'_>, _ctx: &Ctx<'_>, _kept: &Value) -> Result<()> {
        Ok(())
    }
}

/// Puts a step behind the pointer [`Workflow::parallel`] takes, so a set can
/// hold steps of different types.
pub fn step(one: impl Step + 'static) -> Arc<dyn Step> {
    Arc::new(one)
}

/// Reads the run's input and says what must not be touched at the same time,
/// or `None` for a run that needs no lock.
type LockKey = Arc<dyn Fn(&Value) -> Option<Uuid> + Send + Sync>;

pub struct Workflow {
    name: &'static str,
    /// Each entry is a set of steps that run at once; most hold one.
    slots: Vec<Vec<Arc<dyn Step>>>,
    lock: Option<LockKey>,
    lock_wait: std::time::Duration,
}

impl std::fmt::Debug for Workflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Workflow")
            .field("name", &self.name)
            .field(
                "steps",
                &self
                    .slots
                    .iter()
                    .map(|slot| slot.iter().map(|s| s.name()).collect::<Vec<_>>())
                    .collect::<Vec<_>>(),
            )
            .field("locked", &self.lock.is_some())
            .finish()
    }
}

/// One set of steps that run together, and where they sit in the run.
struct Slot<'a> {
    group: i32,
    at: i32,
    steps: &'a [Arc<dyn Step>],
}

impl Workflow {
    pub fn new(name: &'static str) -> Self {
        Workflow {
            name,
            slots: Vec::new(),
            lock: None,
            lock_wait: LOCK_WAIT,
        }
    }

    pub fn then(mut self, one: impl Step + 'static) -> Self {
        self.slots.push(vec![Arc::new(one)]);
        self
    }

    /// Steps that run at once. The next step is handed their outputs as an
    /// array, in the order given here.
    pub fn parallel(mut self, steps: Vec<Arc<dyn Step>>) -> Self {
        if !steps.is_empty() {
            self.slots.push(steps);
        }
        self
    }

    /// Another workflow's steps, run as part of this one: same run, same
    /// transaction key, unwound together. The inner workflow's own lock is not
    /// carried over — the run that owns the steps owns the lock.
    pub fn nest(mut self, inner: Workflow) -> Self {
        self.slots.extend(inner.slots);
        self
    }

    /// What this run must have to itself, derived from the run's input.
    pub fn locked_by(
        mut self,
        key: impl Fn(&Value) -> Option<Uuid> + Send + Sync + 'static,
    ) -> Self {
        self.lock = Some(Arc::new(key));
        self
    }

    /// How long to wait for that lock before failing. Never forever.
    pub fn lock_wait(mut self, wait: std::time::Duration) -> Self {
        self.lock_wait = wait;
        self
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    fn plan(&self) -> Vec<Slot<'_>> {
        let mut at = 0;
        let mut plan = Vec::with_capacity(self.slots.len());
        for (group, steps) in self.slots.iter().enumerate() {
            plan.push(Slot {
                group: group as i32,
                at,
                steps,
            });
            at += steps.len() as i32;
        }
        plan
    }
}

/// What became of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Running,
    Compensating,
    /// Every step invoked.
    Done,
    /// A step failed and every earlier step was undone.
    Reverted,
    /// A step failed and undoing it failed too. Somebody has to look.
    Failed,
}

impl State {
    fn as_str(self) -> &'static str {
        match self {
            State::Running => "running",
            State::Compensating => "compensating",
            State::Done => "done",
            State::Reverted => "reverted",
            State::Failed => "failed",
        }
    }

    fn parse(text: &str) -> Result<Self> {
        Ok(match text {
            "running" => State::Running,
            "compensating" => State::Compensating,
            "done" => State::Done,
            "reverted" => State::Reverted,
            "failed" => State::Failed,
            _ => return Err(Error::bug("a workflow run holds a state nothing writes")),
        })
    }

    fn settled(self) -> bool {
        matches!(self, State::Done | State::Reverted | State::Failed)
    }
}

#[derive(Debug, Clone)]
pub struct Run {
    pub id: WorkflowRunId,
    pub state: State,
    pub output: Value,
    pub failure: Option<String>,
}

/// Starts a workflow, or picks up the one this key already started.
///
/// Opens its own transactions, one per step. The caller's transaction, if it
/// has one, should be committed first: a run that is waiting on a row this
/// request has not released will simply wait.
pub async fn run(
    pool: &PgPool,
    ctx: &Ctx<'_>,
    workflow: &Workflow,
    transaction_key: &str,
    input: Value,
) -> Result<Run> {
    let guard = match workflow.lock.as_ref().and_then(|key| key(&input)) {
        Some(key) => Some(acquire(pool, key, workflow.lock_wait).await?),
        None => None,
    };

    let outcome = async {
        let id = claim(pool, ctx, workflow, transaction_key, input).await?;
        drive(pool, ctx, workflow, id).await
    }
    .await;

    if let Some(tx) = guard {
        tx.rollback().await?;
    }
    outcome
}

/// Writes the run and its steps without running any of them, for a caller that
/// wants a [`work`] loop to do it.
pub async fn start(
    pool: &PgPool,
    ctx: &Ctx<'_>,
    workflow: &Workflow,
    transaction_key: &str,
    input: Value,
) -> Result<WorkflowRunId> {
    claim(pool, ctx, workflow, transaction_key, input).await
}

async fn scoped(
    pool: &PgPool,
    ctx: &Ctx<'_>,
) -> Result<sqlx::Transaction<'static, sqlx::Postgres>> {
    let mut tx = pool.begin().await?;
    sqlx::query("select set_config('app.scope', $1, true)")
        .bind(ctx.scope.0.to_string())
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

/// Folds a uuid into the one integer an advisory lock is named by. Only the
/// entity goes in, not the workflow: two different workflows on one cart are
/// exactly what this is for.
fn advisory_key(id: Uuid) -> i64 {
    let (high, low) = id.as_u64_pair();
    (high ^ low) as i64
}

/// Holds the lock in an open transaction, because the engine's own
/// transactions are one per step and a lock taken in one would be gone by the
/// next. Losing the connection releases it, which is the point of not using a
/// row.
async fn acquire(
    pool: &PgPool,
    key: Uuid,
    wait: std::time::Duration,
) -> Result<sqlx::Transaction<'static, sqlx::Postgres>> {
    let key = advisory_key(key);
    let deadline = std::time::Instant::now() + wait;

    loop {
        let mut tx = pool.begin().await?;
        let taken: bool = sqlx::query_scalar("select pg_try_advisory_xact_lock($1)")
            .bind(key)
            .fetch_one(&mut *tx)
            .await?;

        if taken {
            return Ok(tx);
        }

        tx.rollback().await?;

        if std::time::Instant::now() >= deadline {
            return Err(Error::conflict(
                "something else is already working on this; try again",
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

/// Inserts the run and its steps, or finds the run this key already made.
async fn claim(
    pool: &PgPool,
    ctx: &Ctx<'_>,
    workflow: &Workflow,
    transaction_key: &str,
    input: Value,
) -> Result<WorkflowRunId> {
    let mut tx = scoped(pool, ctx).await?;

    let existing: Option<Uuid> = sqlx::query_scalar(
        "select id from workflow_run where scope = $1 and name = $2 and transaction_key = $3",
    )
    .bind(ctx.scope.0)
    .bind(workflow.name)
    .bind(transaction_key)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(id) = existing {
        tx.commit().await?;
        return Ok(WorkflowRunId::from_uuid(id));
    }

    let id = WorkflowRunId::new();
    sqlx::query(
        "insert into workflow_run (id, scope, name, transaction_key, input)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(workflow.name)
    .bind(transaction_key)
    .bind(&input)
    .execute(&mut *tx)
    .await?;

    for slot in workflow.plan() {
        for (k, one) in slot.steps.iter().enumerate() {
            sqlx::query(
                "insert into workflow_step
                     (id, scope, run_id, name, ordering, group_ordering, max_attempts)
                 values ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(Uuid::now_v7())
            .bind(ctx.scope.0)
            .bind(id.as_uuid())
            .bind(one.name())
            .bind(slot.at + k as i32)
            .bind(slot.group)
            .bind(one.max_attempts())
            .execute(&mut *tx)
            .await?;
        }
    }

    tx.commit().await?;
    Ok(id)
}

#[derive(sqlx::FromRow)]
struct StepRow {
    state: String,
    attempts: i32,
    max_attempts: i32,
    output: Option<Value>,
    compensate_input: Option<Value>,
}

struct Head {
    state: State,
    input: Value,
    output: Value,
    failure: Option<String>,
}

/// Invokes what has not run, then unwinds if something refuses to.
async fn drive(
    pool: &PgPool,
    ctx: &Ctx<'_>,
    workflow: &Workflow,
    id: WorkflowRunId,
) -> Result<Run> {
    let head = head(pool, ctx, id).await?;

    if head.state.settled() {
        return Ok(Run {
            id,
            state: head.state,
            output: head.output,
            failure: head.failure,
        });
    }

    if head.state == State::Compensating {
        let why = head
            .failure
            .unwrap_or_else(|| "the run was already unwinding".into());
        return unwind(pool, ctx, workflow, id, why).await;
    }

    let mut carried = head.input;

    for slot in workflow.plan() {
        let rows = step_rows(pool, ctx, id, slot.at, slot.steps.len()).await?;

        let mut outputs = vec![Value::Null; slot.steps.len()];
        let mut waiting = Vec::new();
        for (k, row) in rows.iter().enumerate() {
            if row.state == "done" {
                outputs[k] = row.output.clone().unwrap_or(Value::Null);
            } else {
                waiting.push(k);
            }
        }

        if !waiting.is_empty() {
            let running = waiting
                .iter()
                .map(|&k| {
                    invoke(
                        pool,
                        ctx,
                        id,
                        slot.at + k as i32,
                        slot.steps[k].as_ref(),
                        &carried,
                        &rows[k],
                    )
                })
                .collect::<Vec<_>>();

            let mut failed: Option<(&'static str, Failure)> = None;
            for (&k, result) in waiting.iter().zip(all(running).await) {
                match result {
                    Ok(output) => outputs[k] = output,
                    Err(failure) => {
                        if failed.is_none() {
                            failed = Some((slot.steps[k].name(), failure));
                        }
                    }
                }
            }

            if let Some((name, failure)) = failed {
                let why = format!("{name}: {}", failure.error().report());
                return unwind(pool, ctx, workflow, id, why).await;
            }
        }

        carried = if outputs.len() == 1 {
            outputs.into_iter().next().unwrap_or(Value::Null)
        } else {
            Value::Array(outputs)
        };
    }

    finish(pool, ctx, id, State::Done, Some(carried.clone()), None).await?;
    Ok(Run {
        id,
        state: State::Done,
        output: carried,
        failure: None,
    })
}

/// Runs every one of them at once. A parallel set holds a handful of steps, so
/// waking all of them to find the one that moved costs less than it saves.
async fn all<F: Future>(futures: Vec<F>) -> Vec<F::Output> {
    let mut futures: Vec<Pin<Box<F>>> = futures.into_iter().map(Box::pin).collect();
    let mut done: Vec<Option<F::Output>> = futures.iter().map(|_| None).collect();

    std::future::poll_fn(|cx| {
        let mut pending = false;
        for (slot, future) in done.iter_mut().zip(futures.iter_mut()) {
            if slot.is_some() {
                continue;
            }
            match future.as_mut().poll(cx) {
                Poll::Ready(value) => *slot = Some(value),
                Poll::Pending => pending = true,
            }
        }
        if pending {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
    .await;

    done.into_iter().flatten().collect()
}

async fn head(pool: &PgPool, ctx: &Ctx<'_>, id: WorkflowRunId) -> Result<Head> {
    let mut tx = scoped(pool, ctx).await?;
    let row: Option<(String, Value, Option<Value>, Option<String>)> =
        sqlx::query_as("select state, input, output, failure from workflow_run where id = $1")
            .bind(id.as_uuid())
            .fetch_optional(&mut *tx)
            .await?;
    tx.commit().await?;

    let (state, input, output, failure) = row.ok_or_else(|| Error::not_found("workflow run"))?;
    Ok(Head {
        state: State::parse(&state)?,
        input,
        output: output.unwrap_or(Value::Null),
        failure,
    })
}

async fn step_rows(
    pool: &PgPool,
    ctx: &Ctx<'_>,
    id: WorkflowRunId,
    at: i32,
    count: usize,
) -> Result<Vec<StepRow>> {
    let mut tx = scoped(pool, ctx).await?;
    let rows: Vec<StepRow> = sqlx::query_as(
        "select state, attempts, max_attempts, output, compensate_input
         from workflow_step
         where run_id = $1 and ordering >= $2 and ordering < $3
         order by ordering",
    )
    .bind(id.as_uuid())
    .bind(at)
    .bind(at + count as i32)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    if rows.len() != count {
        return Err(Error::bug("a workflow lost a step it wrote"));
    }
    Ok(rows)
}

async fn step_row(pool: &PgPool, ctx: &Ctx<'_>, id: WorkflowRunId, at: i32) -> Result<StepRow> {
    step_rows(pool, ctx, id, at, 1)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| Error::bug("a workflow lost a step it wrote"))
}

/// Runs one step, retrying while it says to and the ceiling allows.
async fn invoke(
    pool: &PgPool,
    ctx: &Ctx<'_>,
    id: WorkflowRunId,
    at: i32,
    step: &dyn Step,
    input: &Value,
    row: &StepRow,
) -> std::result::Result<Value, Failure> {
    let mut attempts = row.attempts;

    if attempts >= row.max_attempts {
        return Err(Failure::Final(Error::conflict(
            "this step has already used every attempt it had",
        )));
    }

    loop {
        attempts += 1;
        let leased = Utc::now() + LEASE;

        let mut tx = scoped(pool, ctx).await.map_err(Failure::Final)?;

        sqlx::query(
            "update workflow_step
             set state = 'invoking', attempts = $3, lease_until = $4
             where run_id = $1 and ordering = $2",
        )
        .bind(id.as_uuid())
        .bind(at)
        .bind(attempts)
        .bind(leased)
        .execute(&mut *tx)
        .await
        .map_err(|err| Failure::Final(Error::from(err)))?;

        match step.invoke(&mut tx, ctx, input).await {
            Ok(outcome) => {
                sqlx::query(
                    "update workflow_step
                     set state = 'done', output = $3, compensate_input = $4, lease_until = null
                     where run_id = $1 and ordering = $2",
                )
                .bind(id.as_uuid())
                .bind(at)
                .bind(&outcome.output)
                .bind(&outcome.compensate_input)
                .execute(&mut *tx)
                .await
                .map_err(|err| Failure::Final(Error::from(err)))?;

                tx.commit()
                    .await
                    .map_err(|err| Failure::Final(Error::from(err)))?;
                return Ok(outcome.output);
            }
            Err(failure) => {
                drop(tx);

                if failure.retryable() && attempts < row.max_attempts {
                    backoff(attempts).await;
                    continue;
                }

                let message = match &failure {
                    Failure::Retry(err) | Failure::Final(err) => err.report(),
                };

                let mut tx = scoped(pool, ctx).await.map_err(Failure::Final)?;
                let _ = sqlx::query(
                    "update workflow_step
                     set state = 'failed', attempts = $3, failure = $4, lease_until = null
                     where run_id = $1 and ordering = $2",
                )
                .bind(id.as_uuid())
                .bind(at)
                .bind(attempts)
                .bind(&message)
                .execute(&mut *tx)
                .await;
                let _ = tx.commit().await;

                return Err(failure);
            }
        }
    }
}

async fn backoff(attempt: i32) {
    let millis = 100u64.saturating_mul(1 << attempt.clamp(0, 6) as u64);
    tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
}

/// Walks back through the steps that finished, undoing each. Latest first, one
/// at a time, whether or not they were started together.
async fn unwind(
    pool: &PgPool,
    ctx: &Ctx<'_>,
    workflow: &Workflow,
    id: WorkflowRunId,
    failure: String,
) -> Result<Run> {
    set_state(pool, ctx, id, State::Compensating, Some(&failure)).await?;

    let mut ok = true;
    let mut at = workflow.slots.iter().map(Vec::len).sum::<usize>() as i32;

    for slot in workflow.plan().into_iter().rev() {
        for one in slot.steps.iter().rev() {
            at -= 1;
            let row = step_row(pool, ctx, id, at).await?;
            if row.state != "done" {
                continue;
            }

            let kept = row.compensate_input.unwrap_or(Value::Null);
            let mut tx = scoped(pool, ctx).await?;

            match one.compensate(&mut tx, ctx, &kept).await {
                Ok(()) => {
                    sqlx::query(
                        "update workflow_step set state = 'reverted' where run_id = $1 and ordering = $2",
                    )
                    .bind(id.as_uuid())
                    .bind(at)
                    .execute(&mut *tx)
                    .await?;
                    tx.commit().await?;
                }
                Err(err) => {
                    drop(tx);
                    ok = false;
                    dead_letter(pool, ctx, id, one.name(), &err.report()).await?;
                }
            }
        }
    }

    let state = if ok { State::Reverted } else { State::Failed };
    finish(pool, ctx, id, state, None, Some(failure.clone())).await?;

    Ok(Run {
        id,
        state,
        output: Value::Null,
        failure: Some(failure),
    })
}

async fn dead_letter(
    pool: &PgPool,
    ctx: &Ctx<'_>,
    id: WorkflowRunId,
    step: &str,
    failure: &str,
) -> Result<()> {
    let mut tx = scoped(pool, ctx).await?;
    sqlx::query(
        "insert into workflow_dead_letter (id, scope, run_id, step_name, failure, state)
         values ($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(step)
    .bind(failure)
    .bind(Value::Null)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn set_state(
    pool: &PgPool,
    ctx: &Ctx<'_>,
    id: WorkflowRunId,
    state: State,
    failure: Option<&str>,
) -> Result<()> {
    let mut tx = scoped(pool, ctx).await?;
    sqlx::query(
        "update workflow_run set state = $2, failure = coalesce($3::text, failure) where id = $1",
    )
    .bind(id.as_uuid())
    .bind(state.as_str())
    .bind(failure)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn finish(
    pool: &PgPool,
    ctx: &Ctx<'_>,
    id: WorkflowRunId,
    state: State,
    output: Option<Value>,
    failure: Option<String>,
) -> Result<()> {
    let mut tx = scoped(pool, ctx).await?;
    sqlx::query(
        "update workflow_run
         set state = $2, output = $3, failure = $4, finished_at = now()
         where id = $1",
    )
    .bind(id.as_uuid())
    .bind(state.as_str())
    .bind(output)
    .bind(failure)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Reads a run back, for a caller that wants to know how one it started ended.
pub async fn get(pool: &PgPool, ctx: &Ctx<'_>, id: WorkflowRunId) -> Result<Run> {
    let head = head(pool, ctx, id).await?;
    Ok(Run {
        id,
        state: head.state,
        output: head.output,
        failure: head.failure,
    })
}

/// Puts back steps whose worker died holding them, so another may take over.
///
/// A lease that ran out means the process invoking is gone, not that the step
/// is slow: a step still working extends its own.
pub async fn recover(pool: &PgPool, ctx: &Ctx<'_>) -> Result<u64> {
    let mut tx = scoped(pool, ctx).await?;
    let done = sqlx::query(
        "update workflow_step
         set state = 'pending', lease_until = null, locked_by = null
         where state = 'invoking' and lease_until < now()",
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(done)
}

/// Extends a claim, for a step that is still working.
pub async fn extend(
    tx: &mut Tx<'_>,
    id: WorkflowRunId,
    at: i32,
    until: DateTime<Utc>,
) -> Result<()> {
    sqlx::query("update workflow_step set lease_until = $3 where run_id = $1 and ordering = $2")
        .bind(id.as_uuid())
        .bind(at)
        .bind(until)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Takes runs that are waiting and drives them, until told to stop.
///
/// A run is claimed by leasing its due steps: another worker asking a moment
/// later finds nothing unleased belonging to it and looks elsewhere. The lease
/// is extended while the run is being driven, so it means "somebody is holding
/// this" rather than "this is slow".
///
/// Returns when `shutdown` resolves, after whatever run is in hand has
/// finished. Only the workflows given can be picked up; a run of anything else
/// is left for the worker that knows it.
pub async fn work(
    pool: &PgPool,
    ctx: &Ctx<'_>,
    workflows: &[&Workflow],
    shutdown: impl Future<Output = ()> + Send,
) -> Result<()> {
    let names: Vec<String> = workflows.iter().map(|w| w.name().to_string()).collect();
    let mut shutdown = std::pin::pin!(shutdown);

    loop {
        let stopping = tokio::select! {
            biased;
            _ = &mut shutdown => true,
            _ = std::future::ready(()) => false,
        };
        if stopping {
            return Ok(());
        }

        recover(pool, ctx).await?;

        match take(pool, ctx, &names).await? {
            Some(id) => {
                let name = run_name(pool, ctx, id).await?;
                let Some(workflow) = workflows.iter().copied().find(|w| w.name() == name) else {
                    continue;
                };
                held(pool, ctx, workflow, id).await?;
            }
            None => {
                tokio::select! {
                    _ = &mut shutdown => return Ok(()),
                    _ = tokio::time::sleep(IDLE) => {}
                }
            }
        }
    }
}

/// Drives one run, keeping its lease alive for as long as that takes.
async fn held(pool: &PgPool, ctx: &Ctx<'_>, workflow: &Workflow, id: WorkflowRunId) -> Result<()> {
    let mut driving = std::pin::pin!(drive(pool, ctx, workflow, id));

    let beat = std::time::Duration::from_millis((LEASE.num_milliseconds() / 3).max(1) as u64);

    loop {
        tokio::select! {
            outcome = &mut driving => {
                outcome?;
                return Ok(());
            }
            _ = tokio::time::sleep(beat) => {
                touch(pool, ctx, id).await?;
            }
        }
    }
}

/// Leases every due step of one waiting run, which is what claiming a run is.
async fn take(pool: &PgPool, ctx: &Ctx<'_>, names: &[String]) -> Result<Option<WorkflowRunId>> {
    let mut tx = scoped(pool, ctx).await?;

    let claimed: Option<Uuid> = sqlx::query_scalar(
        "with candidate as (
             select r.id
             from workflow_run r
             where r.state in ('running', 'compensating')
               and r.name = any($1::text[])
               and exists (
                   select 1 from workflow_step s
                   where s.run_id = r.id
                     and s.state in ('pending', 'compensating')
                     and s.run_after <= now()
                     and (s.lease_until is null or s.lease_until < now())
               )
             order by r.created_at
             limit 1
         ),
         taken as (
             select s.id
             from workflow_step s
             join candidate c on c.id = s.run_id
             where s.state in ('pending', 'compensating')
             for update of s skip locked
         ),
         leased as (
             update workflow_step s
             set lease_until = $2, locked_by = $3
             from taken t
             where s.id = t.id
             returning s.run_id
         )
         select distinct run_id from leased",
    )
    .bind(names)
    .bind(Utc::now() + LEASE)
    .bind(Uuid::now_v7().to_string())
    .fetch_optional(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(claimed.map(WorkflowRunId::from_uuid))
}

async fn run_name(pool: &PgPool, ctx: &Ctx<'_>, id: WorkflowRunId) -> Result<String> {
    let mut tx = scoped(pool, ctx).await?;
    let name: Option<String> = sqlx::query_scalar("select name from workflow_run where id = $1")
        .bind(id.as_uuid())
        .fetch_optional(&mut *tx)
        .await?;
    tx.commit().await?;
    name.ok_or_else(|| Error::not_found("workflow run"))
}

/// Pushes the lease out for everything this run is holding.
async fn touch(pool: &PgPool, ctx: &Ctx<'_>, id: WorkflowRunId) -> Result<()> {
    let mut tx = scoped(pool, ctx).await?;
    sqlx::query(
        "update workflow_step set lease_until = $2
         where run_id = $1 and state in ('pending', 'invoking', 'compensating')",
    )
    .bind(id.as_uuid())
    .bind(Utc::now() + LEASE)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

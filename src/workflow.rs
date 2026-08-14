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

use std::sync::Arc;

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

pub struct Workflow {
    name: &'static str,
    steps: Vec<Arc<dyn Step>>,
}

impl std::fmt::Debug for Workflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Workflow")
            .field("name", &self.name)
            .field(
                "steps",
                &self.steps.iter().map(|s| s.name()).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Workflow {
    pub fn new(name: &'static str) -> Self {
        Workflow {
            name,
            steps: Vec::new(),
        }
    }

    pub fn then(mut self, step: impl Step + 'static) -> Self {
        self.steps.push(Arc::new(step));
        self
    }

    pub fn name(&self) -> &'static str {
        self.name
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
    let id = claim(pool, ctx, workflow, transaction_key, input).await?;
    drive(pool, ctx, workflow, id).await
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

    for (at, step) in workflow.steps.iter().enumerate() {
        sqlx::query(
            "insert into workflow_step (id, scope, run_id, name, ordering, max_attempts)
             values ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::now_v7())
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .bind(step.name())
        .bind(at as i32)
        .bind(step.max_attempts())
        .execute(&mut *tx)
        .await?;
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

/// Invokes what has not run, then unwinds if something refuses to.
async fn drive(
    pool: &PgPool,
    ctx: &Ctx<'_>,
    workflow: &Workflow,
    id: WorkflowRunId,
) -> Result<Run> {
    let mut carried = run_input(pool, ctx, id).await?;

    for (at, step) in workflow.steps.iter().enumerate() {
        let row = step_row(pool, ctx, id, at as i32).await?;

        if row.state == "done" {
            carried = row.output.unwrap_or(Value::Null);
            continue;
        }

        match invoke(pool, ctx, id, at as i32, step.as_ref(), &carried, &row).await {
            Ok(outcome) => carried = outcome,
            Err(failure) => {
                let message = failure.error().to_string();
                return unwind(pool, ctx, workflow, id, step.name(), message).await;
            }
        }
    }

    finish(pool, ctx, id, State::Done, Some(carried.clone()), None).await?;
    Ok(Run {
        id,
        state: State::Done,
        output: carried,
        failure: None,
    })
}

async fn run_input(pool: &PgPool, ctx: &Ctx<'_>, id: WorkflowRunId) -> Result<Value> {
    let mut tx = scoped(pool, ctx).await?;
    let input: Value = sqlx::query_scalar("select input from workflow_run where id = $1")
        .bind(id.as_uuid())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| Error::not_found("workflow run"))?;
    tx.commit().await?;
    Ok(input)
}

async fn step_row(pool: &PgPool, ctx: &Ctx<'_>, id: WorkflowRunId, at: i32) -> Result<StepRow> {
    let mut tx = scoped(pool, ctx).await?;
    let row: StepRow = sqlx::query_as(
        "select state, attempts, max_attempts, output, compensate_input
         from workflow_step where run_id = $1 and ordering = $2",
    )
    .bind(id.as_uuid())
    .bind(at)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| Error::bug("a workflow lost a step it wrote"))?;
    tx.commit().await?;
    Ok(row)
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
                    Failure::Retry(err) | Failure::Final(err) => err.to_string(),
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

/// Walks back through the steps that finished, undoing each.
async fn unwind(
    pool: &PgPool,
    ctx: &Ctx<'_>,
    workflow: &Workflow,
    id: WorkflowRunId,
    failed_at: &str,
    failure: String,
) -> Result<Run> {
    set_state(pool, ctx, id, State::Compensating).await?;

    let mut ok = true;

    for (at, step) in workflow.steps.iter().enumerate().rev() {
        let row = step_row(pool, ctx, id, at as i32).await?;
        if row.state != "done" {
            continue;
        }

        let kept = row.compensate_input.unwrap_or(Value::Null);
        let mut tx = scoped(pool, ctx).await?;

        match step.compensate(&mut tx, ctx, &kept).await {
            Ok(()) => {
                sqlx::query(
                    "update workflow_step set state = 'reverted' where run_id = $1 and ordering = $2",
                )
                .bind(id.as_uuid())
                .bind(at as i32)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
            }
            Err(err) => {
                drop(tx);
                ok = false;
                dead_letter(pool, ctx, id, step.name(), &err.to_string()).await?;
            }
        }
    }

    let state = if ok { State::Reverted } else { State::Failed };
    let message = format!("{failed_at}: {failure}");
    finish(pool, ctx, id, state, None, Some(message.clone())).await?;

    Ok(Run {
        id,
        state,
        output: Value::Null,
        failure: Some(message),
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

async fn set_state(pool: &PgPool, ctx: &Ctx<'_>, id: WorkflowRunId, state: State) -> Result<()> {
    let mut tx = scoped(pool, ctx).await?;
    sqlx::query("update workflow_run set state = $2 where id = $1")
        .bind(id.as_uuid())
        .bind(state.as_str())
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
    let mut tx = scoped(pool, ctx).await?;
    let row: Option<(String, Option<Value>, Option<String>)> =
        sqlx::query_as("select state, output, failure from workflow_run where id = $1")
            .bind(id.as_uuid())
            .fetch_optional(&mut *tx)
            .await?;
    tx.commit().await?;

    let (state, output, failure) = row.ok_or_else(|| Error::not_found("workflow run"))?;
    Ok(Run {
        id,
        state: State::parse(&state)?,
        output: output.unwrap_or(Value::Null),
        failure,
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
         set state = 'pending', lease_until = null
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

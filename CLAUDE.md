# Working in tezgah

A commerce engine as a Rust library. Read `README.md` first — it carries the
decisions and the reasons, and this file does not repeat them.

## This repository is public

Everything here is readable by anyone, forever: code, comments, tests, commit
messages, fixtures. Nothing about whoever runs it goes in. No customer names,
addresses, hostnames or e-mails; nothing out of a live database; no
credentials; no server addresses. Test data uses the reserved domains —
`example.com`, `example.test`, `example.invalid` — and nothing else.

tezgah is a library and does not know who is using it. Keep it that way: no
host's name in the code, and no feature shaped for one caller.

## Nothing is built or tested on the machine this was written on

That machine serves other people's sites and a build taking every core has
taken it off the air before. Do not run `cargo build`, `cargo test`,
`cargo nextest run`, `cargo clippy` or `cargo check` there. Branch, write, commit, open the pull
request, and read what CI says. `.github/workflows/ci.yml` runs the formatter,
clippy with warnings denied, the tests against a real Postgres, the doctests,
a dependency audit and a secret scan.

## What the code has to keep true

**Ports ask, they do not answer.** `src/ports.rs` is the whole of what tezgah
wants from a host. Adding a trait there is a real decision — it becomes work
for everybody embedding this. Prefer a parameter.

**Every public function that reaches data asks first.** `ctx.permit(..)` puts
the question to the host's `Authorizer`, and a denial is an error rather than a
`false`. This is a convention with a test behind it rather than a type the
compiler makes you carry: no function takes a `Permit` as a parameter, and
`tests/permit_asked.rs` reads `src/` and fails when a public function runs a
query with no `ctx.permit(..)` above it and no reason in its `TOLERATED` list.
If a code path does not need permission, say so there, where a reader can see
it.

"Public" there means reachable from outside the crate: `pub`, not
`pub(crate)`. A host never calls a `pub(crate)` function directly, so asking
it to hold a permit is asking twice — the crate-external entry point it sits
behind already asked. Keep it `pub(crate)` on purpose, not `pub`, if the only
reason it is visible past its module is another function in this crate.

**A workflow step can say it had nothing to do.** `workflow_step.state` permits
`'skipped'` for exactly this: `Outcome::skipped(output)` carries the step's
input forward the way `Outcome::new` does, records `state = 'skipped'`
instead of `'done'`, and the run does not call that step's `compensate` when
it later unwinds — a skipped step wrote nothing, so there is nothing to undo.
Reach for it only when a step's behaviour is genuinely conditional — spending
credit a cart does not have, authorizing a charge for nothing once credit
covered the total — not as a way to make a step's return type more
interesting.

**Audit rows, events and jobs are written in the caller's transaction.** Never
after the commit. A change that rolls back takes them with it, and an event
that was never delivered is still in the outbox to deliver.

**Money is `Money`.** No `f64`, no minor-unit integers, no multiplying by a
hundred. An allocation across lines must add back up to the whole, and there
is a test that says so.

**Every table carries a scope and has row-level security forced on.** Not
enabled — forced, so a table owner does not bypass it. A migration adding a
table without both fails the schema test.

**Amounts, quantities and state transitions belong to the database too.** Check
constraints, not comments. Two writers always turn up.

**A migration is append-only, so a bug in one cannot be edited away — but it
can be corrected.** `tests/migration_dml.rs` reads a migration's own text, and
a bad backfill sits there forever even after a later migration fixes the rows
it left wrong. Its `TOLERATED` list distinguishes the two: an entry names the
migration that corrected it, and the test checks that migration is still in
the tree. A hole nobody has dealt with yet cannot cite one — that is what
keeps the list from becoming a place fixed bugs go to be remembered as open
ones.

**A migration that indexes a table already carrying rows takes its lock for
the duration, and sqlx can avoid that.** #172 measured it: sqlx 0.8 honours a
literal `-- no-transaction` first line by running that migration outside a
transaction, which is what `create index concurrently` requires — Postgres
refuses it inside one. So a new migration that adds an index to a table that
existed before it opens with `-- no-transaction` and builds the index
`concurrently`, and — because that leaves the migration non-atomic — drops an
invalid index of the same name before creating it, so a failed attempt leaves
a retry clean rather than an operator's problem. A migration creating the
table in the same file needs none of this — a table with no rows costs
nothing to lock. `tests/migration_lock.rs` mechanically checks the first two
(the header and `concurrently` both have to be there); the defensive drop is
the same discipline `tezgah_fk` already asks of `not valid` constraints and is
reviewed by eye, not by the test. Its `TOLERATED` list is the 23 (migration,
table) pairs that indexed an existing table before the rule existed, across
19 migrations, and may only shrink — never grow for a migration written after
#172 landed.

## Mistakes this codebase has made more than once

Every one of these was found by running the code or counting its callers, never
by reading it and finding it wrong. They are written down because each has
recurred, and because each is invisible in review.

**A number scoped to a part, used against the whole.** A capture's slice and the
order's total in one expression; a fixed fee clamped against one line when it is
defined against the order. Identical whenever there is exactly one part, wrong
the moment there are two — so every test that captures in full passes. When an
expression mixes two totals, ask which one each came from.

**Written, tested, and reachable from nothing.** Five features shipped this way:
correct modules with no route, no caller, and no way for a shop to touch them.
`tests/reachable.rs` catches it now. A domain function without a route is not
finished, and neither is a table only tests write to.

**A constraint left out because a fixture could not satisfy it.** Twice. The
constraint is usually right and the fixture is usually wrong — teach
`tests/isolation.rs`'s seeder, then put the rule back in a corrective migration.
A rule enforced only in Rust holds until the second writer turns up, and this is
a ledger.

**A row copied by naming its columns.** The cart merge silently dropped three
fields that way, each added later by somebody who updated every writer and never
found the one place that copies. Copy the row — `insert … select`, naming only
what genuinely changes.

**One fact with two answers.** The current order version read two ways in one
transaction; the payout ledger with two write paths; four private digests that
disagreed about case. Nothing keeps them in step, so they agree until they do
not. Pick the source and have the other read it.

**A comment promising to come back.** `reservation_item.line_item_id` carried
"this cannot be a foreign key yet" for sixty migrations, and expired carts held
stock forever behind it. A deferred constraint is an issue, not a comment.

**A partial unique index whose predicate is not repeated in `on conflict`.**
Postgres refuses the whole statement, and it has broken checkout twice.

**An error that tells you what a permission would have hidden.** A missing row
answered `not_found` without asking anybody, while a row that existed and was
not yours asked and answered `denied` — so the pair told a stranger which ids
exist. Eighty-nine routes did this. Ids here are uuidv7 and carry a timestamp,
so an oracle over them leaks when a shop trades, not just whether. Ask before
you answer, on the branch where there is nothing to answer about.

**A migration's text read as if it were the schema.** Migrations are
append-only, so a `create table` block keeps saying `references product (id)`
long after a later migration converted that key through `tezgah_fk`. Grepping
the corpus produced a list of 44 "unswept" keys; measuring the built schema
found none. If the question is what the database looks like, apply the
migrations and ask the catalogue.

**Documentation asserting the opposite of the code.** The README told readers
four features were deliberately absent that had already landed; the roadmap
blamed closed issues for gaps they closed. Verify a claim about behaviour
against the behaviour, not against the issue that once described it.

## Tests

CI runs them with `cargo nextest run --profile ci`, one process per test, with
a test group holding the database-backed ones to eight at a time — each holds
up to ten Postgres connections. `.config/nextest.toml` carries the arithmetic.
nextest does not run doctests, so `cargo test --doc` is a separate step and is
not redundant.

The tests that check a rule against every table — `tests/schema.rs`,
`tests/isolation.rs` — read the catalogue rather than a list somebody keeps up
to date, so a table added tomorrow is covered the day it is added.

Against a real Postgres, version 18. Never SQLite: the same query returns
different things under each, and the difference shows up in production rather
than in CI. 18 is also the floor the README states, and for the same reason it
is stated rather than assumed — nothing in `migrations/` needs a feature newer
than 15, but 18 is the only version anything runs against, and a version nobody
tests is a version nobody supports.

Concurrency claims are tested concurrently — two connections, at the same time.
A test that simulates a race by doing one thing after another proves nothing.

## Payments belong to kasapay

Providers are not tezgah's to write. [kasapay](https://github.com/productdevbook/kasapay)
is one payment API in Rust over any provider — Stripe and iyzico today, the rest
the same shape — and tezgah is a consumer of it.

So: a provider bug, a missing capability, a new provider, anything about how a
payment is taken — **open it on kasapay**, not here. What belongs here is the
mapping onto its `Provider` trait, and what tezgah does with the answer: the
collection, the ledger, the webhook table that makes a redelivery land once.

A capability a provider may or may not have arrives as an extension trait —
`trait RecurringProvider: PaymentProvider` — never as a method on
`PaymentProvider` itself, which every implementor would have to grow. A provider
that does not implement it cannot sell the thing that needs it, and says so at
compile time rather than at the till.

`src/providers/` used to hold tezgah's own hosted-flow adapters for Stripe and
iyzico, written before kasapay existed — #53 deleted both once kasapay's own
gaps closed under them. What is left is the mapping onto `Provider`, kept
ready for whichever host wires it up; nothing here should grow back into a
second payment abstraction.

## Commits and pull requests

Conventional Commits. The subject is `<type>(<scope>): <summary>`, lower case,
imperative, no full stop, under 72 characters.

Types: `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `build`, `ci`,
`chore`, `revert`. The scope is the domain or module the change lands in —
`order`, `cart`, `workflow`, `schema`, `ports` — and is left out only when the
change is genuinely across the whole crate.

    feat(inventory): reserve stock without decrementing it
    fix(cart): stop a second add from leaving two rows for one variant
    test(workflow): interrupt a checkout at every step in turn

A breaking change is marked `feat(order)!: ...` and explained in the body under
`BREAKING CHANGE:`.

The body is where the reasoning goes: what was wrong, what it does now, and why
that way. A pull request title follows the same rule, because it becomes the
squashed commit.

## Comments

Default: don't. Write a comment only for something the code cannot say —
a constraint that reads as wrong until explained, an outside system's odd
behaviour, or why a choice was made rather than what it does. One line. If it
is longer than the code, or the code already says it, delete it. What changed
belongs in the commit message.

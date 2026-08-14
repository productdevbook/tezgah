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

## Tests

CI runs them with `cargo nextest run --profile ci`, one process per test, with
a test group holding the database-backed ones to eight at a time — each holds
up to ten Postgres connections. `.config/nextest.toml` carries the arithmetic.
nextest does not run doctests, so `cargo test --doc` is a separate step and is
not redundant.

The tests that check a rule against every table — `tests/schema.rs`,
`tests/isolation.rs` — read the catalogue rather than a list somebody keeps up
to date, so a table added tomorrow is covered the day it is added.

Against a real Postgres. Never SQLite: the same query returns different things
under each, and the difference shows up in production rather than in CI.

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

`src/providers/` predates that repository and is on its way out; see the issue
tracking it.

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

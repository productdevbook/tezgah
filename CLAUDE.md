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
`cargo clippy` or `cargo check` there. Branch, write, commit, open the pull
request, and read what CI says. `.github/workflows/ci.yml` runs the formatter,
clippy with warnings denied, the tests against a real Postgres, the doctests,
a dependency audit and a secret scan.

## What the code has to keep true

**Ports ask, they do not answer.** `src/ports.rs` is the whole of what tezgah
wants from a host. Adding a trait there is a real decision — it becomes work
for everybody embedding this. Prefer a parameter.

**Nothing reaches data without a `Permit`.** A repository call takes one, and
the only way to get one is to have asked the host's `Authorizer`. If a code
path does not need permission, say so where a reader can see it.

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

Against a real Postgres. Never SQLite: the same query returns different things
under each, and the difference shows up in production rather than in CI.

Concurrency claims are tested concurrently — two connections, at the same time.
A test that simulates a race by doing one thing after another proves nothing.

## Comments

Default: don't. Write a comment only for something the code cannot say —
a constraint that reads as wrong until explained, an outside system's odd
behaviour, or why a choice was made rather than what it does. One line. If it
is longer than the code, or the code already says it, delete it. What changed
belongs in the commit message.

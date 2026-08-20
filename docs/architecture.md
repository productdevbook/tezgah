# Architecture

`README.md` says what tezgah does. This says how it is arranged, which layer
is allowed to know about which, and — measured rather than asserted — where
the arrangement is not finished yet.

## Three shapes, one engine

tezgah is a library. That is not a modesty about scope; it is the decision
everything else follows from, and it exists because the same commerce engine
has to serve three quite different callers.

**A host that already is an application.** A CMS, an ERP, a marketplace
somebody already runs. It has accounts, roles, file storage, a mailer and a
worker before it has a shop, and it wants commerce *inside* its own
transaction — an order and whatever else that request wrote committing
together. It supplies the five ports out of things it already owns, and often
runs many shops on one deployment, so `Scope` varies per request.

**One shop, self-hosted.** Somebody who wants a commerce backend, not a
library to embed in something. They run two containers and get a panel and an
API. There is one `Scope` and it never changes, nobody has an application to
plug the ports into, and everything a host is expected to supply has to be
supplied by what this repository ships. That is [`app/`](../app), and it is
the product this repository is responsible for.

**A worked example.** [`examples/shop`](../examples/shop) — the library
called directly from a `main`, no framework, small enough to read in one
sitting. Not a product; the smallest honest demonstration of what embedding
is.

The three are the same crate. What separates them is who answers the five
questions in [`src/ports.rs`](../src/ports.rs).

## The layers, and what may not leak

    src/            the library — domains, the route table, the workflow runner
    migrations/     the tables it owns, in your database
    app/server/     one binary over it: axum, config, the five ports answered
    app/client/     one panel over that binary's HTTP
    examples/shop/  the same library with no framework at all

The rule that holds all of it together is one sentence: **`src/` may not know
that `app/` exists.** No feature shaped for one caller, no host's name in the
code, no `if self_hosted`. When the app needs something the library will not
give it, the answer is a port or a parameter — never a special case.

It runs the other way too, and is easier to get wrong: **`app/` may not
reimplement what `src/` owns.** A total computed in a handler, a status
transition decided in TypeScript, a list the panel sorts because the API will
not — each is a second answer to a question the database already answers, and
this codebase has a written record of what happens when one fact has two
answers.

`app/server` and `app/client` are one product in two processes. They ship as
two images and are useful only together: the panel talks to no other API, and
the binary serves an admin surface nothing else draws.

## What each shape supplies

The five ports are in [`src/ports.rs`](../src/ports.rs) and
[`docs/hosting.md`](hosting.md) says what each one obliges you to. Who answers
them is the whole difference between the shapes:

| Port | An application embedding it | `app/` — one shop, self-hosted |
|---|---|---|
| `Authorizer` | its own role engine | grants everything; accounts and sessions stand in front |
| `AuditSink` | its own audit log | a JSON line on stdout |
| `EventSink` | its own bus or outbox | a JSON line on stdout |
| `Jobs` | its own queue and workers | `server_job`, claimed by a worker that dispatches nothing |
| `Clock` | its own | `Utc::now()` |

The right-hand column is the honest shape of the self-hosted product today,
and the next section is the rest of it. Only the first row has a person behind
it; the other four are still a JSON line on stdout, a table nothing dispatches
from, and the system clock.

## Where this arrangement is not finished

Everything below was measured against the tree rather than remembered, and
each says which layer owns the fix. None of it is a hole in the commerce
domain — [`GOAL.md`](../GOAL.md) tracks that sweep separately, and it is done.
These are the platform gaps: the things that are nobody's problem while tezgah
is a library, and become the product's problem the moment this repository
ships a shop somebody else runs.

### The library

**A list cannot be searched, filtered or sorted.** `Paging` carries a cursor
and a limit and nothing else; there is exactly one filter type in 62,000 lines
(`catalogue::ProductFilter`); every paged query in the crate ends `order by
created_at`; and `ilike`, `to_tsvector` and `pg_trgm` appear nowhere in `src/`
or `migrations/`. So an operator with forty thousand orders cannot find one by
e-mail, and a panel that offered a sortable column would be claiming something
about the pages that are not on screen. This is the single largest reason the
panel cannot be as capable as an established platform's, and it cannot be
fixed in the panel.

**`Page<T>` carries no count.** `items` and `next`, by design — a cursor page
does not know how many rows are behind it. It is the right shape for paging
and the wrong shape for a back office, which wants to say "1–50 of 41,309".
A count is a second, cheaper question (`count(*)` under the same filter,
answered once and cached by the caller), not a change to how paging works.

**No route accepts a provider's callback.** `payment::record_webhook` is
written, tested, and stores `(provider, event_id)` uniquely so a redelivery
lands once — and nothing in `src/api/` declares a path for it. Any payment
confirmed asynchronously (3-D Secure, a hosted form, a bank transfer) has
nowhere to be confirmed *to*. The library should declare the route; the host
supplies the signature check, because the secret is the host's.

**A resource whose owner is discovered by loading it is judged after the
load.** 89 routes answer `not_found` to a caller who would have been denied,
because the permission depends on a `customer_id` only the row carries. Ids
here are uuidv7 and carry a timestamp, so the pair leaks when a shop trades.
Filed as #151 and #152; it is a `ports.rs`-level decision, not a patch.

### The self-hosted app

**There are accounts, and no way to invite anybody to one.** Operators,
argon2id passwords and sessions that expire live in `app/server/src/identity.rs`,
and `ADMIN_TOKEN` stays beside them as what it always was — the way to make
the first account and the way back in when the last password is lost.
`Actor::Staff` now carries a real id for a signed-in operator, so an audit row
can say who changed a price; for an `ADMIN_TOKEN` request it carries the nil
uuid, visibly, because a shared secret is not a person. What is still missing
is everything that needs a mailer: an invitation, a password reset, a
notification that an account was made. There is no mailer, and a reset link a
server cannot send is worse than one it never offered.

**Authentication, not authorization.** Whoever clears the gate reaches the
crate as `Actor::Staff`, and the app's `Authorizer` grants every `Action` to
it — `View` and `Write` and `Delete` alike. The seam for a split is already
there: `authorize` receives the `Action` on every call and the request now
carries who is asking. Nothing answers it yet.

**The sweeps run; the jobs still do not.** `cart::expire` and
`inventory::expire_reservations` are called every five minutes by
`app/server/src/schedule.rs` — before that they were called by tests and by
nothing else, so on the shipped image an abandoned cart was never cleared and
the stock it reserved was held for ever. What still does not run is the queue:
the crate enqueues exactly one job kind, a subscription's dunning retry, and
the worker claims it, prints it and marks it processed. So a declined renewal
is retried never.

**Events go to stdout.** No outbox, no subscriber, no delivery, no retry. A
shop that wants `order.paid` to reach its own systems, or to become an e-mail,
has nowhere to say so. A file store and a mailer are the same absence seen
from a different side: a product image can only be a URL somebody else hosts,
and nothing in the product can send a receipt.

**111 of 483 declared routes are bound.** The panel draws 228. The difference
is not a mistake — each binding is written by hand, deliberately — but it does
mean the panel and the binary disagree about what the product is, and the
number will not close by hand at that rate. Either the route table grows a
generated binding (it already carries surface, method, path and permission for
every operation; only the handler's signature is not uniform) or the gap is
permanent.

**Nothing observes it.** No tracing, no metrics, no request log, no readiness
distinct from liveness, no CORS policy and no rate limit. `println!` is the
whole of it. Serving somebody else's shop without any of that is not a
position to be in the first time it is slow.

### The panel

Measured: 16,451 lines of TSX across 67 screens, no locales, no form library.
An established platform's dashboard, for scale, is 128,585 lines and 34
locales. Line counts prove nothing on their own; what they line up with is
what is missing.

**No filtering, searching or sorting** — because the API offers none. This is
the library gap above, seen from the screen.

**Most forms are still hand-rolled.** `react-hook-form` and a zod resolver
arrived with the route-modal work, and one domain uses them; the rest still
carry a `useState`, a `safeParse` and a hand-built map of field errors each.
The schemas were always the expensive half and they were already generated.

**Translation exists and covers almost nothing.** English and Turkish, with
the compiler enforcing that the two dictionaries match, over the shared
chrome — actions, errors, the unsaved-changes prompt. Every screen's own
words are still English in the source.

**Nothing bulk.** No multi-select, no bulk edit grid, no import or export
screen — although `batch` is a domain here with three routed endpoints for
products, prices and stock. Nothing draws them.

**Mountable in what it says, not in how it routes.** No screen reaches for a
global any more: where the API is, what token to send, what to do when it is
refused and which language to draw in are a host's answers, which is what
lets an application embedding the library put these screens in its own back
office. What is still the standalone application's own is routing — file
routes under a fixed root, and a sidebar written as a switch over that closed
route union. A host mounting them under a path of its own needs a basepath
and a shell of its own, and that is the rest of the seam.

**A child route whose parent draws no outlet is a screen nothing can reach,
and nothing says so.** All five of the panel's "edit a record" screens were
in that state from the commit that added them: the route file was right, the
screen was right, the router reported a match, and the form never drew. The
router's own generated tree is what finds it — routes with children, checked
against whether that component renders an outlet — and it is worth a test
rather than a habit.

## What is deliberately not here

Naming a gap is a decision; so is refusing one. These are refusals, not
oversights:

- **A role system.** tezgah asks an `Authorizer`. A host has roles already,
  and a library that invents a second set makes every embedder reconcile two.
  The self-hosted app will need its own — that is the app's, and it will be
  the thing that *answers* the port rather than a thing inside the crate.
- **A search index.** A separate service to answer what a join can answer.
  Postgres can search this data; the gap above is that nothing asks it to.
- **A cache.** A library that caches behind a caller's back is a bug.
- **A plugin runtime.** This is Rust; extension is a dependency and a trait
  implementation, not a directory scanned at boot. What is fair to ask for is
  that a provider — payment, fulfilment, tax, and later files and mail — can
  be chosen by configuration rather than by recompiling, and that is the app's
  job to offer.

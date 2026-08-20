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
| `Authorizer` | its own role engine | three roles at the door; a signed-in shopper reaches only their own rows |
| `AuditSink` | its own audit log | a row, in the caller's transaction |
| `EventSink` | its own bus or outbox | an outbox row, posted to one URL and signed |
| `Jobs` | its own queue and workers | `server_job`, claimed and dispatched, with a backoff and a dead letter |
| `Clock` | its own | `Utc::now()` |

The right-hand column is the honest shape of the self-hosted product today,
and the next section is the rest of it. All five are answered now; what that
section measures is how much each answer covers.

## Where this arrangement is not finished

Everything below was measured against the tree rather than remembered, and
each says which layer owns the fix. None of it is a hole in the commerce
domain — [`GOAL.md`](../GOAL.md) tracks that sweep separately, and it is done.
These are the platform gaps: the things that are nobody's problem while tezgah
is a library, and become the product's problem the moment this repository
ships a shop somebody else runs.

### The library

**Only one list can be searched, and nothing can be sorted.** `page::Search`
and `ProductFilter.search` gave the catalogue a search box — title, handle and
subtitle, `ilike`, no index. Orders and customers still have none, and both
take their filters as positional arguments rather than a struct, so giving
them one is a signature change across their callers. One list sorts two ways: a cursor
carries a key now — a timestamp or a text — so a page ordered by title
resumes from a title, and `catalogue::products` takes `by=title`. The design
that was named as missing is done; what is left is applying it, which is a
column and a variant per list rather than a shape to work out.

**The document described no query parameter at all until #254**, so every
filter the crate already supported was invisible to it — `"parameters": []`
on `GET /admin/products`, whose handler has taken eight of them since it was
written. Three lists describe theirs now and the rest do not. Worth writing
down for the way it was found rather than the size of it: adding `q` to the
catalogue passed CI without changing a byte of the snapshot, and a snapshot
that cannot notice a new filter is not watching the thing it was meant to
watch.

**`Page<T>` carries a count when it is asked for, on one list.** `items` and
`next` by design — a cursor page does not know how many rows are behind it —
plus `total`, which is `null` unless the caller said `count=true` and the
list can answer. Null and zero stay distinguishable: zero is a real answer.

`GET /admin/products` answers one. Its predicates moved into a
`product_filter!` macro that both queries read, because a count assembled
from a second copy of the where clause is one fact with two answers and would
drift the day a filter changed. Every other list still says `null`, and each
gaining one is that same extraction — small, but not free.

**A provider's callback is received, and nothing acts on it.**
`POST /webhooks/payments/{provider}` is declared on its own surface —
`Surface::Webhook`, because neither a shopper's key nor a back office's token
is what authenticates it — and `app/server` mounts it only when a secret was
configured, checking a signature over the exact bytes before the body is
believed. A redelivery lands once, against the unique `(provider, event_id)`.

What is left is the second step. Recording is all the route does: capturing,
moving an order's state, anything that follows from what the provider *said*
is provider-specific mapping, and `GET /admin/payment-webhooks` hands back
what has arrived and not been acted on. That split is deliberate — the row is
durable before anything acts, so a crash between the two resumes rather than
loses — but until something acts, an asynchronously confirmed payment is
written down rather than applied.

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
uuid, visibly, because a shared secret is not a person. What needs a letter is
still missing and is now smaller than it was: there is no invitation and no
notification, and the password reset is a person rather than a link — an
owner sets a new one and tells them the way they told them the first. Every
session that operator holds ends with it. A reset link a server cannot send
is worse than one it never offered, and this server has no mailer.

**Authorization at the door, not at the row.** An operator has one of three
roles, and the gate checks it against the `Action` the route table already
declares — the same table the OpenAPI document and the permission matrix read.
A viewer may only `View`; staff may do everything but `Settle`, which is
capture, refund and cancel; an owner may do anything and is the only role that
may make an account. The split is the crate's own: `ports::Action` separates
`Settle` from `Write` and says why.

What that answers is "may this person refund anything at all". What it does
not answer is "may this person refund *this* order" — that is what
`ports::Authorizer` is for, and the app still answers it by granting
everything.

Worth being exact about why, because "the app should implement its authorizer"
is the obvious next step and would be dead code today. `Resource` carries the
owner on the five kinds that have one — a cart, an order, a payment, a credit,
a subscription — so a per-row rule needs no database, only an actor to compare
against. This binary has no actor to compare: it produces `Actor::Staff` for
the back office, and for the storefront it produces `Actor::Guest { cart }`
with the cart id taken from the same path parameter it is then asked about.
Actor and resource agree by construction, so a rule comparing them refuses
nothing.

What would make it bite is a storefront sign-in — an `Actor::Customer` whose
id came from a session rather than from the URL. That is a feature this
product does not have rather than a rule it forgot, and it is the thing to
build before the authorizer, not after.

**The sweeps and the queue both run.** `cart::expire` and
`inventory::expire_reservations` are called every five minutes by
`app/server/src/schedule.rs`; before that they were called by tests and by
nothing else, so on the shipped image an abandoned cart was never cleared and
the stock it reserved was held for ever. The worker used to claim a job, print
it and mark it processed whatever its kind was, which swallowed the one kind
the crate enqueues; it dispatches now, retries with a doubling backoff, and
leaves a job dead with its reason after five attempts. A kind nothing handles
fails with that as its reason rather than being marked done.

The one kind the crate enqueues still cannot run, and now says why: a
subscription's dunning retry needs a provider that can charge a card left on
file, and no published version of the payment library can name which card.
The capability is on that library's main branch and was committed eleven
hours after its newest tag, so what is missing is a release rather than the
work — productdevbook/kasapay#225. Asked for there rather than worked around
here, and the job records the reason and waits.

**Events are written down and sent.** `EventSink` writes an outbox row and
`AuditSink` an audit row, both in the transaction of the change they belong
to — so a row is a thing that happened, and a change that rolled back left
none. Both were a line on stdout, which meant an event that mattered was gone
unless somebody was tailing the log. A worker then posts each undelivered row
to one URL, signed `sha256=<hmac>` over the exact bytes, retrying with the
same doubling backoff the job worker uses and leaving a spent row dead with
its reason on it.

One destination, deliberately. A shop that needs events in five places puts
something that fans out behind the one URL; what the product owes is that the
event leaves the building, once per event, with something the receiver can
check. Delivery is at-least-once and says so: a crash between the receiver's
answer and the row's update sends it again, and the event's id is in the body
to deduplicate on — the same contract `payment::record_webhook` implements on
the way in.

**There is a mailer, and one thing sends a letter.** SMTP through lettre,
configured or absent — absent, everything that would have needed a letter
says so rather than pretending. What sends one today is an operator
invitation: a one-use token in a link, seven days, replacing any open
invitation for the same address rather than adding a second.

**There is a file store too, and one screen uploads to it.** A directory,
five image types listed rather than prefix-matched, a name this binary
chooses so nothing a caller sent decides where a file lands. A product's
image is still a URL in a column — which is what tezgah models and what makes
moving to object storage a matter of putting a bucket behind the same path.

**What still sends no letter.** A shopper gets no order confirmation, and an
operator who forgets a password has an owner set them a new one rather than
asking for a link. Both are one letter each now rather than a missing
subsystem.

**116 of 486 declared routes are bound.** The panel draws 228. The difference
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

**A record's page has one editor, except the product's.** A section that can
be changed gets its own address and its own drawer, so a save is small enough
to describe — the product's page has three. This was written down as
impossible once, on the reasoning that the API offers one write per record
rather than one per part of it. The reasoning was wrong: the write takes
every field as an option, so a form sending three of them leaves the rest
alone. What is left is the other records, and for most of them one editor is right
rather than unfinished: a customer has six fields to a product's seventeen,
and splitting six across three drawers is ceremony.

**Bulk is a round trip.** A page of variants out as CSV, edited, and back in
— the export's columns and the import's are the same, which is what makes it
one. Multi-select and a bulk delete are on the products
list; prices and an item's stock levels each have an edit grid, which they
can have because their batch routes take the rows together. A list whose
writes are one row at a time cannot have a grid worth using, and the round
trip is what a shop changing four hundred of anything else reaches for.

**Mountable.** No screen reaches for a global: where the API is, what token
to send, what to do when it is refused, which language to draw in and where
these screens live in the URL are a host's answers. An application embedding
the library renders one element — `<Panel basepath="/admin/shop" …/>` — and
gets the whole route tree under that prefix, importing no screen, no route
and no query key.

Two things make that true rather than intended. The router is built per
mount, so a basepath is a mount's answer and two panels can sit on one origin
— it used to be a module singleton, which could only ever be at the root.
And `scripts/check-host-answers.mjs` fails the build when anything outside
the seven files that *are* the standalone host reads `import.meta.env`,
`localStorage` or `document.cookie`. It caught one thing the day it was
written: shadcn's sidebar wrote `sidebar_state` at `path=/`, which a panel
inside an application would have written over that application's own with.

What is left is packaging. There is no library build here yet, so a host
consumes these screens as source; a published entry point is a build step and
a version, not a seam.

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

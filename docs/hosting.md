# What a host has to do

tezgah is a library. Some of what a commerce platform does is not commerce, and
some of it needs a connection or a clock that only the surrounding application
can open. This is the list of those, with the reasoning — the README carries
the summary.

## The ports

`src/ports.rs` is the whole of what tezgah asks for. Two of the five are
load-bearing for anything that happens with nobody watching, and both fail
silently when they are implemented carelessly.

**`Authorizer` must not deny `Actor::System`.** A subscription renewal runs as
`System`, because there is no shopper in a browser to run it as. An authorizer
that denies it stops every subscription in the shop from renewing — and stops
them quietly, because the only caller is a cron and nobody is watching the
answer.

**`Jobs` must actually run the jobs.** A declined renewal queues its next
attempt through `Jobs`, in the same transaction as the `past_due` it belongs
to. A host that implements the trait as a no-op has a shop that stops charging
somebody and never tries again.

## What `Jobs` is, and what it is not

`Jobs` is for one shape: a write that knows, at the moment it happens, that
another must follow it at a later, specific time. A retry. A reminder. Work
this transaction cannot do because it has not happened yet.

It is not how tezgah asks to be run on a clock. A reservation timing out
(`inventory::expire_reservations`) and a subscription's period ending
(`subscription::due`) are sweeps over every row past a deadline, not one row's
own next step. Both are plain functions a host calls on whatever schedule it
already has. A digital entitlement's expiry follows the same rule, rather than
growing a job for every kind of "this timed out".

## Money arriving

**Call `settlement::capture`.** A route does, and so does a host's own webhook
handler — never `payment::capture_only`, which takes the money and does nothing
else.

`settlement::capture` calls it and then everything a captured payment obligates
the shop to: a purchased gift card printed, a digital entitlement granted, a
subscription's first period started. `settlement::refund` is its mirror, and
gives back what a refund takes away.

The names are deliberate. `capture_only` reads like a warning because it is
one.

## Saving a card

The instrument stays with the payment provider. A shopper tokenises it in their
browser and tezgah never sees a card number.

What `payment::save_account_holder` keeps is the reference that comes back —
the provider's id for that customer — so a later charge can name the same
instrument instead of asking for it again. `POST
/store/customers/me/account-holders` is the one route onto it, and it always
saves the signed-in shopper's own reference: nothing in the request names a
different customer, and re-saving a reference somebody else has already claimed
is a conflict rather than a takeover.

A subscription's `account_holder_id` is this id, carried forward.

## Splitting a checkout across sellers

A marketplace seller is its own scope. Driving a checkout under a scope needs a
`Tx` and a `Ctx` for that scope, and only a host can open those — a library
opening its own connections per seller would be doing the one thing this crate
refuses everywhere else.

So the division is:

**tezgah owns the join.** `order_basket`, `order.basket_id` and
`cart.basket_id` are how a customer's one order number and one payment survive
being split. `checkout::Machine::place` takes an optional `basket_id` for a run
that is one seller's leg rather than a whole checkout.

**The host decides when to call it more than once.** It opens the basket in the
marketplace's own scope, opens one cart per seller-scope carrying that
`basket_id`, and then for each seller scope the basket touches — found through
`cart::for_basket`, read under that scope's own `Ctx` — opens a `Tx`/`Ctx` and
calls `place` with that leg's cart and the basket id.

Each leg's `redeem_credit` and `authorize_payment` steps skip themselves. The
shopper pays once, into the collection `order_basket::attach_payment_collection`
attaches to the basket after every leg has an order — not once per seller.

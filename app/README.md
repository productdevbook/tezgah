# app

One shop, self-hosted. A binary and the panel that talks to it.

`../src` is the library: it decides nothing about how it is run, owns no
`main`, draws no screens, and knows nothing about this directory. This is the
other half — everything somebody needs who wants a commerce backend rather
than a crate to embed in something they already have.

    app/
      server/   the binary: axum over the route table, the five ports answered
      client/   the panel: React 19, Vite, TanStack Router and Query, shadcn

Two images, useful only together. `docker-compose.yml` at the repository root
runs both beside a Postgres, and [`docs/self-hosting.md`](../docs/self-hosting.md)
is the operator's version of this page.

## Why `app/` and not `selfhost/`

Self-hosting is what somebody does with this; it is not what this is. What
this is, is one host over the library — the same thing an application
embedding tezgah writes for itself, written once and shipped. Naming the
directory after one deployment style would make the name wrong the day a
second host lands beside it.

That matters because a second one is plausible: many shops on one deployment,
`Scope` resolved per request rather than fixed. Nothing in this directory
would be reused by it and nothing in `../src` would change for it — it would
sit here as a sibling, and `selfhost/` would by then be a lie about both.

## The one thing to hold on to

**This directory may not decide anything the library should have decided.**
A total added up in a handler, a status transition checked in TypeScript, a
list the panel sorts because the API will not — each is a second answer to a
question `../src` already answers, and one fact with two answers is the
failure this codebase has written down the most times.

When something is missing here because the library will not give it, that is a
gap in the library and belongs in an issue against it. The list of them, and
which layer owns each, is [`docs/architecture.md`](../docs/architecture.md).

## What it is not, yet

Read that document before assuming this is a finished product. In short:
there is authentication and no authorization — every operator can do
everything; nothing can send a letter, so there is no invitation and no
password reset; the job worker dispatches nothing, so a declined subscription
renewal is retried never; events and audit entries go to stdout; a product
image can only be a URL somebody else hosts; and 116 of the library's 486
declared routes are bound.

Each of those is written down rather than discovered, and each says which side
of the seam it belongs to.

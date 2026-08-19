//! The vocabulary [kasapay](https://github.com/productdevbook/kasapay) lends
//! tezgah, and nothing else — #53.
//!
//! This used to hold tezgah's own hosted-flow adapters for Stripe and iyzico,
//! written before kasapay existed. Both are gone: kasapay 0.0.5 closed the two
//! gaps that kept them around — [kasapay#149] gave `Currency` the 119
//! currencies Stripe settles in, and [kasapay#150] gave `Provider::charge` a
//! buyer, addresses and a basket, so iyzico's classic hosted form goes through
//! the trait instead of refusing every call with `Unsupported`. There was
//! nothing left for a second payment abstraction to earn its keep against.
//!
//! The `kasapay` meta-crate — and `kasapay-stripe`, `kasapay-iyzico` — are not
//! a dependency here, and never should be: which provider a deployment
//! compiles in is the host's decision, the same as `Authorizer`/`Clock`/
//! `Jobs` — see `CLAUDE.md`'s "Ports ask, they do not answer". A host depends
//! on `kasapay-core` plus whichever provider crates it wants, builds an
//! `Arc<dyn kasapay_core::Provider>`, and calls the functions in
//! [`kasapay`](self::kasapay) to move money through it.
//!
//! [kasapay#149]: https://github.com/productdevbook/kasapay/issues/149
//! [kasapay#150]: https://github.com/productdevbook/kasapay/issues/150

pub mod kasapay;

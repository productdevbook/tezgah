//! What a back office may ask.
//!
//! Everything a shopper cannot: drafts, other people's orders, the money.
//! Every route here declares the permission it asks for, and the host's
//! authorizer answers — tezgah has no notion of an administrator.

use super::Route;

pub(super) static ROUTES: &[Route] = &[];

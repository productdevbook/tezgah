//! Turning `config::Payment` into something that can take money.
//!
//! tezgah writes no payment provider: `CLAUDE.md` says a provider belongs to
//! kasapay, and this file is the whole of what that means in practice — a
//! `match` from a name in the environment onto an adapter crate somebody else
//! maintains, wrapped in `provider::KasapayProvider` so tezgah sees its own
//! `PaymentProvider` trait and nothing about which bank is behind it.
//!
//! Adding a provider is a variant in `config::Payment`, an arm here, and a
//! dependency. It is deliberately that small: the day this file needs to know
//! what a charge *means* is the day the abstraction has been put in the wrong
//! place.

use std::sync::Arc;

use kasapay_core::Secret;

use crate::config::Payment;

/// Builds the provider named in the environment.
///
/// Fails rather than falling back: an install that named iyzico and got
/// something else would take money it could not later refund through the same
/// provider.
pub fn build(
    payment: &Payment,
) -> Result<Arc<dyn kasapay_core::Provider>, Box<dyn std::error::Error>> {
    match payment {
        Payment::Iyzico {
            api_key,
            secret_key,
            sandbox,
        } => {
            let credentials =
                kasapay_iyzico::Credentials::new(Secret::new(api_key), Secret::new(secret_key));
            let config = if *sandbox {
                kasapay_iyzico::classic::Config::sandbox(credentials)
            } else {
                kasapay_iyzico::classic::Config::production(credentials)
            };
            Ok(Arc::new(kasapay_iyzico::classic::Client::new(config)?))
        }
        Payment::Stripe { secret_key } => Ok(Arc::new(kasapay_stripe::Stripe::new(&Secret::new(
            secret_key,
        )))),
    }
}

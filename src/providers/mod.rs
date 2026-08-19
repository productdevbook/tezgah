//! Payment providers that talk to a real network.
//!
//! Both of the ones here are hosted flows: the shopper types a card number on
//! the provider's own page, and a card number never reaches this process. There
//! is no form here, and no iframe that claims to be only a proxy — either would
//! put whoever runs tezgah inside PCI scope.
//!
//! No method holds a database transaction. That is [`PaymentProvider`]'s shape
//! and it is kept: a call goes out, and the caller writes the answer down
//! afterwards with the functions in [`crate::payment`].
//!
//! Every request has a deadline. A provider that stops answering must not turn
//! into a checkout worker that never returns.
//!
//! [`PaymentProvider`]: crate::payment::PaymentProvider

use std::collections::HashMap;
use std::time::Duration;

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use crate::error::{Error, Result};
use crate::money::{Currency, Money};

pub mod iyzico;
pub mod kasapay;
pub mod stripe;

/// How long any one call to a provider may take before it is given up on.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// How many decimal places each currency is written in, which is the only
/// honest way to turn [`Money`] into what a provider wants.
///
/// The defaults are ISO 4217's. A host whose `currency` table disagrees says so
/// with [`Exponents::with`] rather than being overruled quietly.
#[derive(Debug, Clone, Default)]
pub struct Exponents {
    overrides: HashMap<Currency, u32>,
}

impl Exponents {
    pub fn new() -> Exponents {
        Exponents::default()
    }

    pub fn with(mut self, currency: Currency, exponent: u32) -> Exponents {
        self.overrides.insert(currency, exponent);
        self
    }

    pub fn of(&self, currency: Currency) -> u32 {
        if let Some(known) = self.overrides.get(&currency) {
            return *known;
        }
        match currency.as_str() {
            "BIF" | "CLP" | "DJF" | "GNF" | "ISK" | "JPY" | "KMF" | "KRW" | "PYG" | "RWF"
            | "UGX" | "UYI" | "VND" | "VUV" | "XAF" | "XOF" | "XPF" => 0,
            "BHD" | "IQD" | "JOD" | "KWD" | "LYD" | "OMR" | "TND" => 3,
            _ => 2,
        }
    }
}

/// An amount as the provider's smallest unit: 10.50 TRY is 1050, 1000 JPY is
/// 1000, and 3.000 KWD is 3000. Never a hundred times anything.
pub fn to_minor(amount: Money, exponents: &Exponents) -> Result<i64> {
    let exponent = exponents.of(amount.currency);
    let scale = Decimal::from(10u64.pow(exponent));
    (amount.amount.round_dp(exponent) * scale)
        .round()
        .to_i64()
        .ok_or_else(|| Error::invalid("that amount does not fit in a provider's minor units"))
}

/// The inverse, for reading an amount back out of a provider's answer.
pub fn from_minor(minor: i64, currency: Currency, exponents: &Exponents) -> Money {
    let exponent = exponents.of(currency);
    let scale = Decimal::from(10u64.pow(exponent));
    Money::new((Decimal::from(minor) / scale).round_dp(exponent), currency)
}

/// An amount as a decimal string with the currency's own number of places,
/// which is what iyzico's API reads.
pub fn to_decimal_string(amount: Money, exponents: &Exponents) -> String {
    let exponent = exponents.of(amount.currency) as usize;
    format!("{:.*}", exponent, amount.amount.round_dp(exponent as u32))
}

pub(crate) fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Compares two secrets in time that does not depend on how much of them
/// matches. A `==` here is a signature oracle.
pub(crate) fn same_secret(left: &[u8], right: &[u8]) -> bool {
    use subtle::ConstantTimeEq;

    if left.len() != right.len() {
        return false;
    }
    left.ct_eq(right).into()
}

pub(crate) fn http_client(timeout: Duration, provider: &'static str) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(timeout)
        .build()
        .map_err(|err| Error::provider(provider, format!("no http client: {err}")))
}

//! Amounts.
//!
//! One `NUMERIC(20, 6)` column per amount and a currency beside it. Not minor
//! units, because a currency's exponent is a formatting fact rather than a
//! storage one and hard-coding two decimal places breaks JPY the day somebody
//! sells to Japan. Not a float, ever.

use std::fmt;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// ISO 4217, upper case, validated on the way in so it cannot be a typo by the
/// time it reaches a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Currency([u8; 3]);

impl Currency {
    pub fn parse(text: &str) -> Result<Self> {
        let bytes = text.as_bytes();
        if bytes.len() != 3 || !bytes.iter().all(u8::is_ascii_alphabetic) {
            return Err(Error::invalid(format!("{text:?} is not a currency code")));
        }
        let mut code = [0u8; 3];
        for (at, byte) in bytes.iter().enumerate() {
            code[at] = byte.to_ascii_uppercase();
        }
        Ok(Currency(code))
    }

    /// `parse` is the only constructor and it admits ASCII letters only, so the
    /// fallback is unreachable; `XXX` is ISO 4217 for "no currency" rather than
    /// a panic in a library.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("XXX")
    }
}

impl Serialize for Currency {
    fn serialize<S: serde::Serializer>(&self, out: S) -> std::result::Result<S::Ok, S::Error> {
        out.serialize_str(self.as_str())
    }
}

/// Through [`Currency::parse`] rather than the derive: a derived impl fills the
/// bytes in without looking at them, and everything downstream believes them.
impl<'de> Deserialize<'de> for Currency {
    fn deserialize<D: serde::Deserializer<'de>>(from: D) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(from)?;
        Currency::parse(&text).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Money {
    pub amount: Decimal,
    pub currency: Currency,
}

impl Money {
    pub fn new(amount: Decimal, currency: Currency) -> Self {
        Money { amount, currency }
    }

    /// Adding two currencies is a bug rather than a validation failure, so it
    /// is refused rather than silently converted.
    pub fn plus(self, other: Money) -> Result<Money> {
        self.same_currency(other)?;
        Ok(Money::new(self.amount + other.amount, self.currency))
    }

    pub fn minus(self, other: Money) -> Result<Money> {
        self.same_currency(other)?;
        Ok(Money::new(self.amount - other.amount, self.currency))
    }

    pub fn times(self, by: Decimal) -> Money {
        Money::new(self.amount * by, self.currency)
    }

    pub fn is_negative(self) -> bool {
        self.amount.is_sign_negative() && !self.amount.is_zero()
    }

    fn same_currency(self, other: Money) -> Result<()> {
        if self.currency == other.currency {
            Ok(())
        } else {
            Err(Error::bug("two currencies met in one sum"))
        }
    }
}

/// Splits an amount across `weights`, giving the rounding remainder away one
/// minor unit at a time so the parts add back up to the whole.
///
/// A discount divided proportionally and then rounded per line is how a total
/// stops matching what the provider was asked to charge.
pub fn allocate(total: Money, weights: &[Decimal], exponent: u32) -> Result<Vec<Money>> {
    let sum: Decimal = weights.iter().sum();
    if sum.is_zero() {
        return Err(Error::bug("allocating across weights that sum to nothing"));
    }

    let step = Decimal::ONE / Decimal::from(10u64.pow(exponent));
    let mut parts: Vec<Decimal> = weights
        .iter()
        .map(|weight| (total.amount * weight / sum).round_dp(exponent))
        .collect();

    let mut drift = total.amount - parts.iter().sum::<Decimal>();
    let mut at = 0;
    while !drift.is_zero() && at < parts.len() {
        let nudge = if drift.is_sign_negative() {
            -step
        } else {
            step
        };
        parts[at] += nudge;
        drift -= nudge;
        at += 1;
    }

    if !drift.is_zero() {
        return Err(Error::bug("an allocation did not add back up"));
    }

    Ok(parts
        .into_iter()
        .map(|amount| Money::new(amount, total.currency))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn try_(code: &str) -> Currency {
        Currency::parse(code).expect("a currency code")
    }

    #[test]
    fn a_currency_is_three_letters_and_upper_case() {
        assert_eq!(try_("try").as_str(), "TRY");
        assert!(Currency::parse("TL").is_err());
        assert!(Currency::parse("TRY1").is_err());
    }

    #[test]
    fn a_currency_is_json_text_and_comes_back_the_same() {
        let code = try_("TRY");
        let json = serde_json::to_string(&code).expect("serialises");
        assert_eq!(json, "\"TRY\"");
        assert_eq!(
            serde_json::from_str::<Currency>(&json).expect("deserialises"),
            code
        );
        assert_eq!(
            serde_json::from_str::<Currency>("\"try\"").expect("deserialises"),
            code
        );
    }

    #[test]
    fn a_money_carries_its_currency_as_text() {
        let money = Money::new(dec!(10.5), try_("KWD"));
        let json = serde_json::to_value(money).expect("serialises");
        assert_eq!(json["currency"], serde_json::json!("KWD"));
        assert_eq!(
            serde_json::from_value::<Money>(json).expect("deserialises"),
            money
        );
    }

    #[test]
    fn a_currency_that_is_not_one_is_refused_rather_than_believed() {
        for text in ["\"€\"", "\"ab\"", "\"ABCD\"", "\"\"", "[84,82,89]", "null"] {
            assert!(
                serde_json::from_str::<Currency>(text).is_err(),
                "{text} was accepted"
            );
        }
    }

    #[test]
    fn two_currencies_do_not_add() {
        let lira = Money::new(dec!(10), try_("TRY"));
        let euro = Money::new(dec!(10), try_("EUR"));
        assert!(lira.plus(euro).is_err());
    }

    #[test]
    fn an_allocation_adds_back_up_to_the_whole() {
        let discount = Money::new(dec!(1.00), try_("TRY"));
        let parts = allocate(discount, &[dec!(1), dec!(1), dec!(1)], 2).expect("allocates");
        let back: Decimal = parts.iter().map(|part| part.amount).sum();
        assert_eq!(back, dec!(1.00));
    }

    #[test]
    fn the_remainder_lands_somewhere_rather_than_vanishing() {
        let total = Money::new(dec!(29.00), try_("TRY"));
        let parts = allocate(total, &[dec!(1), dec!(1), dec!(1)], 2).expect("allocates");
        assert_eq!(
            parts.iter().map(|part| part.amount).sum::<Decimal>(),
            dec!(29.00)
        );
        assert_ne!(parts[0].amount, parts[2].amount);
    }
}

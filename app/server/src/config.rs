//! Everything this binary reads from its environment, gathered in one place
//! and checked once at startup rather than the first time each value is
//! needed.
//!
//! A container gets its configuration from the environment it was started
//! with, not from a file it has to be handed separately — so there is no
//! config file format here, and there will not be one.

use tezgah::id::StockLocationId;

/// A currency's decimal places, when nothing more specific is given — most
/// currencies in circulation use two, and a shop trading in one that does not
/// (Japanese yen has none, Bahraini dinar has three) sets
/// `TEZGAH_CURRENCY_EXPONENT` itself. `provider::KasapayProvider`'s own doc
/// says why this is one number rather than one per currency: the payment
/// provider wrapper here assumes a shop selling in a single currency, the
/// same trade `examples/shop` makes.
const DEFAULT_CURRENCY_EXPONENT: u32 = 2;

const DEFAULT_PORT: u16 = 8080;

/// The only value `TEZGAH_DEMO_BANK` accepts. A phrase, not `1` or `true`,
/// so that setting it is a decision rather than a habit — see
/// `provider::DemoBank`'s own doc comment for what it authorises and
/// `docs/self-hosting.md` for what taking real money instead requires.
const DEMO_BANK_CONFIRMATION: &str = "i-understand-this-takes-no-money";

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub port: u16,
    pub skip_migrations: bool,
    /// The bearer token that unlocks the admin surface. `None` means the
    /// surface is not mounted at all — see `http::router`'s doc comment for
    /// why that is the only honest default.
    pub admin_token: Option<String>,
    /// The one warehouse `checkout::Checkout` reserves stock from and ships
    /// from. `None` leaves `POST /store/carts/{id}/complete` unbound, because
    /// `tezgah::checkout::Checkout::new` cannot be built without one — see
    /// `README.md`'s route table for what that costs a fresh install.
    pub stock_location_id: Option<StockLocationId>,
    pub currency_exponent: u32,
    /// `true` only when `TEZGAH_DEMO_BANK` is set to exactly
    /// `DEMO_BANK_CONFIRMATION`. The only payment provider this binary ships
    /// is `provider::DemoBank`, which authorises every charge and remembers
    /// nothing — so checkout stays unbound on `false`, the same way it stays
    /// unbound with no `stock_location_id`. `main.rs` is where the two
    /// combine.
    pub demo_bank_enabled: bool,
    /// Where an outbox row is sent. `None` leaves the deliverer unstarted and
    /// every event unsent — which is what this binary did before there was a
    /// deliverer at all, and is still the honest default: an event posted to
    /// nowhere in particular is worse than one left in a table somebody can
    /// read.
    pub event_webhook: Option<String>,
    /// Signs the body so the receiver can tell it came from this shop.
    /// Required whenever `event_webhook` is set: an unsigned webhook is an
    /// endpoint anybody who guesses the URL can post to.
    pub event_secret: Option<String>,
    /// The secret a payment provider's callback is signed with. Unset leaves
    /// `POST /webhooks/payments/{provider}` unmounted rather than open.
    pub webhook_secret: Option<String>,
    /// lettre's own URL — `smtps://user:pass@host:465`. Unset means this shop
    /// cannot send a letter, and everything that would have needed one says so
    /// rather than pretending it was sent.
    pub smtp_url: Option<String>,
    /// Who a letter is from. Required with `smtp_url`, because a message with
    /// no sender is a message most servers refuse.
    pub mail_from: Option<String>,
    /// Where the panel is, so an invitation can carry a link somebody can
    /// click. Required with `smtp_url` for the same reason: a link this
    /// binary has to guess is a link that goes to the wrong host.
    pub panel_url: Option<String>,
}

#[derive(Debug)]
pub struct ConfigError(String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

fn err(message: impl Into<String>) -> ConfigError {
    ConfigError(message.into())
}

impl Config {
    /// Reads and validates every setting this binary needs, failing with one
    /// message naming what is wrong rather than letting a missing value
    /// surface as a panic on the first request that needed it.
    pub fn from_env() -> Result<Config, ConfigError> {
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| err("DATABASE_URL must be set — a postgres:// connection string"))?;
        if database_url.trim().is_empty() {
            return Err(err("DATABASE_URL is set but empty"));
        }

        let port = match std::env::var("PORT") {
            Ok(text) => text.parse::<u16>().map_err(|_| {
                err(format!(
                    "PORT is set to {text:?}, which is not a port number"
                ))
            })?,
            Err(std::env::VarError::NotPresent) => DEFAULT_PORT,
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(err("PORT is set but is not valid text"));
            }
        };

        let skip_migrations = std::env::var("TEZGAH_SKIP_MIGRATIONS")
            .map(|value| value == "1")
            .unwrap_or(false);

        let admin_token = match std::env::var("ADMIN_TOKEN") {
            Ok(token) if token.trim().is_empty() => {
                return Err(err(
                    "ADMIN_TOKEN is set but empty — unset it to run without an admin surface",
                ));
            }
            Ok(token) => Some(token),
            Err(_) => None,
        };

        let stock_location_id = match std::env::var("TEZGAH_STOCK_LOCATION_ID") {
            Ok(text) => {
                let uuid = uuid::Uuid::parse_str(&text).map_err(|_| {
                    err(format!(
                        "TEZGAH_STOCK_LOCATION_ID is set to {text:?}, which is not a uuid"
                    ))
                })?;
                Some(StockLocationId::from_uuid(uuid))
            }
            Err(_) => None,
        };

        let currency_exponent = match std::env::var("TEZGAH_CURRENCY_EXPONENT") {
            Ok(text) => text.parse::<u32>().map_err(|_| {
                err(format!(
                    "TEZGAH_CURRENCY_EXPONENT is set to {text:?}, which is not a whole number"
                ))
            })?,
            Err(_) => DEFAULT_CURRENCY_EXPONENT,
        };

        let demo_bank_enabled = std::env::var("TEZGAH_DEMO_BANK")
            .map(|value| value == DEMO_BANK_CONFIRMATION)
            .unwrap_or(false);

        let event_webhook = match std::env::var("TEZGAH_EVENT_WEBHOOK") {
            Ok(url) if url.trim().is_empty() => {
                return Err(err(
                    "TEZGAH_EVENT_WEBHOOK is set but empty — unset it to leave events undelivered",
                ));
            }
            Ok(url) if !url.starts_with("https://") && !url.starts_with("http://") => {
                return Err(err(format!(
                    "TEZGAH_EVENT_WEBHOOK is set to {url:?}, which is not an http(s) url"
                )));
            }
            Ok(url) => Some(url),
            Err(_) => None,
        };

        let event_secret = match std::env::var("TEZGAH_EVENT_SECRET") {
            Ok(secret) if secret.trim().is_empty() => None,
            Ok(secret) => Some(secret),
            Err(_) => None,
        };

        // Refused rather than defaulted to unsigned. A receiver cannot tell a
        // real event from anybody who guessed the address, and finding that
        // out later means every event already sent was unverifiable.
        if event_webhook.is_some() && event_secret.is_none() {
            return Err(err(
                "TEZGAH_EVENT_WEBHOOK is set without TEZGAH_EVENT_SECRET — \
                 an unsigned webhook is an endpoint anybody can post to",
            ));
        }

        let webhook_secret = match std::env::var("TEZGAH_PAYMENT_WEBHOOK_SECRET") {
            Ok(secret) if secret.trim().is_empty() => {
                return Err(err(
                    "TEZGAH_PAYMENT_WEBHOOK_SECRET is set but empty — unset it to leave the \
                     callback route unmounted",
                ));
            }
            Ok(secret) => Some(secret),
            Err(_) => None,
        };

        let smtp_url = match std::env::var("TEZGAH_SMTP_URL") {
            Ok(url) if url.trim().is_empty() => {
                return Err(err(
                    "TEZGAH_SMTP_URL is set but empty — unset it to run without a mailer",
                ));
            }
            Ok(url) => Some(url),
            Err(_) => None,
        };

        let mail_from = std::env::var("TEZGAH_MAIL_FROM")
            .ok()
            .filter(|from| !from.trim().is_empty());
        let panel_url = std::env::var("TEZGAH_PANEL_URL")
            .ok()
            .filter(|url| !url.trim().is_empty());

        // Refused together rather than discovered one invitation later. A
        // mailer with no sender cannot send, and one with no panel address
        // sends a link to nowhere.
        if smtp_url.is_some() && (mail_from.is_none() || panel_url.is_none()) {
            return Err(err(
                "TEZGAH_SMTP_URL is set without TEZGAH_MAIL_FROM and TEZGAH_PANEL_URL — \
                 a letter needs a sender and an invitation needs somewhere to point",
            ));
        }

        Ok(Config {
            database_url,
            port,
            skip_migrations,
            admin_token,
            stock_location_id,
            currency_exponent,
            demo_bank_enabled,
            event_webhook,
            event_secret,
            webhook_secret,
            smtp_url,
            mail_from,
            panel_url,
        })
    }
}

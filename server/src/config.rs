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

        Ok(Config {
            database_url,
            port,
            skip_migrations,
            admin_token,
            stock_location_id,
            currency_exponent,
        })
    }
}

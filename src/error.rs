//! What can go wrong, and what a caller can do about it.
//!
//! One [`Error`] for the crate. Its cause is private so that adding a failure
//! mode is not a breaking change; ask it questions ([`Error::is_denied`],
//! [`Error::code`]) rather than matching it.

use std::backtrace::Backtrace;
use std::fmt;

use uuid::Uuid;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub(crate) enum Cause {
    Invalid(String),
    NotFound {
        entity: &'static str,
    },
    Denied,
    Conflict(String),
    Provider {
        provider: &'static str,
        message: String,
    },
    OutOfStock {
        variant: Uuid,
    },
    Db(sqlx::Error),
    Bug(&'static str),
}

pub struct Error {
    cause: Box<Cause>,
    backtrace: Backtrace,
}

impl Error {
    pub(crate) fn new(cause: Cause) -> Self {
        Error {
            cause: Box::new(cause),
            backtrace: Backtrace::capture(),
        }
    }

    pub fn invalid(what: impl Into<String>) -> Self {
        Error::new(Cause::Invalid(what.into()))
    }

    pub fn conflict(what: impl Into<String>) -> Self {
        Error::new(Cause::Conflict(what.into()))
    }

    pub fn not_found(entity: &'static str) -> Self {
        Error::new(Cause::NotFound { entity })
    }

    /// What an [`Authorizer`](crate::ports::Authorizer) returns when it says no.
    pub fn denied() -> Self {
        Error::new(Cause::Denied)
    }

    /// What a payment or fulfilment provider refused, in its own words.
    pub fn provider(provider: &'static str, message: impl Into<String>) -> Self {
        Error::new(Cause::Provider {
            provider,
            message: message.into(),
        })
    }

    /// A variant that ran out while somebody was buying it.
    pub fn out_of_stock_for(variant: uuid::Uuid) -> Self {
        Error::new(Cause::OutOfStock { variant })
    }

    pub(crate) fn bug(what: &'static str) -> Self {
        Error::new(Cause::Bug(what))
    }

    /// A stable string for a host to map onto its own error shape, and for a
    /// client to branch on without reading English.
    pub fn code(&self) -> &'static str {
        match *self.cause {
            Cause::Invalid(_) => "invalid",
            Cause::NotFound { .. } => "not_found",
            Cause::Denied => "denied",
            Cause::Conflict(_) => "conflict",
            Cause::Provider { .. } => "provider",
            Cause::OutOfStock { .. } => "out_of_stock",
            Cause::Db(_) | Cause::Bug(_) => "internal",
        }
    }

    /// Whether this is ours rather than the caller's. The message of one of
    /// these belongs in a log and never in a response body.
    pub fn is_internal(&self) -> bool {
        matches!(*self.cause, Cause::Db(_) | Cause::Bug(_))
    }

    pub fn is_denied(&self) -> bool {
        matches!(*self.cause, Cause::Denied)
    }

    pub fn is_not_found(&self) -> bool {
        matches!(*self.cause, Cause::NotFound { .. })
    }

    /// Somebody else got there first, or the state machine forbids the move.
    /// Worth retrying in a way the other kinds are not.
    pub fn is_conflict(&self) -> bool {
        matches!(*self.cause, Cause::Conflict(_))
    }

    /// The variant that ran out, when that is what happened.
    pub fn out_of_stock(&self) -> Option<Uuid> {
        match *self.cause {
            Cause::OutOfStock { variant } => Some(variant),
            _ => None,
        }
    }

    /// The note left with a bug, for a log. Never for a response body.
    pub fn detail(&self) -> Option<&str> {
        match &*self.cause {
            Cause::Bug(what) => Some(what),
            Cause::Provider { message, .. } => Some(message),
            _ => None,
        }
    }

    /// Display, plus the cause when it is one this crate hid.
    ///
    /// For a log or a dead letter, never for a response body: an internal
    /// error's message names tables and constraints.
    pub fn report(&self) -> String {
        match &*self.cause {
            Cause::Db(err) => format!("the database refused a query: {err}"),
            Cause::Bug(what) => format!("internal error: {what}"),
            _ => self.to_string(),
        }
    }

    pub fn backtrace(&self) -> &Backtrace {
        &self.backtrace
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &*self.cause {
            Cause::Invalid(what) => write!(f, "{what}"),
            Cause::NotFound { entity } => write!(f, "no {entity} here"),
            Cause::Denied => f.write_str("not allowed"),
            Cause::Conflict(what) => write!(f, "{what}"),
            Cause::Provider { provider, message } => write!(f, "{provider}: {message}"),
            Cause::OutOfStock { variant } => write!(f, "stock ran out for {variant}"),
            Cause::Db(_) => f.write_str("the database refused a query"),
            Cause::Bug(_) => f.write_str("internal error"),
        }
    }
}

/// Renders the cause and the backtrace, which `Display` deliberately does not:
/// a database message names tables and constraints, and that belongs in a log.
impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.cause)?;
        if matches!(
            self.backtrace.status(),
            std::backtrace::BacktraceStatus::Captured
        ) {
            write!(f, "\n{}", self.backtrace)?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &*self.cause {
            Cause::Db(err) => Some(err),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for Error {
    fn from(err: sqlx::Error) -> Self {
        Error::new(Cause::Db(err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_database_message_stays_out_of_display() {
        let err = Error::from(sqlx::Error::RowNotFound);
        assert!(!err.to_string().contains("RowNotFound"));
        assert!(err.is_internal());
    }

    #[test]
    fn a_refusal_is_not_internal() {
        assert!(!Error::new(Cause::Denied).is_internal());
        assert!(Error::new(Cause::Denied).is_denied());
    }
}

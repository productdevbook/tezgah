use uuid::Uuid;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),

    #[error("no {entity} here")]
    NotFound { entity: &'static str },

    #[error("not allowed")]
    Denied,

    /// Somebody else changed the row first, or the state machine forbids the
    /// move. Distinct from `Invalid` because the caller may usefully retry.
    #[error("{0}")]
    Conflict(String),

    /// A provider said no in a way that is theirs to explain.
    #[error("{provider}: {message}")]
    Provider {
        provider: &'static str,
        message: String,
    },

    #[error("stock ran out for {variant}")]
    OutOfStock { variant: Uuid },

    #[error(transparent)]
    Db(#[from] sqlx::Error),

    /// Ours, not the caller's. The message is for a log, never for a body.
    #[error("bug: {0}")]
    Bug(&'static str),
}

impl Error {
    pub fn invalid(what: impl Into<String>) -> Self {
        Error::Invalid(what.into())
    }

    pub fn conflict(what: impl Into<String>) -> Self {
        Error::Conflict(what.into())
    }

    /// A stable string a host can map onto its own error shape, and a client
    /// can branch on without reading English.
    pub fn code(&self) -> &'static str {
        match self {
            Error::Invalid(_) => "invalid",
            Error::NotFound { .. } => "not_found",
            Error::Denied => "denied",
            Error::Conflict(_) => "conflict",
            Error::Provider { .. } => "provider",
            Error::OutOfStock { .. } => "out_of_stock",
            Error::Db(_) | Error::Bug(_) => "internal",
        }
    }
}

use std::fmt::{Debug, Display, Formatter};

/// All errors that can occur in this crate.
#[derive(Debug)]
#[allow(missing_copy_implementations)]
pub enum Error<SessionStoreConnectorError> {
    /// A session was attempted to be updated, but the session does not exist.
    /// This may happen due to concurrent modification, and is forbidden to prevent data inconsistencies.
    /// If you receive this error, revert everything that you did while handling the request that
    /// used this session.
    UpdatedSessionDoesNotExist,

    /// Tried as often as desired to generate a session id, but all generated ids already exist.
    MaximumSessionIdGenerationTriesReached,

    /// The given cookie has a wrong length.
    WrongCookieLength {
        /// The expected cookie length.
        expected: usize,
        /// The actual cookie length.
        actual: usize,
    },

    /// An error occurred in the session store connector.
    SessionStoreConnector(SessionStoreConnectorError),
}

impl<SessionStoreConnectorError> From<SessionStoreConnectorError>
    for Error<SessionStoreConnectorError>
{
    fn from(error: SessionStoreConnectorError) -> Self {
        Self::SessionStoreConnector(error)
    }
}

impl<SessionStoreConnectorError: Display> Display for Error<SessionStoreConnectorError> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::UpdatedSessionDoesNotExist => write!(f, "the updated session does not exist, which indicates that it was concurrently modified or deleted."),
            Error::MaximumSessionIdGenerationTriesReached => write!(f, "tried to generate a new session id but generated only existing ids until the maximum retry limit was reached."),
            Error::WrongCookieLength { expected, actual } => write!(f, "wrong cookie length, expected {expected}, but got {actual}"),
            Error::SessionStoreConnector(error) => write!(f, "{error}"),
        }
    }
}

impl<SessionStoreConnectorError: Debug + Display> std::error::Error
    for Error<SessionStoreConnectorError>
{
}

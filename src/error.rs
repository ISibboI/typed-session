use std::fmt::Debug;

/// The default result type used by this crate.
pub type Result<T = (), SessionStoreConnectorError = ()> =
    std::result::Result<T, Error<SessionStoreConnectorError>>;

/// All errors that can occur in this crate.
#[derive(Debug)]
#[allow(missing_copy_implementations)]
pub enum Error<SessionStoreConnectorError: Debug> {
    /// A session was attempted to be updated, but the session does not exist.
    /// This may happen due to concurrent modification, and is forbidden to prevent data inconsistencies.
    /// If you receive this error, revert everything that you did while handling the request that
    /// used this session.
    UpdatedSessionDoesNotExist,

    /// Tried as often as desired to generate a session id, but all generated ids already exist.
    MaximumSessionIdGenerationTriesReached,

    /// An error occurred in the session store connector.
    SessionStoreConnector(SessionStoreConnectorError),
}

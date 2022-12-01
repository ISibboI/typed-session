use chrono::{DateTime, Utc};
use crate::{async_trait, Result, Session};
use crate::session::SessionId;

/// An async session store.
#[async_trait]
pub trait SessionStore<Data> {
    /// Get a session from the storage backend.
    ///
    /// The `cookie_value` is the value of a cookie identifying the session.
    /// The return value is `Ok(Some(_))` if there is a session identified by the given cookie that is not expired,
    /// or `Ok(None)` if there is no such session that is not expired.
    async fn load_session(&self, cookie_value: String) -> Result<Option<Session<Data>>>;

    /// Store a session in the storage backend.
    /// If the session is marked for deletion, this method deletes the session.
    ///
    /// If the session cookie requires to be updated, because the session data or expiry changed,
    /// then a [SetSessionCookieCommand] is returned.
    async fn store_session(&self, session: Session<Data>) -> Result<Option<SetSessionCookieCommand>>;

    /// Empties the entire store, deleting all sessions.
    async fn clear_store(&self) -> Result;
}

/// Indicates that the session store should create a new session with the given content.
/// The session store should randomly generate a free session id.
pub struct CreateSessionCommand<Data> {
    pub expiry: DateTime<Utc>,
    pub data: Data,
}

/// Indicates that the session store should update the session with the given `old_id`.
/// It should randomly generate a new and free session id.
pub struct UpdateSessionCommand<Data> {
    pub old_id: Box<SessionId>,
    pub expiry: DateTime<Utc>,
    pub data: Data,
}

/// Indicates that the session store should delete the session with the given id.
pub struct DeleteSessionCommand {
    pub id: Box<SessionId>,
}

/// Indicates that the client's session cookie should be updated.
pub struct SetSessionCookieCommand {
    /// The value of the session cookie.
    pub cookie_value: String,
    /// The expiry time of the session cookie.
    pub expiry: DateTime<Utc>,
}

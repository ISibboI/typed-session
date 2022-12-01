use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::mem;

/// A session with a client.
/// This type handles the creation, updating and deletion of sessions.
/// It is marked `#[must_use]`, as manually dropping it will not update the session store.
/// Instead, it should be passed to [SessionStore::store_session](crate::session_store::SessionStore::store_session).
///
/// `COOKIE_LENGTH` should be a multiple of 32, which is the block-size of blake3.
///
/// # Change tracking example
/// ```rust
/// # use typed_session::{Session, MemoryStore};
/// # fn main() -> typed_session::Result { async_std::task::block_on(async {
/// let mut session_store = MemoryStore::new();
/// let session = Session::new(15);
/// session_store.store_session(session);
///
/// let mut session = session_store.load_session();
/// # Ok(()) }) }
/// ```
#[derive(Debug, Clone)]
#[must_use]
pub struct Session<Data, const COOKIE_LENGTH: usize = 64> {
    pub(crate) state: SessionState<Data>,
}

#[derive(Debug, Clone)]
pub(crate) enum SessionState<Data> {
    /// The session was newly generated for this request.
    New {
        expiry: Option<DateTime<Utc>>,
        data: Data,
    },
    /// The session was loaded from the session store, and was not changed.
    Unchanged {
        id: SessionId,
        expiry: Option<DateTime<Utc>>,
        data: Data,
    },
    /// The session was loaded from the session store, and was changed.
    /// Either the expiry datetime or the data have changed.
    Changed {
        old_id: SessionId,
        expiry: Option<DateTime<Utc>>,
        data: Data,
    },
    /// The session was marked for deletion.
    Deleted { id: SessionId },
    /// The session was marked for deletion before it was ever communicated to database or client.
    NewDeleted,
    /// Used internally to avoid unsafe code when replacing the session state through a mutable reference.
    Invalid,
}

/// The type of a session id.
pub type SessionIdType = [u8; blake3::OUT_LEN];

/// A session id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionId(Box<SessionIdType>);

impl<const COOKIE_LENGTH: usize, Data: Debug> Session<Data, COOKIE_LENGTH> {
    /// Create a new session. Does not set an expiry by default.
    /// The session id is generated once the session is stored in the session store.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # fn main() -> typed_session::Result { async_std::task::block_on(async {
    /// let session = Session::new(());
    /// assert_eq!(None, session.expiry());
    /// # Ok(()) }) }
    pub fn new(data: Data) -> Self {
        Self {
            state: SessionState::new(data),
        }
    }

    /// **This method should only be called by a session store!**
    ///
    /// Create a session instance from parts loaded by a session store.
    /// The session state will be `Unchanged`.
    pub fn new_from_session_store(
        id: SessionId,
        expiry: Option<DateTime<Utc>>,
        data: Data,
    ) -> Self {
        Self {
            state: SessionState::new_from_session_store(id, expiry, data),
        }
    }

    /// Returns the expiry timestamp of this session, if there is one.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # fn main() -> typed_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new(());
    /// assert_eq!(None, session.expiry());
    /// session.expire_in(std::time::Duration::from_secs(1));
    /// assert!(session.expiry().is_some());
    /// # Ok(()) }) }
    /// ```
    pub fn expiry(&self) -> Option<&DateTime<Utc>> {
        self.state.expiry()
    }

    /// Returns a reference to the data associated with this session.
    /// This does not mark the session as changed.
    pub fn data(&self) -> &Data {
        self.state.data()
    }

    /// Returns a mutable reference to the data associated with this session,
    /// and marks the session as changed.
    ///
    /// Note that the session gets marked as changed, even if the returned reference is never written to.
    ///
    /// **Panics** if the session was marked for deletion before.
    pub fn data_mut(&mut self) -> &mut Data {
        self.state.data_mut()
    }

    /// Returns true if this session is marked for destruction.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # fn main() -> typed_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new(());
    /// assert!(!session.is_deleted());
    /// session.delete();
    /// assert!(session.is_deleted());
    /// # Ok(()) }) }
    pub fn is_deleted(&self) -> bool {
        self.state.is_deleted()
    }

    /// mark this session for destruction. the actual session record
    /// is not destroyed until the end of this response cycle.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # fn main() -> typed_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new(());
    /// assert!(!session.is_deleted());
    /// session.delete();
    /// assert!(session.is_deleted());
    /// # Ok(()) }) }
    pub fn delete(&mut self) {
        self.state.delete();
    }

    /// Generates a new id and cookie for this session.
    pub fn regenerate(&mut self) {
        self.data_mut();
    }

    /// Updates the expiry timestamp of this session.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # fn main() -> typed_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new(());
    /// assert_eq!(None, session.expiry());
    /// session.set_expiry(chrono::Utc::now());
    /// assert!(session.expiry().is_some());
    /// # Ok(()) }) }
    /// ```
    pub fn set_expiry(&mut self, expiry: DateTime<Utc>) {
        *self.state.expiry_mut() = Some(expiry);
    }

    /// Sets this session to never expire.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # fn main() -> typed_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new(());
    /// assert_eq!(None, session.expiry());
    /// session.set_expiry(chrono::Utc::now());
    /// assert!(session.expiry().is_some());
    /// session.do_not_expire();
    /// assert!(session.expiry().is_none());
    /// # Ok(()) }) }
    /// ```
    pub fn do_not_expire(&mut self) {
        *self.state.expiry_mut() = None;
    }

    /// Sets this session to expire `ttl` time into the future.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # fn main() -> typed_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new(());
    /// assert_eq!(None, session.expiry());
    /// session.expire_in(std::time::Duration::from_secs(1));
    /// assert!(session.expiry().is_some());
    /// # Ok(()) }) }
    /// ```
    pub fn expire_in(&mut self, ttl: std::time::Duration) {
        *self.state.expiry_mut() = Some(Utc::now() + Duration::from_std(ttl).unwrap());
    }

    /// Return true if the session is expired.
    /// The session is expired if it has an expiry timestamp that is in the future.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # use std::time::Duration;
    /// # use async_std::task;
    /// # fn main() -> typed_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new(());
    /// assert_eq!(None, session.expiry());
    /// assert!(!session.is_expired());
    /// session.expire_in(Duration::from_secs(1));
    /// assert!(!session.is_expired());
    /// task::sleep(Duration::from_secs(2)).await;
    /// assert!(session.is_expired());
    /// # Ok(()) }) }
    /// ```
    pub fn is_expired(&self) -> bool {
        match self.state.expiry() {
            Some(expiry) => *expiry < Utc::now(),
            None => false,
        }
    }

    /// Returns the duration from now to the expiry time of this session.
    /// Returns `None` if it is expired.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # use std::time::Duration;
    /// # use async_std::task;
    /// # fn main() -> typed_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new(());
    /// session.expire_in(Duration::from_secs(123));
    /// let expires_in = session.expires_in().unwrap();
    /// assert!(123 - expires_in.as_secs() < 2);
    /// # Ok(()) }) }
    /// ```
    pub fn expires_in(&self) -> Option<std::time::Duration> {
        let duration = self.state.expiry()?.signed_duration_since(Utc::now());
        if duration > Duration::zero() {
            Some(duration.to_std().unwrap())
        } else {
            None
        }
    }
}

impl<Data: Debug> SessionState<Data> {
    fn new(data: Data) -> Self {
        Self::New { expiry: None, data }
    }

    fn new_from_session_store(id: SessionId, expiry: Option<DateTime<Utc>>, data: Data) -> Self {
        Self::Unchanged { id, expiry, data }
    }

    fn expiry(&self) -> Option<&DateTime<Utc>> {
        match self {
            Self::New { expiry, .. }
            | Self::Unchanged { expiry, .. }
            | Self::Changed { expiry, .. } => expiry.as_ref(),
            Self::Deleted { .. } | Self::NewDeleted => {
                panic!("Attempted to retrieve the expiry of a purged session {self:?}")
            }
            Self::Invalid => unreachable!("Invalid state is used internally only"),
        }
    }

    fn expiry_mut(&mut self) -> &mut Option<DateTime<Utc>> {
        self.change();

        match self {
            Self::New { expiry, .. }
            | Self::Unchanged { expiry, .. }
            | Self::Changed { expiry, .. } => expiry,
            Self::Deleted { .. } | Self::NewDeleted => {
                panic!("Attempted to retrieve the expiry of a purged session {self:?}")
            }
            Self::Invalid => unreachable!("Invalid state is used internally only"),
        }
    }

    fn data(&self) -> &Data {
        match self {
            Self::New { data, .. } | Self::Unchanged { data, .. } | Self::Changed { data, .. } => {
                data
            }
            Self::Deleted { .. } | Self::NewDeleted => {
                panic!("Attempted to retrieve the data of a purged session {self:?}")
            }
            Self::Invalid => unreachable!("Invalid state is used internally only"),
        }
    }

    fn data_mut(&mut self) -> &mut Data {
        self.change();

        match self {
            Self::New { data, .. } | Self::Unchanged { data, .. } | Self::Changed { data, .. } => {
                data
            }
            Self::Deleted { .. } | Self::NewDeleted => {
                panic!("Attempted to retrieve the data of a purged session {self:?}")
            }
            Self::Invalid => unreachable!("Invalid state is used internally only"),
        }
    }

    fn is_deleted(&self) -> bool {
        matches!(self, Self::Deleted { .. } | Self::NewDeleted)
    }

    fn change(&mut self) {
        match self {
            Self::New { .. } => { /* New implies changed, as new sessions anyways need to be communicated to client and database. */
            }
            Self::Unchanged { .. } => {
                let Self::Unchanged { id, expiry, data } = mem::replace(self, Self::Invalid) else {unreachable!()};
                *self = Self::Changed {
                    old_id: id,
                    expiry,
                    data,
                };
            }
            Self::Changed { .. } => { /* Already changed. */ }
            Self::Deleted { .. } | Self::NewDeleted => {
                panic!("Attempted to change purged session {self:?}")
            }
            Self::Invalid => unreachable!("Invalid state is used internally only"),
        }
    }

    fn delete(&mut self) {
        match self {
            Self::New { .. } => {
                *self = Self::NewDeleted;
            }
            Self::Unchanged { .. } => {
                let Self::Unchanged { id, .. } = mem::replace(self, Self::Invalid) else {unreachable!()};
                *self = Self::Deleted { id };
            }
            Self::Changed { .. } => {
                let Self::Changed { old_id, .. } = mem::replace(self, Self::Invalid) else {unreachable!()};
                *self = Self::Deleted { id: old_id };
            }
            Self::Deleted { .. } | Self::NewDeleted => {
                panic!("Attempted to purge a purged session {self:?}")
            }
            Self::Invalid => unreachable!("Invalid state is used internally only"),
        }
    }
}

impl SessionId {
    /// Applies a cryptographic hash function on a cookie value to obtain the session id for that cookie.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # fn main() -> typed_session::Result { async_std::task::block_on(async {
    /// let session = Session::new(());
    /// let id = session.id().to_string();
    /// let cookie_value = session.into_cookie_value().unwrap();
    /// assert_eq!(id, Session::id_from_cookie_value(&cookie_value)?);
    /// # Ok(()) }) }
    /// ```
    pub fn from_cookie_value(cookie_value: &str) -> Self {
        // The original code used base64 encoded binary ids of length of a multiple of the blake3 block size.
        // We do the same but with alphanumerical ids with a length multiple of the blake3 block size.
        let hash = blake3::hash(cookie_value.as_bytes());
        Self(Box::new(hash.into()))
    }
}

impl From<SessionId> for SessionIdType {
    fn from(id: SessionId) -> Self {
        *id.0
    }
}

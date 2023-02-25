use chrono::{DateTime, Duration, Utc};
use std::fmt::Debug;
use std::mem;

/// A session with a client.
/// This type handles the creation, updating and deletion of sessions.
/// It is marked `#[must_use]`, as dropping it will not update the session store.
/// Instead, it should be passed to [SessionStore::store_session](crate::session_store::SessionStore::store_session).
///
/// `SessionData` is the data associated with a session.
/// `COOKIE_LENGTH` is the length of the session cookie, in characters.
/// It should be a multiple of 32, which is the block size of blake3.
#[derive(Debug, Clone)]
#[must_use]
pub struct Session<SessionData, const COOKIE_LENGTH: usize = 64> {
    pub(crate) state: SessionState<SessionData>,
}

#[derive(Debug, Clone)]
pub(crate) enum SessionState<SessionData> {
    /// The session was newly generated for this request, and not yet written to.
    /// In this state, the session does not necessarily need to be communicated to the client.
    NewUnchanged { data: SessionData },
    /// The session was newly generated for this request, and was written to.
    /// In this state, the session must be communicated to the client.
    NewChanged {
        expiry: SessionExpiry,
        data: SessionData,
    },
    /// The session was loaded from the session store, and was not changed.
    Unchanged {
        previous_id: Option<SessionId>,
        current_id: SessionId,
        expiry: SessionExpiry,
        data: SessionData,
    },
    /// The session was loaded from the session store, and was changed.
    /// Either the expiry datetime or the data have changed.
    Changed {
        deletable_id: Option<SessionId>,
        previous_id: SessionId,
        expiry: SessionExpiry,
        data: SessionData,
    },
    /// The session was marked for deletion.
    Deleted {
        current_id: SessionId,
        previous_id: Option<SessionId>,
    },
    /// The session was marked for deletion before it was ever communicated to database or client.
    NewDeleted,
    /// Used internally to avoid unsafe code when replacing the session state through a mutable reference.
    Invalid,
}

/// The expiry of a session.
/// Either a given date and time, or never.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum SessionExpiry {
    /// The session expires at the given date and time.
    DateTime(DateTime<Utc>),
    /// The session never expires, unless it is explicitly deleted.
    Never,
}

/// The type of a session id.
pub type SessionIdType = [u8; blake3::OUT_LEN];

/// A session id.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SessionId(Box<SessionIdType>);

impl<SessionData, const COOKIE_LENGTH: usize> Session<SessionData, COOKIE_LENGTH> {
    /// Extract the optionally associated data and expiry while consuming the session.
    ///
    /// **This function is supposed to be used in tests only.**
    /// This loses the association of the data to the actual session, making it useless for most
    /// purposes.
    pub fn into_data_expiry_pair(self) -> (Option<SessionData>, Option<SessionExpiry>) {
        self.state.into_data_expiry_pair()
    }
}

impl<SessionData: Default, const COOKIE_LENGTH: usize> Session<SessionData, COOKIE_LENGTH> {
    /// Create a new session with default data. Does not set an expiry.
    /// Using this method does not mark the session as changed, i.e. it will be silently dropped if
    /// neither the data nor the expiry are accessed mutably.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # fn main() -> typed_session::Result { use typed_session::SessionExpiry;
    /// # async_std::task::block_on(async {
    /// let session: Session<i32> = Session::new();
    /// assert_eq!(&SessionExpiry::Never, session.expiry());
    /// assert_eq!(i32::default(), *session.data());
    /// # Ok(()) }) }
    pub fn new() -> Self {
        Self {
            state: SessionState::new(),
        }
    }
}

impl<SessionData, const COOKIE_LENGTH: usize> Session<SessionData, COOKIE_LENGTH> {
    /// Create a new session with the given session data. Does not set an expiry.
    /// Using this method marks the session as changed, i.e. it will be stored in the backend and
    /// communicated to the client even if it was created with default data and never accessed mutably.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # fn main() -> typed_session::Result { use typed_session::SessionExpiry;
    /// # async_std::task::block_on(async {
    /// let session: Session<_> = Session::new_with_data(4);
    /// assert_eq!(&SessionExpiry::Never, session.expiry());
    /// assert_eq!(4, *session.data());
    /// # Ok(()) }) }
    pub fn new_with_data(data: SessionData) -> Self {
        Self {
            state: SessionState::new_with_data(data),
        }
    }

    /// **This method should only be called by a session store!**
    ///
    /// Create a session instance from parts loaded by a session store.
    /// The session state will be `Unchanged`.
    pub fn new_from_session_store(
        current_id: SessionId,
        previous_id: Option<SessionId>,
        expiry: SessionExpiry,
        data: SessionData,
    ) -> Self {
        Self {
            state: SessionState::new_from_session_store(current_id, previous_id, expiry, data),
        }
    }

    /// Returns true if this session is marked for destruction.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # fn main() -> typed_session::Result { async_std::task::block_on(async {
    /// let mut session: Session<()> = Session::new();
    /// assert!(!session.is_deleted());
    /// session.delete();
    /// assert!(session.is_deleted());
    /// # Ok(()) }) }
    pub fn is_deleted(&self) -> bool {
        self.state.is_deleted()
    }
}

impl<SessionData: Debug, const COOKIE_LENGTH: usize> Session<SessionData, COOKIE_LENGTH> {
    /// Returns the expiry timestamp of this session, if there is one.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # fn main() -> typed_session::Result { use chrono::Utc;
    /// # use typed_session::SessionExpiry;
    /// # async_std::task::block_on(async {
    /// let mut session: Session<()> = Session::new();
    /// assert_eq!(&SessionExpiry::Never, session.expiry());
    /// session.expire_in(Utc::now(), std::time::Duration::from_secs(1));
    /// assert!(matches!(session.expiry(), SessionExpiry::DateTime { .. }));
    /// # Ok(()) }) }
    /// ```
    pub fn expiry(&self) -> &SessionExpiry {
        self.state.expiry()
    }

    /// Returns a reference to the data associated with this session.
    /// This does not mark the session as changed.
    pub fn data(&self) -> &SessionData {
        self.state.data()
    }

    /// Returns a mutable reference to the data associated with this session,
    /// and marks the session as changed.
    ///
    /// Note that the session gets marked as changed, even if the returned reference is never written to.
    ///
    /// **Panics** if the session was marked for deletion before.
    pub fn data_mut(&mut self) -> &mut SessionData {
        self.state.data_mut()
    }

    /// Mark this session for destruction. the actual session record
    /// is not destroyed until the end of this response cycle.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # fn main() -> typed_session::Result { async_std::task::block_on(async {
    /// let mut session: Session<()> = Session::new();
    /// assert!(!session.is_deleted());
    /// session.delete();
    /// assert!(session.is_deleted());
    /// # Ok(()) }) }
    pub fn delete(&mut self) {
        self.state.delete();
    }

    /// Generates a new id and cookie for this session.
    pub fn regenerate(&mut self) {
        // Calling this marks the state as changed.
        self.state.change();
    }

    /// Updates the expiry timestamp of this session.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # fn main() -> typed_session::Result { use typed_session::SessionExpiry;
    /// # async_std::task::block_on(async {
    /// let mut session: Session<()> = Session::new();
    /// assert_eq!(&SessionExpiry::Never, session.expiry());
    /// session.set_expiry(chrono::Utc::now());
    /// assert!(matches!(session.expiry(), SessionExpiry::DateTime { .. }));
    /// # Ok(()) }) }
    /// ```
    pub fn set_expiry(&mut self, expiry: DateTime<Utc>) {
        *self.state.expiry_mut() = SessionExpiry::DateTime(expiry);
    }

    /// Sets this session to never expire.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # fn main() -> typed_session::Result { use typed_session::SessionExpiry;
    /// # async_std::task::block_on(async {
    /// let mut session: Session<()> = Session::new();
    /// assert_eq!(&SessionExpiry::Never, session.expiry());
    /// session.set_expiry(chrono::Utc::now());
    /// assert!(matches!(session.expiry(), SessionExpiry::DateTime { .. }));
    /// session.do_not_expire();
    /// assert!(matches!(session.expiry(), SessionExpiry::Never));
    /// # Ok(()) }) }
    /// ```
    pub fn do_not_expire(&mut self) {
        *self.state.expiry_mut() = SessionExpiry::Never;
    }

    /// Sets this session to expire `ttl` time into the future.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use typed_session::Session;
    /// # fn main() -> typed_session::Result { use chrono::Utc;
    /// # use typed_session::SessionExpiry;
    /// # async_std::task::block_on(async {
    /// let mut session: Session<()> = Session::new();
    /// assert_eq!(&SessionExpiry::Never, session.expiry());
    /// session.expire_in(Utc::now(), std::time::Duration::from_secs(1));
    /// assert!(matches!(session.expiry(), SessionExpiry::DateTime { .. }));
    /// # Ok(()) }) }
    /// ```
    pub fn expire_in(&mut self, now: DateTime<Utc>, ttl: std::time::Duration) {
        *self.state.expiry_mut() = SessionExpiry::DateTime(now + Duration::from_std(ttl).unwrap());
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
    /// # fn main() -> typed_session::Result { use chrono::Utc;
    /// # use typed_session::SessionExpiry;
    /// # async_std::task::block_on(async {
    /// let mut session: Session<()> = Session::new();
    /// assert_eq!(&SessionExpiry::Never, session.expiry());
    /// assert!(!session.is_expired(Utc::now()));
    /// session.expire_in(Utc::now(), Duration::from_secs(1));
    /// assert!(!session.is_expired(Utc::now()));
    /// task::sleep(Duration::from_secs(2)).await;
    /// assert!(session.is_expired(Utc::now()));
    /// # Ok(()) }) }
    /// ```
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        match self.state.expiry() {
            SessionExpiry::DateTime(expiry) => *expiry < now,
            SessionExpiry::Never => false,
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
    /// # fn main() -> typed_session::Result { use chrono::Utc;
    /// # async_std::task::block_on(async {
    /// let mut session: Session<()> = Session::new();
    /// session.expire_in(Utc::now(), Duration::from_secs(123));
    /// let expires_in = session.expires_in(Utc::now()).unwrap();
    /// assert!(123 - expires_in.as_secs() < 2);
    /// # Ok(()) }) }
    /// ```
    pub fn expires_in(&self, now: DateTime<Utc>) -> Option<std::time::Duration> {
        match self.state.expiry() {
            SessionExpiry::DateTime(date_time) => {
                let duration = date_time.signed_duration_since(now);
                if duration > Duration::zero() {
                    Some(duration.to_std().unwrap())
                } else {
                    None
                }
            }
            SessionExpiry::Never => None,
        }
    }
}

impl<SessionData: Default, const COOKIE_LENGTH: usize> Default
    for Session<SessionData, COOKIE_LENGTH>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<SessionData: Default> SessionState<SessionData> {
    fn new() -> Self {
        Self::NewUnchanged {
            data: Default::default(),
        }
    }
}

impl<SessionData> SessionState<SessionData> {
    fn new_with_data(data: SessionData) -> Self {
        Self::NewChanged {
            data,
            expiry: SessionExpiry::Never,
        }
    }

    fn new_from_session_store(
        current_id: SessionId,
        previous_id: Option<SessionId>,
        expiry: SessionExpiry,
        data: SessionData,
    ) -> Self {
        Self::Unchanged {
            current_id,
            previous_id,
            expiry,
            data,
        }
    }

    fn is_deleted(&self) -> bool {
        matches!(self, Self::Deleted { .. } | Self::NewDeleted)
    }

    fn into_data_expiry_pair(self) -> (Option<SessionData>, Option<SessionExpiry>) {
        match self {
            SessionState::NewUnchanged { data } => (Some(data), None),
            SessionState::NewChanged { data, expiry }
            | SessionState::Unchanged { data, expiry, .. }
            | SessionState::Changed { data, expiry, .. } => (Some(data), Some(expiry)),
            SessionState::Deleted { .. } | SessionState::NewDeleted | SessionState::Invalid => {
                (None, None)
            }
        }
    }
}

impl<SessionData: Debug> SessionState<SessionData> {
    fn expiry(&self) -> &SessionExpiry {
        match self {
            Self::NewUnchanged { .. } => &SessionExpiry::Never,
            Self::NewChanged { expiry, .. }
            | Self::Unchanged { expiry, .. }
            | Self::Changed { expiry, .. } => expiry,
            Self::Deleted { .. } | Self::NewDeleted => {
                panic!("Attempted to retrieve the expiry of a purged session {self:?}")
            }
            Self::Invalid => unreachable!("Invalid state is used internally only"),
        }
    }

    fn expiry_mut(&mut self) -> &mut SessionExpiry {
        self.change();

        match self {
            Self::NewChanged { expiry, .. } | Self::Changed { expiry, .. } => expiry,
            Self::Deleted { .. } | Self::NewDeleted => {
                panic!("Attempted to retrieve the expiry of a purged session {self:?}")
            }
            Self::NewUnchanged { .. } | Self::Unchanged { .. } => {
                unreachable!("Cannot be unchanged after explicitly changing")
            }
            Self::Invalid => unreachable!("Invalid state is used internally only"),
        }
    }

    fn data(&self) -> &SessionData {
        match self {
            Self::NewUnchanged { data }
            | Self::NewChanged { data, .. }
            | Self::Unchanged { data, .. }
            | Self::Changed { data, .. } => data,
            Self::Deleted { .. } | Self::NewDeleted => {
                panic!("Attempted to retrieve the data of a purged session {self:?}")
            }
            Self::Invalid => unreachable!("Invalid state is used internally only"),
        }
    }

    fn data_mut(&mut self) -> &mut SessionData {
        self.change();

        match self {
            Self::NewChanged { data, .. } | Self::Changed { data, .. } => data,
            Self::Deleted { .. } | Self::NewDeleted => {
                panic!("Attempted to retrieve the data of a purged session {self:?}")
            }
            Self::NewUnchanged { .. } | Self::Unchanged { .. } => {
                unreachable!("Cannot be unchanged after explicitly changing")
            }
            Self::Invalid => unreachable!("Invalid state is used internally only"),
        }
    }

    fn change(&mut self) {
        match self {
            Self::NewUnchanged { .. } => {
                let Self::NewUnchanged { data } = mem::replace(self, Self::Invalid) else {unreachable!()};
                *self = Self::NewChanged {
                    expiry: SessionExpiry::Never,
                    data,
                };
            }
            Self::Unchanged { .. } => {
                let Self::Unchanged { current_id, previous_id, expiry, data } = mem::replace(self, Self::Invalid) else {unreachable!()};
                *self = Self::Changed {
                    previous_id: current_id,
                    deletable_id: previous_id,
                    expiry,
                    data,
                };
            }
            Self::Changed { .. } | Self::NewChanged { .. } => { /* Already changed. */ }
            Self::Deleted { .. } | Self::NewDeleted => {
                panic!("Attempted to change purged session {self:?}")
            }
            Self::Invalid => unreachable!("Invalid state is used internally only"),
        }
    }

    fn delete(&mut self) {
        match self {
            Self::NewUnchanged { .. } | Self::NewChanged { .. } => {
                *self = Self::NewDeleted;
            }
            Self::Unchanged { .. } => {
                let Self::Unchanged { current_id, previous_id, .. } = mem::replace(self, Self::Invalid) else { unreachable!() };
                *self = Self::Deleted {
                    current_id,
                    previous_id,
                };
            }
            Self::Changed { .. } => {
                let Self::Changed { previous_id, deletable_id, .. } = mem::replace(self, Self::Invalid) else { unreachable!() };
                *self = Self::Deleted {
                    current_id: previous_id,
                    previous_id: deletable_id,
                };
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
    /// This is automatically done by the [`SessionStore`](crate::SessionStore), and this function is only public for test purposes.
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

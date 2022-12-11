use crate::session::{SessionId, SessionState};
use crate::{Result, Session};
use anyhow::Error;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rand::distributions::{Alphanumeric, DistString};
use rand::Rng;
use std::marker::PhantomData;

/// An async session store.
///
/// This is the user-facing interface of the session store.
/// It abstracts over CRUD-based database operations on sessions,
#[derive(Debug, Clone)]
pub struct SessionStore<Data, Implementation, const COOKIE_LENGTH: usize = 64> {
    implementation: Implementation,
    data: PhantomData<Data>,
}

/// Generate a random cookie.
fn generate_cookie<const COOKIE_LENGTH: usize>(rng: &mut impl Rng) -> String {
    let mut cookie = String::new();
    Alphanumeric.append_string(rng, &mut cookie, COOKIE_LENGTH);
    cookie
}

impl<Data, Implementation: SessionStoreImplementation<Data>, const COOKIE_LENGTH: usize>
    SessionStore<Data, Implementation, COOKIE_LENGTH>
{
    /// Create a new session store with the given implementation.
    pub fn new(implementation: Implementation) -> Self {
        Self {
            implementation,
            data: Default::default(),
        }
    }

    /// Get a session from the storage backend.
    ///
    /// The `cookie_value` is the value of a cookie identifying the session.
    /// We take it by value, as it is sensible information that should not lay around longer than necessary.
    ///
    /// The return value is `Ok(Some(_))` if there is a session identified by the given cookie that is not expired,
    /// or `Ok(None)` if there is no such session that is not expired.
    pub async fn load_session(&self, cookie_value: String) -> Result<Option<Session<Data>>> {
        let session_id = SessionId::from_cookie_value(&cookie_value);
        self.implementation.read_session(&session_id).await
    }

    /// Store a session in the storage backend.
    /// If the session is marked for deletion, this method deletes the session.
    ///
    /// If the session cookie requires to be updated, because the session data or expiry changed,
    /// then a [SetSessionCookieCommand] is returned.
    pub async fn store_session(
        &mut self,
        session: Session<Data>,
        rng: &mut impl Rng,
    ) -> Result<Option<SetSessionCookieCommand>> {
        if matches!(
            &session.state,
            SessionState::New { .. } | SessionState::Changed { .. } | SessionState::Deleted { .. }
        ) {
            if let Some(maximum_retries_on_collision) =
                Implementation::MAXIMUM_RETRIES_ON_ID_COLLISION
            {
                for _ in 0..maximum_retries_on_collision {
                    match self.try_store_session(&session, rng).await? {
                        WriteSessionResult::Ok(command) => return Ok(command),
                        WriteSessionResult::SessionIdExists => { /* continue trying */ }
                    }
                }

                Err(Error::msg(
                    "Reached the maximum number of tries when generating a session id",
                ))
            } else {
                loop {
                    match self.try_store_session(&session, rng).await? {
                        WriteSessionResult::Ok(command) => return Ok(command),
                        WriteSessionResult::SessionIdExists => { /* continue trying */ }
                    }
                }
            }
        } else {
            Ok(None)
        }
    }

    async fn try_store_session(
        &mut self,
        session: &Session<Data>,
        rng: &mut impl Rng,
    ) -> Result<WriteSessionResult<Option<SetSessionCookieCommand>>> {
        match &session.state {
            SessionState::New { expiry, data } => {
                let cookie_value = generate_cookie::<COOKIE_LENGTH>(rng);
                let id = SessionId::from_cookie_value(&cookie_value);
                Ok(self
                    .implementation
                    .create_session(&id, expiry, data)
                    .await?
                    .map(|()| {
                        Some(SetSessionCookieCommand {
                            cookie_value,
                            expiry: *expiry,
                        })
                    }))
            }
            SessionState::Changed {
                old_id,
                expiry,
                data,
            } => {
                let cookie_value = generate_cookie::<COOKIE_LENGTH>(rng);
                let id = SessionId::from_cookie_value(&cookie_value);
                Ok(self
                    .implementation
                    .update_session(old_id, &id, expiry, data)
                    .await?
                    .map(|()| {
                        Some(SetSessionCookieCommand {
                            cookie_value,
                            expiry: *expiry,
                        })
                    }))
            }
            SessionState::Deleted { id } => {
                self.implementation.delete_session(id).await?;
                Ok(WriteSessionResult::Ok(None))
            }
            SessionState::Unchanged { .. } | SessionState::NewDeleted => unreachable!(),
            SessionState::Invalid => unreachable!("Invalid state is used internally only"),
        }
    }

    /// Empties the entire store, deleting all sessions.
    pub async fn clear_store(&mut self) -> Result {
        self.implementation.clear().await
    }
}

/// This is the backend-facing interface of the session store.
/// It defines simple [CRUD]-methods on sessions.
///
/// The session id is expected to be the primary key, uniquely identifying a session.
///
/// [CRUD]: https://en.wikipedia.org/wiki/Create,_read,_update_and_delete
#[async_trait]
pub trait SessionStoreImplementation<Data> {
    /// Writing a session may fail if the id already exists.
    /// This constant indicates how often the caller should retry with different randomly generated ids until it should give up.
    /// The value `None` indicates that the caller should never give up, possibly looping infinitely.
    const MAXIMUM_RETRIES_ON_ID_COLLISION: Option<u8>;

    /// Create a session with the given `id`, `expiry` and `data`.
    async fn create_session(
        &mut self,
        id: &SessionId,
        expiry: &Option<DateTime<Utc>>,
        data: &Data,
    ) -> Result<WriteSessionResult>;

    /// Read the session with the given `id`.
    async fn read_session(&self, id: &SessionId) -> Result<Option<Session<Data>>>;

    /// Update the session with id `old_id`, replacing `old_id` with `new_id` and updating `expiry` and `data`.
    async fn update_session(
        &mut self,
        old_id: &SessionId,
        new_id: &SessionId,
        expiry: &Option<DateTime<Utc>>,
        data: &Data,
    ) -> Result<WriteSessionResult>;

    /// Delete the session with the given `id`.
    async fn delete_session(&mut self, id: &SessionId) -> Result<()>;

    /// Delete all sessions in the store.
    async fn clear(&mut self) -> Result<()>;
}

/// The result of writing a session, indicating if the session could be written, or if the id collided.
#[derive(Debug)]
pub enum WriteSessionResult<OkData = ()> {
    /// The session could be written without id collision.
    Ok(OkData),
    /// The session could not be written, because the chosen id already exists.
    SessionIdExists,
}

impl<OkData> WriteSessionResult<OkData> {
    fn map<OtherOkData>(
        self,
        f: impl FnOnce(OkData) -> OtherOkData,
    ) -> WriteSessionResult<OtherOkData> {
        match self {
            Self::Ok(data) => WriteSessionResult::Ok(f(data)),
            Self::SessionIdExists => WriteSessionResult::SessionIdExists,
        }
    }
}

/// Indicates that the client's session cookie should be updated.
#[derive(Debug)]
pub struct SetSessionCookieCommand {
    /// The value of the session cookie.
    pub cookie_value: String,
    /// The expiry time of the session cookie.
    pub expiry: Option<DateTime<Utc>>,
}

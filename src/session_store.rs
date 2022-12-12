use crate::session::{SessionId, SessionState};
use crate::{Result, Session};
use anyhow::Error;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rand::distributions::{Alphanumeric, DistString};
use rand::Rng;
use std::fmt::Debug;
use std::marker::PhantomData;

/// An async session store.
///
/// This is the user-facing interface of the session store.
/// It abstracts over CRUD-based database operations on sessions,
#[derive(Debug)]
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

    /// Store a session in the storage backend.
    /// If the session is marked for deletion, this method deletes the session.
    ///
    /// If the session cookie requires to be updated, because the session data or expiry changed,
    /// then a [SessionCookieCommand] is returned.
    pub async fn store_session(
        &mut self,
        session: Session<Data>,
        rng: &mut impl Rng,
    ) -> Result<SessionCookieCommand> {
        if matches!(
            &session.state,
            SessionState::NewChanged { .. }
                | SessionState::Changed { .. }
                | SessionState::Deleted { .. }
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
            Ok(SessionCookieCommand::DoNothing)
        }
    }

    async fn try_store_session(
        &mut self,
        session: &Session<Data>,
        rng: &mut impl Rng,
    ) -> Result<WriteSessionResult<SessionCookieCommand>> {
        match &session.state {
            SessionState::NewChanged { expiry, data } => {
                let cookie_value = generate_cookie::<COOKIE_LENGTH>(rng);
                let id = SessionId::from_cookie_value(&cookie_value);
                Ok(self
                    .implementation
                    .create_session(&id, expiry, data)
                    .await?
                    .map(|()| SessionCookieCommand::Set {
                        cookie_value,
                        expiry: *expiry,
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
                    .map(|()| SessionCookieCommand::Set {
                        cookie_value,
                        expiry: *expiry,
                    }))
            }
            SessionState::Deleted { id } => {
                self.implementation.delete_session(id).await?;
                Ok(WriteSessionResult::Ok(SessionCookieCommand::Delete))
            }
            SessionState::NewUnchanged { .. }
            | SessionState::Unchanged { .. }
            | SessionState::NewDeleted => unreachable!(),
            SessionState::Invalid => unreachable!("Invalid state is used internally only"),
        }
    }

    /// Empties the entire store, deleting all sessions.
    pub async fn clear_store(&mut self) -> Result {
        self.implementation.clear().await
    }
}

impl<Data: Debug, Implementation: SessionStoreImplementation<Data>, const COOKIE_LENGTH: usize>
    SessionStore<Data, Implementation, COOKIE_LENGTH>
{
    /// Get a session from the storage backend.
    ///
    /// The `cookie_value` is the value of a cookie identifying the session.
    ///
    /// The return value is `Ok(Some(_))` if there is a session identified by the given cookie that is not expired,
    /// or `Ok(None)` if there is no such session that is not expired.
    pub async fn load_session(
        &self,
        cookie_value: impl AsRef<str>,
    ) -> Result<Option<Session<Data>>> {
        let session_id = SessionId::from_cookie_value(cookie_value.as_ref());
        Ok(self
            .implementation
            .read_session(&session_id)
            .await?
            // We could delete expired sessions here, but that does not make sense:
            // the client will not purposefully send us an expired session cookie, so only in the unlikely
            // event that the session expires while being transmitted this will actually be triggered.
            .filter(|session| !session.is_expired()))
    }
}

impl<Data, Implementation: Clone, const COOKIE_LENGTH: usize> Clone
    for SessionStore<Data, Implementation, COOKIE_LENGTH>
{
    fn clone(&self) -> Self {
        Self {
            implementation: self.implementation.clone(),
            data: self.data,
        }
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

/// Indicates if the client's session cookie should be updated.
#[derive(Debug)]
pub enum SessionCookieCommand {
    /// Set or update the session cookie.
    Set {
        /// The value of the session cookie.
        cookie_value: String,
        /// The expiry time of the session cookie.
        expiry: Option<DateTime<Utc>>,
    },
    /// Delete the session cookie.
    Delete,
    /// Do not inform the client about any updates to the session cookie.
    /// This means that the cookie stayed the same.
    DoNothing,
}

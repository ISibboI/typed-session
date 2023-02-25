use crate::session::{SessionId, SessionState};
use crate::session_store::cookie_generator::SessionCookieGenerator;
use crate::{DefaultSessionCookieGenerator, Result, Session, SessionExpiry};
use anyhow::Error;
use async_trait::async_trait;
use chrono::Duration;
use chrono::Utc;
use std::fmt::Debug;
use std::marker::PhantomData;

pub(crate) mod cookie_generator;

/// An async session store.
///
/// This is the "front-end" interface of the session store.
///
/// `SessionData` is the data associated with a session.
/// `SessionStoreConnection` is the connection to the backend session store.
/// `COOKIE_LENGTH` is the length of the session cookie, in characters.
/// It should be a multiple of 32, which is the block size of blake3.
/// `CookieGenerator` is the type used to generate random session cookies.
#[derive(Debug)]
pub struct SessionStore<
    SessionData,
    Implementation,
    const COOKIE_LENGTH: usize = 64,
    CookieGenerator = DefaultSessionCookieGenerator<COOKIE_LENGTH>,
> {
    implementation: Implementation,
    cookie_generator: CookieGenerator,
    expiry_strategy: SessionRenewalStrategy,
    data: PhantomData<SessionData>,
}

/// The strategy to renew sessions.
#[derive(Clone, Copy, Debug)]
pub enum SessionRenewalStrategy {
    /// Never update the expiry of a session.
    /// This leaves updating expiry times to the user.
    Ignore,

    /// Sessions have a given time-to-live, and their expiry is renewed periodically.
    /// For example, if the TTL is 7 days, and the maximum remaining TTL for renewal is 6 days,
    /// then the session's expiry will be updated about daily, if the session is being used.
    AutomaticRenewal {
        /// The time-to-live for a new or renewed session.
        time_to_live: Duration,
        /// The maximum remaining time-to-live to trigger a session renewal.
        maximum_remaining_time_to_live_for_renewal: Duration,
    },
}

impl<SessionData, Implementation, CookieGenerator, const COOKIE_LENGTH: usize>
    SessionStore<SessionData, Implementation, COOKIE_LENGTH, CookieGenerator>
{
    /// Consume the `SessionStore` and return the wrapped `Implementation`.
    pub fn into_inner(self) -> Implementation {
        self.implementation
    }
}

impl<SessionData, Implementation, const COOKIE_LENGTH: usize>
    SessionStore<
        SessionData,
        Implementation,
        COOKIE_LENGTH,
        DefaultSessionCookieGenerator<COOKIE_LENGTH>,
    >
{
    /// Create a new session store with the given implementation, cookie generator and session renewal strategy.
    pub fn new(implementation: Implementation, expiry_strategy: SessionRenewalStrategy) -> Self {
        Self {
            implementation,
            cookie_generator: Default::default(),
            expiry_strategy,
            data: Default::default(),
        }
    }
}

impl<SessionData, Implementation, const COOKIE_LENGTH: usize, CookieGenerator>
    SessionStore<SessionData, Implementation, COOKIE_LENGTH, CookieGenerator>
{
    /// Create a new session store with the given implementation, cookie generator and session renewal strategy.
    pub fn new_with_cookie_generator(
        implementation: Implementation,
        cookie_generator: CookieGenerator,
        expiry_strategy: SessionRenewalStrategy,
    ) -> Self {
        Self {
            implementation,
            cookie_generator,
            expiry_strategy,
            data: Default::default(),
        }
    }
}

impl<
        SessionData,
        Implementation: SessionStoreImplementation<SessionData>,
        const COOKIE_LENGTH: usize,
        CookieGenerator: SessionCookieGenerator<COOKIE_LENGTH>,
    > SessionStore<SessionData, Implementation, COOKIE_LENGTH, CookieGenerator>
{
    /// Store a session in the storage backend.
    /// If the session is marked for deletion, this method deletes the session.
    ///
    /// If the session cookie requires to be updated, because the session data or expiry changed,
    /// then a [SessionCookieCommand] is returned.
    pub async fn store_session(
        &mut self,
        session: Session<SessionData>,
    ) -> Result<SessionCookieCommand> {
        if matches!(
            &session.state,
            SessionState::NewChanged { .. }
                | SessionState::Changed { .. }
                | SessionState::Deleted { .. }
        ) {
            if let Some(maximum_retries_on_collision) =
                self.implementation.maximum_retries_on_id_collision()
            {
                for _ in 0..maximum_retries_on_collision {
                    match self.try_store_session(&session).await? {
                        WriteSessionResult::Ok(command) => return Ok(command),
                        WriteSessionResult::SessionIdExists => { /* continue trying */ }
                    }
                }

                Err(Error::msg(
                    "Reached the maximum number of tries when generating a session id",
                ))
            } else {
                loop {
                    match self.try_store_session(&session).await? {
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
        session: &Session<SessionData>,
    ) -> Result<WriteSessionResult<SessionCookieCommand>> {
        match &session.state {
            SessionState::NewChanged { expiry, data } => {
                let cookie_value = self.cookie_generator.generate_cookie();
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
                previous_id,
                deletable_id,
                expiry,
                data,
            } => {
                let cookie_value = self.cookie_generator.generate_cookie();
                let current_id = SessionId::from_cookie_value(&cookie_value);
                Ok(self
                    .implementation
                    .update_session(&current_id, previous_id, deletable_id, expiry, data)
                    .await?
                    .map(|()| SessionCookieCommand::Set {
                        cookie_value,
                        expiry: *expiry,
                    }))
            }
            SessionState::Deleted {
                current_id,
                previous_id,
            } => {
                self.implementation
                    .delete_session(current_id, previous_id)
                    .await?;
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

impl<
        SessionData: Debug,
        Implementation: SessionStoreImplementation<SessionData>,
        const COOKIE_LENGTH: usize,
        CookieGenerator,
    > SessionStore<SessionData, Implementation, COOKIE_LENGTH, CookieGenerator>
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
    ) -> Result<Option<Session<SessionData>>> {
        let session_id = SessionId::from_cookie_value(cookie_value.as_ref());
        if let Some(mut session) = self.implementation.read_session(&session_id).await? {
            let now = Utc::now();
            if session.is_expired(now) {
                // We could delete expired sessions here, but that does not make sense:
                // the client will not purposefully send us an expired session cookie, so only in the unlikely
                // event that the session expires while being transmitted this will actually be triggered.
                return Ok(None);
            }

            match &self.expiry_strategy {
                SessionRenewalStrategy::Ignore => {}
                SessionRenewalStrategy::AutomaticRenewal {
                    time_to_live,
                    maximum_remaining_time_to_live_for_renewal,
                } => {
                    let new_expiry = now + *time_to_live;
                    match *session.expiry() {
                        SessionExpiry::DateTime(old_expiry) => {
                            // Renew only if within maximum remaining time.
                            if old_expiry - now <= *maximum_remaining_time_to_live_for_renewal {
                                session.set_expiry(new_expiry);
                            }
                        }
                        // Always renew if the expiry is set to never, otherwise the session will never expire.
                        SessionExpiry::Never => session.set_expiry(new_expiry),
                    }
                }
            }

            Ok(Some(session))
        } else {
            Ok(None)
        }
    }
}

impl<SessionData, Implementation: Clone, const COOKIE_LENGTH: usize, CookieGenerator: Clone> Clone
    for SessionStore<SessionData, Implementation, COOKIE_LENGTH, CookieGenerator>
{
    fn clone(&self) -> Self {
        Self {
            implementation: self.implementation.clone(),
            cookie_generator: self.cookie_generator.clone(),
            expiry_strategy: self.expiry_strategy,
            data: self.data,
        }
    }
}

/// This is the backend-facing interface of the session store.
/// It defines simple [CRUD]-methods on sessions.
///
/// Sessions are identified by up to two session ids (`current_id` and `previous_id`) to handle session renewal under concurrent requests.
/// Otherwise, the following may happen:
///  * The client sends requests `A` and `B` with session id `X`.
///  * We handle request `A`, renewing the session id to `Y`.
///  * Then we handle request `B`. But request `B` was sent with the old session id `X`, so it now fails.
///
/// The session store must ensure that there is never any overlap between the ids,
/// i.e. the multiset of all current and previous ids must contain each id at most once.
///
/// [CRUD]: https://en.wikipedia.org/wiki/Create,_read,_update_and_delete
#[async_trait]
pub trait SessionStoreImplementation<SessionData> {
    /// Writing a session may fail if the id already exists.
    /// This constant indicates how often the caller should retry with different randomly generated ids until it should give up.
    /// The value `None` indicates that the caller should never give up, possibly looping infinitely.
    fn maximum_retries_on_id_collision(&self) -> Option<u32>;

    /// Create a session with the given `current_id`, `expiry` and `data`.
    /// The `previous_id` stays unset.
    async fn create_session(
        &mut self,
        current_id: &SessionId,
        expiry: &SessionExpiry,
        data: &SessionData,
    ) -> Result<WriteSessionResult>;

    /// Read the session with the given `id`.
    /// This must return the session that either has `previous_id == id` or `current_id == id`.
    /// Older session ids must not be considered.
    async fn read_session(&self, id: &SessionId) -> Result<Option<Session<SessionData>>>;

    /// Update a session with new ids, data and expiry.
    ///
    /// This method must be implemented as follows:
    ///  1. Find a session `A` identified by the given `previous_id`. The session must have either `previous_id == id` or `current_id == id`.
    ///  2. Set `A.current_id = current_id` and `A.previous_id = previous_id`. Optionally, remove `A`'s association with `deletable_id`.
    ///  3. Set `A.expiry = expiry` and `A.data = data`.
    async fn update_session(
        &mut self,
        current_id: &SessionId,
        previous_id: &SessionId,
        deletable_id: &Option<SessionId>,
        expiry: &SessionExpiry,
        data: &SessionData,
    ) -> Result<WriteSessionResult>;

    /// Delete the session with the given `current_id` and optionally `previous_id`.
    async fn delete_session(
        &mut self,
        current_id: &SessionId,
        previous_id: &Option<SessionId>,
    ) -> Result<()>;

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
#[derive(Debug, Eq, PartialEq)]
pub enum SessionCookieCommand {
    /// Set or update the session cookie.
    Set {
        /// The value of the session cookie.
        cookie_value: String,
        /// The expiry time of the session cookie.
        expiry: SessionExpiry,
    },
    /// Delete the session cookie.
    Delete,
    /// Do not inform the client about any updates to the session cookie.
    /// This means that the cookie stayed the same.
    DoNothing,
}

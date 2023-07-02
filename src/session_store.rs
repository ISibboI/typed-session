use crate::session::{SessionId, SessionState};
use crate::session_store::cookie_generator::SessionCookieGenerator;
use crate::{error::Result, DefaultSessionCookieGenerator, Error, Session, SessionExpiry};
use async_trait::async_trait;
use chrono::Utc;
use chrono::{DateTime, Duration};
use std::fmt::Debug;
use std::marker::PhantomData;

pub(crate) mod cookie_generator;

/// An async session store.
///
/// This is the "front-end" interface of the session store.
///
/// `SessionData` is the data associated with a session.
/// `SessionStoreConnection` is the connection to the backend session store.
/// `CookieGenerator` is the type used to generate random session cookies.
#[derive(Debug)]
pub struct SessionStore<
    SessionData,
    SessionStoreConnection,
    CookieGenerator = DefaultSessionCookieGenerator,
> {
    implementation: SessionStoreConnection,
    cookie_generator: CookieGenerator,
    session_renewal_strategy: SessionRenewalStrategy,
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

impl<SessionData, SessionStoreConnection, CookieGenerator>
    SessionStore<SessionData, SessionStoreConnection, CookieGenerator>
{
    /// Consume the `SessionStore` and return the wrapped `SessionStoreConnection`.
    pub fn into_inner(self) -> SessionStoreConnection {
        self.implementation
    }
}

impl<SessionData, SessionStoreConnection>
    SessionStore<SessionData, SessionStoreConnection, DefaultSessionCookieGenerator>
{
    /// Create a new session store with the given implementation, cookie generator and session renewal strategy.
    pub fn new(
        implementation: SessionStoreConnection,
        expiry_strategy: SessionRenewalStrategy,
    ) -> Self {
        Self {
            implementation,
            cookie_generator: Default::default(),
            session_renewal_strategy: expiry_strategy,
            data: Default::default(),
        }
    }
}

impl<SessionData, SessionStoreConnection, CookieGenerator>
    SessionStore<SessionData, SessionStoreConnection, CookieGenerator>
{
    /// Create a new session store with the given implementation, cookie generator and session renewal strategy.
    pub fn new_with_cookie_generator(
        implementation: SessionStoreConnection,
        cookie_generator: CookieGenerator,
        session_renewal_strategy: SessionRenewalStrategy,
    ) -> Self {
        Self {
            implementation,
            cookie_generator,
            session_renewal_strategy,
            data: Default::default(),
        }
    }

    /// A reference to the session renewal strategy of this session store.
    pub fn session_renewal_strategy(&self) -> &SessionRenewalStrategy {
        &self.session_renewal_strategy
    }

    /// A mutable reference to the session renewal strategy of this session store.
    pub fn session_renewal_strategy_mut(&mut self) -> &mut SessionRenewalStrategy {
        &mut self.session_renewal_strategy
    }
}

impl<
        SessionData: Debug,
        SessionStoreConnection: SessionStoreConnector<SessionData>,
        CookieGenerator: SessionCookieGenerator,
    > SessionStore<SessionData, SessionStoreConnection, CookieGenerator>
{
    /// Store a session in the storage backend.
    /// If the session is marked for deletion, this method deletes the session.
    ///
    /// If the session cookie requires to be updated, because the session data or expiry changed,
    /// then a [SessionCookieCommand] is returned.
    pub async fn store_session(
        &self,
        mut session: Session<SessionData>,
    ) -> Result<SessionCookieCommand> {
        if matches!(
            &session.state,
            SessionState::NewChanged { .. }
                | SessionState::Changed { .. }
                | SessionState::Deleted { .. }
        ) {
            // If we store a new session, we need to update its expiry.
            // In all other cases, the expiry is updated when loading the session.
            // This allows the user to see the current session expiry by inspecting the session.
            if matches!(&session.state, SessionState::NewChanged { .. }) {
                self.session_renewal_strategy
                    .apply_to_session(&mut session, Utc::now());
            }

            if let Some(maximum_retries_on_collision) =
                self.implementation.maximum_retries_on_id_collision()
            {
                for _ in 0..maximum_retries_on_collision {
                    match self.try_store_session(&session).await? {
                        WriteSessionResult::Ok(command) => return Ok(command),
                        WriteSessionResult::SessionIdExists => { /* continue trying */ }
                    }
                }

                Err(Error::MaximumSessionIdGenerationTriesReached)
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
        &self,
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
                current_id: previous_id,
                expiry,
                data,
            } => {
                let cookie_value = self.cookie_generator.generate_cookie();
                let current_id = SessionId::from_cookie_value(&cookie_value);
                Ok(self
                    .implementation
                    .update_session(&current_id, previous_id, expiry, data)
                    .await?
                    .map(|()| SessionCookieCommand::Set {
                        cookie_value,
                        expiry: *expiry,
                    }))
            }
            SessionState::Deleted { current_id } => {
                self.implementation.delete_session(current_id).await?;
                Ok(WriteSessionResult::Ok(SessionCookieCommand::Delete))
            }
            SessionState::NewUnchanged { .. }
            | SessionState::Unchanged { .. }
            | SessionState::NewDeleted => unreachable!(),
            SessionState::Invalid => unreachable!("Invalid state is used internally only"),
        }
    }

    /// Empties the entire store, deleting all sessions.
    pub async fn clear_store(&self) -> Result {
        self.implementation.clear().await
    }
}

impl<
        SessionData: Debug,
        SessionStoreConnection: SessionStoreConnector<SessionData>,
        CookieGenerator,
    > SessionStore<SessionData, SessionStoreConnection, CookieGenerator>
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

            self.session_renewal_strategy
                .apply_to_session(&mut session, now);

            Ok(Some(session))
        } else {
            Ok(None)
        }
    }
}

impl<SessionData, SessionStoreConnection: Clone, CookieGenerator: Clone> Clone
    for SessionStore<SessionData, SessionStoreConnection, CookieGenerator>
{
    fn clone(&self) -> Self {
        Self {
            implementation: self.implementation.clone(),
            cookie_generator: self.cookie_generator.clone(),
            session_renewal_strategy: self.session_renewal_strategy,
            data: self.data,
        }
    }
}

/// This is the backend-facing interface of the session store.
/// It defines simple [CRUD]-methods on sessions.
///
/// This type must be `Clone` and thread safe (i.e. `Send` and `Sync`).
/// Different cloned implementations of this trait should not block each other, but should allow
/// concurrent queries through the different instances.
/// This is to allow the whole [`SessionStore`] to be cloned and used concurrently, e.g. by a
/// parallel or at least concurrent server application.
///
/// Sessions are identified by a session id (`current_id`).
/// The session store must ensure that there is never any overlap between the ids.
///
/// [CRUD]: https://en.wikipedia.org/wiki/Create,_read,_update_and_delete
#[async_trait]
pub trait SessionStoreConnector<SessionData>: Clone + Send + Sync {
    /// Writing a session may fail if the id already exists.
    /// This constant indicates how often the caller should retry with different randomly generated ids until it should give up.
    /// The value `None` indicates that the caller should never give up, possibly looping infinitely.
    fn maximum_retries_on_id_collision(&self) -> Option<u32>;

    /// Create a session with the given `current_id`, `expiry` and `data`.
    async fn create_session(
        &self,
        current_id: &SessionId,
        expiry: &SessionExpiry,
        data: &SessionData,
    ) -> Result<WriteSessionResult>;

    /// Read the session with the given `id`.
    async fn read_session(&self, id: &SessionId) -> Result<Option<Session<SessionData>>>;

    /// Update a session with new ids, data and expiry.
    ///
    /// This method must be implemented as follows:
    ///  1. Find the session `A` identified by the given `previous_id`.
    ///  2. Remap `A` to be identified by `current_id` instead of `previous_id`.
    ///  3. Set `A.expiry = expiry` and `A.data = data`.
    ///
    /// **Security:** To avoid race conditions, this method must not allow concurrent updates of a session id.
    /// It must never happen that by updating a session id `X` concurrently, there are suddenly two different session ids `Y` and `Z` stemming both from `X`.
    /// Instead, one of the updates must fail.
    async fn update_session(
        &self,
        current_id: &SessionId,
        previous_id: &SessionId,
        expiry: &SessionExpiry,
        data: &SessionData,
    ) -> Result<WriteSessionResult>;

    /// Delete the session with the given `id`.
    async fn delete_session(&self, id: &SessionId) -> Result<()>;

    /// Delete all sessions in the store.
    async fn clear(&self) -> Result<()>;
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
/// Annotated with `#[must_use]`, because silently dropping this very likely indicates that the communication of the session to the client was forgotten about.
#[derive(Debug, Eq, PartialEq)]
#[must_use]
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

impl SessionRenewalStrategy {
    fn apply_to_session<SessionData: Debug>(
        &self,
        session: &mut Session<SessionData>,
        now: DateTime<Utc>,
    ) {
        match self {
            SessionRenewalStrategy::Ignore => { /* do nothing */ }
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
    }
}

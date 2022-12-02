use crate::session_store::WriteSessionResult;
use crate::{async_trait, Result, Session, SessionId, SessionStoreImplementation};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::fmt::Debug;

/// # in-memory session store
/// Because there is no external
/// persistence, this session store is ephemeral and will be cleared
/// on server restart.
///
/// # ***READ THIS BEFORE USING IN A PRODUCTION DEPLOYMENT***
///
/// Storing sessions only in memory brings the following problems:
///
/// 1. All sessions must fit in available memory (important for high load services)
/// 2. Sessions stored in memory are cleared only if a client calls [MemoryStore::destroy_session] or [MemoryStore::clear_store].
///    If sessions are not cleaned up properly it might result in OOM
/// 3. All sessions will be lost on shutdown
/// 4. If the service is clustered particular session will be stored only on a single instance.
///    This might be solved by using load balancers with sticky sessions.
///    Unfortunately, this solution brings additional complexity especially if the connection is
///    using secure transport since the load balancer has to perform SSL termination to understand
///    where should it forward packets to
///
#[derive(Debug, Clone)]
pub struct MemoryStore<Data> {
    session_map: HashMap<SessionId, SessionBody<Data>>,
}

#[derive(Debug, Clone)]
struct SessionBody<Data> {
    expiry: Option<DateTime<Utc>>,
    data: Data,
}

#[async_trait]
impl<Data: Send + Sync + Clone> SessionStoreImplementation<Data> for MemoryStore<Data> {
    const MAXIMUM_RETRIES_ON_ID_COLLISION: Option<u8> = None;

    async fn create_session(
        &mut self,
        id: &SessionId,
        expiry: &Option<DateTime<Utc>>,
        data: &Data,
    ) -> Result<WriteSessionResult> {
        // replace with `try_insert` once stable #82766
        if self.session_map.contains_key(id) {
            Ok(WriteSessionResult::SessionIdExists)
        } else {
            self.session_map
                .insert(id.clone(), SessionBody::new_cloned(expiry, data));
            Ok(WriteSessionResult::Ok(()))
        }
    }

    async fn read_session(&self, id: &SessionId) -> Result<Option<Session<Data>>> {
        Ok(self.session_map.get(id).map(|body| {
            Session::new_from_session_store(id.clone(), body.expiry, body.data.clone())
        }))
    }

    async fn update_session(
        &mut self,
        old_id: &SessionId,
        new_id: &SessionId,
        expiry: &Option<DateTime<Utc>>,
        data: &Data,
    ) -> Result<WriteSessionResult> {
        if self.session_map.contains_key(new_id) {
            Ok(WriteSessionResult::SessionIdExists)
        } else {
            self.session_map.remove(old_id);
            self.session_map
                .insert(new_id.clone(), SessionBody::new_cloned(expiry, data));
            Ok(WriteSessionResult::Ok(()))
        }
    }

    async fn delete_session(&mut self, id: &SessionId) -> Result<()> {
        self.session_map.remove(id);
        Ok(())
    }

    async fn clear(&mut self) -> Result<()> {
        self.session_map.clear();
        Ok(())
    }
}

impl<Data> MemoryStore<Data> {
    /// Create a new empty memory store.
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns the number of elements in the memory store.
    pub fn len(&self) -> usize {
        self.session_map.len()
    }

    /// Returns true if the memory store is empty.
    pub fn is_empty(&self) -> bool {
        self.session_map.is_empty()
    }

    /// Performs session cleanup. This should be run on an
    /// intermittent basis if this store is run for long enough that
    /// memory accumulation is a concern.
    pub async fn cleanup(&mut self) -> Result {
        log::trace!("Cleaning up memory store...");
        let now = Utc::now();
        let initial_len = self.session_map.len();
        self.session_map
            .retain(|_, body| body.expiry.map(|expiry| expiry > now).unwrap_or(true));
        log::trace!(
            "Deleted {} expired sessions",
            initial_len - self.session_map.len()
        );
        Ok(())
    }
}

impl<Data: Clone> SessionBody<Data> {
    fn new_cloned(expiry: &Option<DateTime<Utc>>, data: &Data) -> Self {
        Self {
            expiry: *expiry,
            data: data.clone(),
        }
    }
}

impl<Data> Default for MemoryStore<Data> {
    fn default() -> Self {
        Self {
            session_map: Default::default(),
        }
    }
}

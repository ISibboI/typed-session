use crate::session_store::WriteSessionResult;
use crate::{Result, Session, SessionExpiry, SessionId, SessionStoreImplementation};
use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

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
    session_map: HashMap<SessionId, Arc<SessionBody<Data>>>,
}

#[derive(Debug, Clone)]
struct SessionBody<Data> {
    current_id: SessionId,
    previous_id: Option<SessionId>,
    expiry: SessionExpiry,
    data: Data,
}

#[async_trait]
impl<Data: Send + Sync + Clone> SessionStoreImplementation<Data> for MemoryStore<Data> {
    const MAXIMUM_RETRIES_ON_ID_COLLISION: Option<u8> = None;

    async fn create_session(
        &mut self,
        id: &SessionId,
        expiry: &SessionExpiry,
        data: &Data,
    ) -> Result<WriteSessionResult> {
        // replace with `try_insert` once stable #82766
        if self.session_map.contains_key(id) {
            Ok(WriteSessionResult::SessionIdExists)
        } else {
            self.session_map.insert(
                id.clone(),
                Arc::new(SessionBody::new_cloned(id, None, expiry, data)),
            );
            Ok(WriteSessionResult::Ok(()))
        }
    }

    async fn read_session(&self, id: &SessionId) -> Result<Option<Session<Data>>> {
        Ok(self.session_map.get(id).map(|body| {
            Session::new_from_session_store(
                body.current_id.clone(),
                body.previous_id.clone(),
                body.expiry,
                body.data.clone(),
            )
        }))
    }

    async fn update_session(
        &mut self,
        current_id: &SessionId,
        previous_id: &SessionId,
        deletable_id: &Option<SessionId>,
        expiry: &SessionExpiry,
        data: &Data,
    ) -> Result<WriteSessionResult> {
        if self.session_map.contains_key(previous_id) {
            Ok(WriteSessionResult::SessionIdExists)
        } else {
            if let Some(deletable_id) = deletable_id {
                self.session_map.remove(deletable_id);
            }

            let body = Arc::new(SessionBody::new_cloned(
                current_id,
                Some(previous_id),
                expiry,
                data,
            ));
            self.session_map.insert(current_id.clone(), body.clone());
            self.session_map.insert(previous_id.clone(), body);
            Ok(WriteSessionResult::Ok(()))
        }
    }

    async fn delete_session(
        &mut self,
        current_id: &SessionId,
        previous_id: &Option<SessionId>,
    ) -> Result<()> {
        self.session_map.remove(current_id);
        if let Some(previous_id) = previous_id.as_ref() {
            self.session_map.remove(previous_id);
        }
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
        tracing::trace!("Cleaning up memory store...");
        let now = Utc::now();
        let initial_len = self.session_map.len();
        self.session_map.retain(|_, body| match body.expiry {
            SessionExpiry::DateTime(expiry) => expiry > now,
            SessionExpiry::Never => true,
        });
        tracing::trace!(
            "Deleted {} expired sessions",
            initial_len - self.session_map.len()
        );
        Ok(())
    }
}

impl<Data: Clone> SessionBody<Data> {
    fn new_cloned(
        current_id: &SessionId,
        previous_id: Option<&SessionId>,
        expiry: &SessionExpiry,
        data: &Data,
    ) -> Self {
        Self {
            current_id: current_id.clone(),
            previous_id: previous_id.cloned(),
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

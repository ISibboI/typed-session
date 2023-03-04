use crate::session_store::WriteSessionResult;
use crate::{Result, Session, SessionExpiry, SessionId, SessionStoreConnector};
use anyhow::Error;
use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

/// # In-memory session store
///
/// This store stores sessions in memory, without any persistence. It is intended to be used for debugging purposes.
/// Sessions are deleted only when calling [delete_session](MemoryStore::delete_session)
/// or when they are expired and [delete_expired_sessions](MemoryStore::delete_expired_sessions) is called.
#[derive(Debug)]
pub struct MemoryStore<SessionData, OperationLogger> {
    store: Arc<Mutex<MemoryStoreData<SessionData, OperationLogger>>>,
}

#[derive(Debug)]
struct MemoryStoreData<SessionData, OperationLogger> {
    session_map: HashMap<SessionId, SessionBody<SessionData>>,
    operation_logger: OperationLogger,
    maximum_retries_on_id_collision: Option<u32>,
}

#[derive(Debug, Clone)]
struct SessionBody<SessionData> {
    current_id: SessionId,
    expiry: SessionExpiry,
    data: SessionData,
}

#[async_trait]
impl<
        SessionData: Send + Sync + Clone,
        OperationLogger: Send + Sync + MemoryStoreOperationLogger<SessionData>,
    > SessionStoreConnector<SessionData> for MemoryStore<SessionData, OperationLogger>
{
    fn maximum_retries_on_id_collision(&self) -> Option<u32> {
        self.store.lock().unwrap().maximum_retries_on_id_collision
    }

    async fn create_session(
        &self,
        id: &SessionId,
        expiry: &SessionExpiry,
        data: &SessionData,
    ) -> Result<WriteSessionResult> {
        let mut store = self.store.lock().unwrap();
        store.operation_logger.log_create_session(id, expiry, data);

        // replace with `try_insert` once stable #82766
        if store.session_map.contains_key(id) {
            Ok(WriteSessionResult::SessionIdExists)
        } else {
            store
                .session_map
                .insert(id.clone(), SessionBody::new_cloned(id, expiry, data));
            Ok(WriteSessionResult::Ok(()))
        }
    }

    async fn read_session(&self, id: &SessionId) -> Result<Option<Session<SessionData>>> {
        let store = self.store.lock().unwrap();
        store.operation_logger.log_read_session(id);

        Ok(store.session_map.get(id).map(|body| {
            Session::new_from_session_store(body.current_id.clone(), body.expiry, body.data.clone())
        }))
    }

    async fn update_session(
        &self,
        current_id: &SessionId,
        previous_id: &SessionId,
        expiry: &SessionExpiry,
        data: &SessionData,
    ) -> Result<WriteSessionResult> {
        let mut store = self.store.lock().unwrap();
        store
            .operation_logger
            .log_update_session(current_id, previous_id, expiry, data);

        if store.session_map.contains_key(current_id) {
            Ok(WriteSessionResult::SessionIdExists)
        } else if let Some(mut session_body) = store.session_map.remove(previous_id) {
            session_body.current_id = current_id.clone();
            session_body.expiry = *expiry;
            session_body.data = data.clone();

            store.session_map.insert(current_id.clone(), session_body);
            Ok(WriteSessionResult::Ok(()))
        } else {
            Err(Error::msg("Tried to update a non-existing session"))
        }
    }

    async fn delete_session(&self, id: &SessionId) -> Result<()> {
        let mut store = self.store.lock().unwrap();
        store.operation_logger.log_delete_session(id);

        store.session_map.remove(id);
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        let mut store = self.store.lock().unwrap();
        store.operation_logger.log_clear();
        store.session_map.clear();
        Ok(())
    }
}

impl<SessionData, OperationLogger> MemoryStore<SessionData, OperationLogger> {
    /// Sets the maximum retries on id collision, see [SessionStoreConnector::maximum_retries_on_id_collision] for details.
    pub fn set_maximum_retries_on_id_collision(
        &mut self,
        maximum_retries_on_id_collision: Option<u32>,
    ) {
        self.store.lock().unwrap().maximum_retries_on_id_collision =
            maximum_retries_on_id_collision;
    }

    /// Returns the number of elements in the memory store.
    pub fn len(&self) -> usize {
        self.store.lock().unwrap().session_map.len()
    }

    /// Returns true if the memory store is empty.
    pub fn is_empty(&self) -> bool {
        self.store.lock().unwrap().session_map.is_empty()
    }

    /// Deletes all expired sessions.
    pub fn delete_expired_sessions(&mut self) -> Result {
        let mut store = self.store.lock().unwrap();
        tracing::trace!("Cleaning up memory store...");
        let now = Utc::now();
        let initial_len = store.session_map.len();
        store.session_map.retain(|_, body| match body.expiry {
            SessionExpiry::DateTime(expiry) => expiry > now,
            SessionExpiry::Never => true,
        });
        tracing::trace!(
            "Deleted {} expired sessions",
            initial_len - store.session_map.len()
        );
        Ok(())
    }

    /// Consumes the store and returns the logged operations.
    pub fn into_logger(self) -> OperationLogger
    where
        SessionData: Debug,
        OperationLogger: Debug,
    {
        Arc::try_unwrap(self.store)
            .unwrap()
            .into_inner()
            .unwrap()
            .operation_logger
    }
}

impl<SessionData: Clone, OperationLogger> MemoryStore<SessionData, OperationLogger> {
    /// Returns an iterator over all sessions in the store.
    pub fn for_each(&self, f: impl FnMut(Session<SessionData>)) {
        self.store
            .lock()
            .unwrap()
            .session_map
            .iter()
            .map(|(id, body)| {
                Session::new_from_session_store(id.clone(), body.expiry, body.data.clone())
            })
            .for_each(f);
    }
}

impl<SessionData> MemoryStore<SessionData, NoLogger> {
    /// Create a new empty memory store.
    pub fn new() -> Self {
        MemoryStoreData {
            session_map: Default::default(),
            operation_logger: NoLogger,
            maximum_retries_on_id_collision: None,
        }
        .into()
    }
}

impl<SessionData> MemoryStore<SessionData, DefaultLogger<SessionData>> {
    /// Create a new empty memory store with the given logger for logging store operations.
    pub fn new_with_logger() -> Self {
        MemoryStoreData {
            session_map: Default::default(),
            operation_logger: Default::default(),
            maximum_retries_on_id_collision: None,
        }
        .into()
    }
}

impl<SessionData: Clone> SessionBody<SessionData> {
    fn new_cloned(current_id: &SessionId, expiry: &SessionExpiry, data: &SessionData) -> Self {
        Self {
            current_id: current_id.clone(),
            expiry: *expiry,
            data: data.clone(),
        }
    }
}

impl<SessionData, OperationLogger: Default> Default for MemoryStore<SessionData, OperationLogger> {
    fn default() -> Self {
        MemoryStoreData {
            session_map: Default::default(),
            operation_logger: Default::default(),
            maximum_retries_on_id_collision: None,
        }
        .into()
    }
}

/// A logger for operations conducted by the memory store.
/// This is intended to be used for debug purposes.
pub trait MemoryStoreOperationLogger<SessionData> {
    /// Log a create session operation.
    fn log_create_session(&mut self, id: &SessionId, expiry: &SessionExpiry, data: &SessionData);

    /// Log a read session operation.
    fn log_read_session(&self, id: &SessionId);

    /// Log a update session operation.
    fn log_update_session(
        &mut self,
        current_id: &SessionId,
        previous_id: &SessionId,
        expiry: &SessionExpiry,
        data: &SessionData,
    );

    /// Log a delete session operation.
    fn log_delete_session(&mut self, current_id: &SessionId);

    /// Log a clear operation.
    fn log_clear(&mut self);
}

/// A logger that ignores all logging operations.
#[derive(Debug, Copy, Clone)]
pub struct NoLogger;

impl<SessionData> MemoryStoreOperationLogger<SessionData> for NoLogger {
    fn log_create_session(
        &mut self,
        _id: &SessionId,
        _expiry: &SessionExpiry,
        _data: &SessionData,
    ) {
        // do nothing
    }

    fn log_read_session(&self, _id: &SessionId) {
        // do nothing
    }

    fn log_update_session(
        &mut self,
        _current_id: &SessionId,
        _previous_id: &SessionId,
        _expiry: &SessionExpiry,
        _data: &SessionData,
    ) {
        // do nothing
    }

    fn log_delete_session(&mut self, _current_id: &SessionId) {
        // do nothing
    }

    fn log_clear(&mut self) {
        // do nothing
    }
}

/// A logger that stores all logging operations in a `Vec`.
#[derive(Debug)]
pub struct DefaultLogger<SessionData> {
    log: Mutex<Vec<Operation<SessionData>>>,
}

/// An operation of the memory store.
#[derive(Debug, Eq, PartialEq)]
#[allow(missing_docs)]
pub enum Operation<SessionData> {
    CreateSession {
        id: SessionId,
        expiry: SessionExpiry,
        data: SessionData,
    },
    ReadSession {
        id: SessionId,
    },
    UpdateSession {
        current_id: SessionId,
        previous_id: SessionId,
        expiry: SessionExpiry,
        data: SessionData,
    },
    DeleteSession {
        current_id: SessionId,
    },
    Clear,
}

impl<SessionData: Clone> MemoryStoreOperationLogger<SessionData> for DefaultLogger<SessionData> {
    fn log_create_session(&mut self, id: &SessionId, expiry: &SessionExpiry, data: &SessionData) {
        self.log.lock().unwrap().push(Operation::CreateSession {
            id: id.clone(),
            expiry: *expiry,
            data: data.clone(),
        });
    }

    fn log_read_session(&self, id: &SessionId) {
        self.log
            .lock()
            .unwrap()
            .push(Operation::ReadSession { id: id.clone() });
    }

    fn log_update_session(
        &mut self,
        current_id: &SessionId,
        previous_id: &SessionId,
        expiry: &SessionExpiry,
        data: &SessionData,
    ) {
        self.log.lock().unwrap().push(Operation::UpdateSession {
            current_id: current_id.clone(),
            previous_id: previous_id.clone(),
            expiry: *expiry,
            data: data.clone(),
        });
    }

    fn log_delete_session(&mut self, current_id: &SessionId) {
        self.log.lock().unwrap().push(Operation::DeleteSession {
            current_id: current_id.clone(),
        });
    }

    fn log_clear(&mut self) {
        self.log.lock().unwrap().push(Operation::Clear);
    }
}

impl<SessionData> DefaultLogger<SessionData> {
    /// Consume the logger and return the vector of logged operations.
    pub fn into_inner(self) -> Vec<Operation<SessionData>> {
        self.log.into_inner().unwrap()
    }
}

impl<SessionData> Default for DefaultLogger<SessionData> {
    fn default() -> Self {
        Self {
            log: Mutex::new(Default::default()),
        }
    }
}

impl<SessionData, OperationLogger> Clone for MemoryStore<SessionData, OperationLogger> {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
        }
    }
}

impl<SessionData, OperationLogger> From<MemoryStoreData<SessionData, OperationLogger>>
    for MemoryStore<SessionData, OperationLogger>
{
    fn from(store: MemoryStoreData<SessionData, OperationLogger>) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }
}

use crate::session_store::WriteSessionResult;
use crate::{Result, Session, SessionExpiry, SessionId, SessionStoreImplementation};
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
#[derive(Debug, Clone)]
pub struct MemoryStore<Data, OperationLogger> {
    session_map: HashMap<SessionId, Arc<SessionBody<Data>>>,
    operation_logger: OperationLogger,
    maximum_retries_on_id_collision: Option<u32>,
}

#[derive(Debug, Clone)]
struct SessionBody<Data> {
    current_id: SessionId,
    previous_id: Option<SessionId>,
    expiry: SessionExpiry,
    data: Data,
}

#[async_trait]
impl<
        Data: Send + Sync + Clone,
        OperationLogger: Send + Sync + MemoryStoreOperationLogger<Data>,
    > SessionStoreImplementation<Data> for MemoryStore<Data, OperationLogger>
{
    fn maximum_retries_on_id_collision(&self) -> Option<u32> {
        self.maximum_retries_on_id_collision
    }

    async fn create_session(
        &mut self,
        id: &SessionId,
        expiry: &SessionExpiry,
        data: &Data,
    ) -> Result<WriteSessionResult> {
        self.operation_logger.log_create_session(id, expiry, data);

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
        self.operation_logger.log_read_session(id);

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
        self.operation_logger.log_update_session(
            current_id,
            previous_id,
            deletable_id,
            expiry,
            data,
        );

        if self.session_map.contains_key(current_id) {
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
        self.operation_logger
            .log_delete_session(current_id, previous_id);

        self.session_map.remove(current_id);
        if let Some(previous_id) = previous_id.as_ref() {
            self.session_map.remove(previous_id);
        }
        Ok(())
    }

    async fn clear(&mut self) -> Result<()> {
        self.operation_logger.log_clear();
        self.session_map.clear();
        Ok(())
    }
}

impl<Data, OperationLogger> MemoryStore<Data, OperationLogger> {
    /// Sets the maximum retries on id collision, see [SessionStoreImplementation::maximum_retries_on_id_collision] for details.
    pub fn set_maximum_retries_on_id_collision(
        &mut self,
        maximum_retries_on_id_collision: Option<u32>,
    ) {
        self.maximum_retries_on_id_collision = maximum_retries_on_id_collision;
    }

    /// Returns the number of elements in the memory store.
    pub fn len(&self) -> usize {
        self.session_map.len()
    }

    /// Returns true if the memory store is empty.
    pub fn is_empty(&self) -> bool {
        self.session_map.is_empty()
    }

    /// Deletes all expired sessions.
    pub fn delete_expired_sessions(&mut self) -> Result {
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

    /// Consumes the store and returns the logged operations.
    pub fn into_logger(self) -> OperationLogger {
        self.operation_logger
    }
}

impl<Data: Clone, OperationLogger> MemoryStore<Data, OperationLogger> {
    /// Returns an iterator over all sessions in the store.
    pub fn iter(&self) -> impl '_ + Iterator<Item = Session<Data>> {
        self.session_map.iter().map(|(id, body)| {
            Session::new_from_session_store(
                id.clone(),
                body.previous_id.clone(),
                body.expiry,
                body.data.clone(),
            )
        })
    }
}

impl<Data> MemoryStore<Data, NoLogger> {
    /// Create a new empty memory store.
    pub fn new() -> Self {
        Self {
            session_map: Default::default(),
            operation_logger: NoLogger,
            maximum_retries_on_id_collision: None,
        }
    }
}

impl<Data> MemoryStore<Data, DefaultLogger<Data>> {
    /// Create a new empty memory store with the given logger for logging store operations.
    pub fn new_with_logger() -> Self {
        Self {
            session_map: Default::default(),
            operation_logger: Default::default(),
            maximum_retries_on_id_collision: None,
        }
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

impl<Data, OperationLogger: Default> Default for MemoryStore<Data, OperationLogger> {
    fn default() -> Self {
        Self {
            session_map: Default::default(),
            operation_logger: Default::default(),
            maximum_retries_on_id_collision: None,
        }
    }
}

/// A logger for operations conducted by the memory store.
/// This is intended to be used for debug purposes.
pub trait MemoryStoreOperationLogger<Data> {
    /// Log a create session operation.
    fn log_create_session(&mut self, id: &SessionId, expiry: &SessionExpiry, data: &Data);

    /// Log a read session operation.
    fn log_read_session(&self, id: &SessionId);

    /// Log a update session operation.
    fn log_update_session(
        &mut self,
        current_id: &SessionId,
        previous_id: &SessionId,
        deletable_id: &Option<SessionId>,
        expiry: &SessionExpiry,
        data: &Data,
    );

    /// Log a delete session operation.
    fn log_delete_session(&mut self, current_id: &SessionId, previous_id: &Option<SessionId>);

    /// Log a clear operation.
    fn log_clear(&mut self);
}

/// A logger that ignores all logging operations.
#[derive(Debug, Copy, Clone)]
pub struct NoLogger;

impl<Data> MemoryStoreOperationLogger<Data> for NoLogger {
    fn log_create_session(&mut self, _id: &SessionId, _expiry: &SessionExpiry, _data: &Data) {
        // do nothing
    }

    fn log_read_session(&self, _id: &SessionId) {
        // do nothing
    }

    fn log_update_session(
        &mut self,
        _current_id: &SessionId,
        _previous_id: &SessionId,
        _deletable_id: &Option<SessionId>,
        _expiry: &SessionExpiry,
        _data: &Data,
    ) {
        // do nothing
    }

    fn log_delete_session(&mut self, _current_id: &SessionId, _previous_id: &Option<SessionId>) {
        // do nothing
    }

    fn log_clear(&mut self) {
        // do nothing
    }
}

/// A logger that stores all logging operations in a `Vec`.
#[derive(Debug)]
pub struct DefaultLogger<Data> {
    log: Mutex<Vec<Operation<Data>>>,
}

/// An operation of the memory store.
#[derive(Debug, Eq, PartialEq)]
#[allow(missing_docs)]
pub enum Operation<Data> {
    CreateSession {
        id: SessionId,
        expiry: SessionExpiry,
        data: Data,
    },
    ReadSession {
        id: SessionId,
    },
    UpdateSession {
        current_id: SessionId,
        previous_id: SessionId,
        deletable_id: Option<SessionId>,
        expiry: SessionExpiry,
        data: Data,
    },
    DeleteSession {
        current_id: SessionId,
        previous_id: Option<SessionId>,
    },
    Clear,
}

impl<Data: Clone> MemoryStoreOperationLogger<Data> for DefaultLogger<Data> {
    fn log_create_session(&mut self, id: &SessionId, expiry: &SessionExpiry, data: &Data) {
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
        deletable_id: &Option<SessionId>,
        expiry: &SessionExpiry,
        data: &Data,
    ) {
        self.log.lock().unwrap().push(Operation::UpdateSession {
            current_id: current_id.clone(),
            previous_id: previous_id.clone(),
            deletable_id: deletable_id.clone(),
            expiry: *expiry,
            data: data.clone(),
        });
    }

    fn log_delete_session(&mut self, current_id: &SessionId, previous_id: &Option<SessionId>) {
        self.log.lock().unwrap().push(Operation::DeleteSession {
            current_id: current_id.clone(),
            previous_id: previous_id.clone(),
        });
    }

    fn log_clear(&mut self) {
        self.log.lock().unwrap().push(Operation::Clear);
    }
}

impl<Data> DefaultLogger<Data> {
    /// Consume the logger and return the vector of logged operations.
    pub fn into_inner(self) -> Vec<Operation<Data>> {
        self.log.into_inner().unwrap()
    }
}

impl<Data> Default for DefaultLogger<Data> {
    fn default() -> Self {
        Self {
            log: Mutex::new(Default::default()),
        }
    }
}

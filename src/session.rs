use chrono::{DateTime, Duration, Utc};
use rand::{Rng, RngCore};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
};
use rand::distributions::{Alphanumeric, DistString};

/// # The main session type.
///
/// `COOKIE_LENGTH` should be a multiple of 32, which is the block-size of blake3.
///
/// ## Cloning and Serialization
///
/// The `cookie_value` field is not cloned or serialized, and it can
/// only be read through `into_cookie_value`. The intent of this field
/// is that it is set either by initialization or by a session store,
/// and read exactly once in order to set the cookie value.
///
/// ## Change tracking session tracks whether any of its inner data
/// was changed since it was last serialized. Any session store that
/// does not undergo a serialization-deserialization cycle must call
/// [`Session::reset_data_changed`] in order to reset the change tracker on
/// an individual record.
///
/// ### Change tracking example
/// ```rust
/// # use async_session::Session;
/// # fn main() -> async_session::Result { async_std::task::block_on(async {
/// let mut session = Session::new();
/// assert!(!session.data_changed());
///
/// session.insert("key", 1)?;
/// assert!(session.data_changed());
///
/// session.reset_data_changed();
/// assert_eq!(session.get::<usize>("key").unwrap(), 1);
/// assert!(!session.data_changed());
///
/// session.insert("key", 2)?;
/// assert!(session.data_changed());
/// assert_eq!(session.get::<usize>("key").unwrap(), 2);
///
/// session.insert("key", 1)?;
/// assert!(session.data_changed(), "reverting the data still counts as a change");
///
/// session.reset_data_changed();
/// assert!(!session.data_changed());
/// session.remove("nonexistent key");
/// assert!(!session.data_changed());
/// session.remove("key");
/// assert!(session.data_changed());
/// # Ok(()) }) }
/// ```
#[derive(Debug, Serialize, Deserialize)]
pub struct Session<Data, const COOKIE_LENGTH: usize = 64> {
    /// Store the id in binary format for efficient use with a database (that supports binary columns/fields).
    /// We box the id to keep the struct small.
    id: Box<SessionId>,
    expiry: Option<DateTime<Utc>>,
    data: Arc<RwLock<Data>>,

    /// Store the cookie value that was used to generate the id.
    #[serde(skip)]
    cookie_value: Arc<String>,
    /// True if the expiry time has changed or the associated data was borrowed mutably.
    #[serde(skip)]
    data_changed: Arc<AtomicBool>,
    /// Mark the session for destruction.
    #[serde(skip)]
    destroy: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SessionId([u8; blake3::OUT_LEN]);

impl<Data> Clone for Session<Data> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            expiry: self.expiry,
            data: self.data.clone(),
            cookie_value: self.cookie_value.clone(),
            data_changed: self.data_changed.clone(),
            destroy: self.destroy.clone(),
        }
    }
}

/// Generate a random cookie.
fn generate_cookie<const COOKIE_LENGTH: usize>(rng: &mut impl Rng) -> String {
    let mut cookie = String::new();
    Alphanumeric.append_string(rng, &mut cookie, COOKIE_LENGTH);
    cookie
}

impl<Data> Session<Data> {
    /// Create a new session. Generates a random id and matching
    /// cookie value. Does not set an expiry by default
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// # fn main() -> async_session::Result { use rand::thread_rng;
    /// # async_std::task::block_on(async {
    /// let session = Session::new(&mut thread_rng(), ());
    /// assert_eq!(None, session.expiry());
    /// assert!(session.into_cookie_value().is_some());
    /// # Ok(()) }) }
    pub fn new(rng: &mut impl Rng, data: Data) -> Self {
        let cookie_value = generate_cookie(rng);
        let id = SessionId::from_cookie_value(&cookie_value);

        Self {
            id: Box::new(id),
            data_changed: Arc::new(AtomicBool::new(false)),
            expiry: None,
            data: Arc::new(RwLock::new(data)),
            cookie_value: Arc::new(cookie_value),
            destroy: Arc::new(AtomicBool::new(false)),
        }
    }

    /// mark this session for destruction. the actual session record
    /// is not destroyed until the end of this response cycle.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// # fn main() -> async_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new();
    /// assert!(!session.is_destroyed());
    /// session.destroy();
    /// assert!(session.is_destroyed());
    /// # Ok(()) }) }
    pub fn destroy(&mut self) {
        self.destroy.store(true, Ordering::SeqCst);
    }

    /// returns true if this session is marked for destruction
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// # fn main() -> async_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new();
    /// assert!(!session.is_destroyed());
    /// session.destroy();
    /// assert!(session.is_destroyed());
    /// # Ok(()) }) }

    pub fn is_destroyed(&self) -> bool {
        self.destroy.load(Ordering::SeqCst)
    }

    /// Gets the session id
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// # fn main() -> async_session::Result { async_std::task::block_on(async {
    /// let session = Session::new();
    /// let id = session.id().to_owned();
    /// let cookie_value = session.into_cookie_value().unwrap();
    /// assert_eq!(id, Session::id_from_cookie_value(&cookie_value)?);
    /// # Ok(()) }) }
    pub fn id(&self) -> &[u8; 32] {
        &self.id
    }

    /// inserts a serializable value into the session hashmap. returns
    /// an error if the serialization was unsuccessful.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use serde::{Serialize, Deserialize};
    /// # use async_session::Session;
    /// #[derive(Serialize, Deserialize)]
    /// struct User {
    ///     name: String,
    ///     legs: u8
    /// }
    /// let mut session = Session::new();
    /// session.insert("user", User { name: "chashu".into(), legs: 4 }).expect("serializable");
    /// assert_eq!(r#"{"name":"chashu","legs":4}"#, session.get_raw("user").unwrap());
    /// ```
    pub fn insert(&mut self, key: &str, value: impl Serialize) -> Result<(), serde_json::Error> {
        self.insert_raw(key, serde_json::to_string(&value)?);
        Ok(())
    }

    /// inserts a string into the session hashmap
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// let mut session = Session::new();
    /// session.insert_raw("ten", "10".to_string());
    /// let ten: usize = session.get("ten").unwrap();
    /// assert_eq!(ten, 10);
    /// ```
    pub fn insert_raw(&mut self, key: &str, value: String) {
        let mut data = self.data.write().unwrap();
        if data.get(key) != Some(&value) {
            data.insert(key.to_string(), value);
            self.data_changed.store(true, Ordering::SeqCst);
        }
    }

    /// deserializes a type T out of the session hashmap
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// let mut session = Session::new();
    /// session.insert("key", vec![1, 2, 3]);
    /// let numbers: Vec<usize> = session.get("key").unwrap();
    /// assert_eq!(vec![1, 2, 3], numbers);
    /// ```
    pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        let data = self.data.read().unwrap();
        let string = data.get(key)?;
        serde_json::from_str(string).ok()
    }

    /// returns the String value contained in the session hashmap
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// let mut session = Session::new();
    /// session.insert("key", vec![1, 2, 3]);
    /// assert_eq!("[1,2,3]", session.get_raw("key").unwrap());
    /// ```
    pub fn get_raw(&self, key: &str) -> Option<String> {
        let data = self.data.read().unwrap();
        data.get(key).cloned()
    }

    /// removes an entry from the session hashmap
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// let mut session = Session::new();
    /// session.insert("key", "value");
    /// session.remove("key");
    /// assert!(session.get_raw("key").is_none());
    /// assert_eq!(session.len(), 0);
    /// ```
    pub fn remove(&mut self, key: &str) {
        let mut data = self.data.write().unwrap();
        if data.remove(key).is_some() {
            self.data_changed.store(true, Ordering::SeqCst);
        }
    }

    /// returns the number of elements in the session hashmap
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// let mut session = Session::new();
    /// assert_eq!(session.len(), 0);
    /// session.insert("key", 0);
    /// assert_eq!(session.len(), 1);
    /// ```
    pub fn len(&self) -> usize {
        let data = self.data.read().unwrap();
        data.len()
    }

    /// Generates a new id and cookie for this session
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// # fn main() -> async_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new();
    /// let old_id = session.id().to_string();
    /// session.regenerate();
    /// assert!(session.id() != &old_id);
    /// let new_id = session.id().to_string();
    /// let cookie_value = session.into_cookie_value().unwrap();
    /// assert_eq!(new_id, Session::id_from_cookie_value(&cookie_value)?);
    /// # Ok(()) }) }
    /// ```
    pub fn regenerate(&mut self) {
        let cookie_value = generate_cookie(64);
        self.id = Session::id_from_cookie_value(&cookie_value).unwrap();
        self.cookie_value = Some(cookie_value);
    }

    /// sets the cookie value that this session will use to serialize
    /// itself. this should only be called by cookie stores. any other
    /// uses of this method will result in the cookie not getting
    /// correctly deserialized on subsequent requests.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// # fn main() -> async_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new();
    /// session.set_cookie_value("hello".to_owned());
    /// let cookie_value = session.into_cookie_value().unwrap();
    /// assert_eq!(cookie_value, "hello".to_owned());
    /// # Ok(()) }) }
    /// ```
    pub fn set_cookie_value(&mut self, cookie_value: String) {
        self.cookie_value = Some(cookie_value)
    }

    /// returns the expiry timestamp of this session, if there is one
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// # fn main() -> async_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new();
    /// assert_eq!(None, session.expiry());
    /// session.expire_in(std::time::Duration::from_secs(1));
    /// assert!(session.expiry().is_some());
    /// # Ok(()) }) }
    /// ```
    pub fn expiry(&self) -> Option<&DateTime<Utc>> {
        self.expiry.as_ref()
    }

    /// assigns an expiry timestamp to this session
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// # fn main() -> async_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new();
    /// assert_eq!(None, session.expiry());
    /// session.set_expiry(chrono::Utc::now());
    /// assert!(session.expiry().is_some());
    /// # Ok(()) }) }
    /// ```
    pub fn set_expiry(&mut self, expiry: DateTime<Utc>) {
        self.expiry = Some(expiry);
    }

    /// assigns the expiry timestamp to a duration from the current time.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// # fn main() -> async_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new();
    /// assert_eq!(None, session.expiry());
    /// session.expire_in(std::time::Duration::from_secs(1));
    /// assert!(session.expiry().is_some());
    /// # Ok(()) }) }
    /// ```
    pub fn expire_in(&mut self, ttl: std::time::Duration) {
        self.expiry = Some(Utc::now() + Duration::from_std(ttl).unwrap());
    }

    /// predicate function to determine if this session is
    /// expired. returns false if there is no expiry set, or if it is
    /// in the past.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// # use std::time::Duration;
    /// # use async_std::task;
    /// # fn main() -> async_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new();
    /// assert_eq!(None, session.expiry());
    /// assert!(!session.is_expired());
    /// session.expire_in(Duration::from_secs(1));
    /// assert!(!session.is_expired());
    /// task::sleep(Duration::from_secs(2)).await;
    /// assert!(session.is_expired());
    /// # Ok(()) }) }
    /// ```
    pub fn is_expired(&self) -> bool {
        match self.expiry {
            Some(expiry) => expiry < Utc::now(),
            None => false,
        }
    }

    /// Ensures that this session is not expired. Returns None if it is expired
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// # use std::time::Duration;
    /// # use async_std::task;
    /// # fn main() -> async_session::Result { async_std::task::block_on(async {
    /// let session = Session::new();
    /// let mut session = session.validate().unwrap();
    /// session.expire_in(Duration::from_secs(1));
    /// let session = session.validate().unwrap();
    /// task::sleep(Duration::from_secs(2)).await;
    /// assert_eq!(None, session.validate());
    /// # Ok(()) }) }
    /// ```
    pub fn validate(self) -> Option<Self> {
        if self.is_expired() {
            None
        } else {
            Some(self)
        }
    }

    /// Checks if the data has been modified. This is based on the
    /// implementation of [`PartialEq`] for the inner data type.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// # fn main() -> async_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new();
    /// assert!(!session.data_changed(), "new session is not changed");
    /// session.insert("key", 1);
    /// assert!(session.data_changed());
    ///
    /// session.reset_data_changed();
    /// assert!(!session.data_changed());
    /// session.remove("key");
    /// assert!(session.data_changed());
    /// # Ok(()) }) }
    /// ```
    pub fn data_changed(&self) -> bool {
        self.data_changed.load(Ordering::SeqCst)
    }

    /// Resets `data_changed` dirty tracking. This is unnecessary for
    /// any session store that serializes the data to a string on
    /// storage.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// # fn main() -> async_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new();
    /// assert!(!session.data_changed(), "new session is not changed");
    /// session.insert("key", 1);
    /// assert!(session.data_changed());
    ///
    /// session.reset_data_changed();
    /// assert!(!session.data_changed());
    /// session.remove("key");
    /// assert!(session.data_changed());
    /// # Ok(()) }) }
    /// ```
    pub fn reset_data_changed(&self) {
        self.data_changed.store(false, Ordering::SeqCst);
    }

    /// Ensures that this session is not expired. Returns None if it is expired
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// # use std::time::Duration;
    /// # use async_std::task;
    /// # fn main() -> async_session::Result { async_std::task::block_on(async {
    /// let mut session = Session::new();
    /// session.expire_in(Duration::from_secs(123));
    /// let expires_in = session.expires_in().unwrap();
    /// assert!(123 - expires_in.as_secs() < 2);
    /// # Ok(()) }) }
    /// ```
    /// Duration from now to the expiry time of this session
    pub fn expires_in(&self) -> Option<std::time::Duration> {
        self.expiry?.signed_duration_since(Utc::now()).to_std().ok()
    }

    /// takes the cookie value and consume this session.
    /// this is generally only performed by the session store
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// # fn main() -> async_session::Result { use rand::thread_rng;
    /// # async_std::task::block_on(async {
    /// let session = Session::new(&mut thread_rng(), ());
    /// session.set_cookie_value("hello".to_owned());
    /// let cookie_value = session.into_cookie_value().unwrap();
    /// assert_eq!(cookie_value, "hello".to_owned());
    /// # Ok(()) }) }
    /// ```
    pub fn into_cookie_value(mut self) -> Option<String> {
        self.cookie_value.take()
    }
}

impl SessionId {
    /// applies a cryptographic hash function on a cookie value
    /// returned by [`Session::into_cookie_value`] to obtain the
    /// session id for that cookie. Returns an error if the cookie
    /// format is not recognized
    ///
    /// # Example
    ///
    /// ```rust
    /// # use async_session::Session;
    /// # fn main() -> async_session::Result { use rand::thread_rng;
    /// # async_std::task::block_on(async {
    /// let session = Session::new(&mut thread_rng(), ());
    /// let id = session.id().to_string();
    /// let cookie_value = session.into_cookie_value().unwrap();
    /// assert_eq!(id, Session::id_from_cookie_value(&cookie_value)?);
    /// # Ok(()) }) }
    /// ```
    pub fn from_cookie_value(cookie_value: &str) -> Self {
        // The original code used base64 encoded binary ids of length of a multiple of the blake3 block size.
        // We do the same but with alphanumerical ids with a length multiple of the blake3 block size.
        let hash = blake3::hash(cookie_value.as_bytes());
        Self(hash.into())
    }
}
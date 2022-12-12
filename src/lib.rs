//! Async HTTP sessions.
//!
//! This crate provides a generic interface between cookie values and
//! storage backends to create a concept of sessions. It provides an
//! interface that can be used to encode and store sessions, and
//! decode and load sessions generating cookies in the process.
//!
//! # Change tracking
//!
//! Changes are tracked automatically.
//! Whenever the data or expiry of a session is accessed mutably, the session is marked as changed.
//! The session store only forwards updates to its implementation when a change has happened.
//! Further, the session store is responsible for deciding whether the session cookie should be
//! renewed, hence its functions return an `Option<`[`SessionCookieCommand`]`>` where applicable.
//!
//! # Security
//!
//! TODO
//!
//! # Example
//!
//! ```
//! use typed_session::{Session, SessionStore, MemoryStore};
//!
//! # fn main() -> typed_session::Result {
//! # use rand::thread_rng;
//! # use typed_session::SessionCookieCommand;
//! # async_std::task::block_on(async {
//! #
//! // Make sure to use a cryptographically secure random generator.
//! // According to the docs of the rand crate, thread_rng() is secure.
//! let mut rng = thread_rng();
//!
//! // Init a new session store we can persist sessions to.
//! let mut store: SessionStore<_, _> = SessionStore::new(MemoryStore::new());
//!
//! // Create and store a new session.
//! // The session can hold arbitrary data, but session stores are type safe,
//! // i.e. all sessions must hold data of the same type.
//! // Use e.g. an enum to distinguish session states like "anonymous" or "logged-in as user X".
//! let session = Session::new_with_data(15);
//! let SessionCookieCommand::Set { cookie_value, ..} = store.store_session(session, &mut rng).await? else {unreachable!("New sessions without expiry always set the cookie")};
//! // The set_cookie_command contains the cookie value and the expiry to be sent to the client.
//!
//! // Retrieve the session using the cookie.
//! let session = store.load_session(cookie_value).await?.unwrap();
//! assert_eq!(*session.data(), 15);
//! #
//! # Ok(()) }) }
//! ```

// #![forbid(unsafe_code, future_incompatible)]
// #![deny(missing_debug_implementations, nonstandard_style)]
// #![warn(missing_docs, missing_doc_code_examples, unreachable_pub)]
#![forbid(unsafe_code)]
#![deny(
    future_incompatible,
    missing_debug_implementations,
    nonstandard_style,
    missing_docs,
    unreachable_pub,
    missing_copy_implementations,
    unused_qualifications
)]

pub use anyhow::Error;
/// An anyhow::Result with default return type of ()
pub type Result<T = ()> = std::result::Result<T, Error>;

//mod cookie_store;
mod memory_store;
mod session;
mod session_store;

//pub use cookie_store::CookieStore;
pub use memory_store::MemoryStore;
pub use session::{Session, SessionId, SessionIdType};
pub use session_store::{SessionCookieCommand, SessionStore, SessionStoreImplementation};

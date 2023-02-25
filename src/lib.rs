//! # Async strongly-typed HTTP sessions.
//!
//! This crate provides a session handling mechanism with abstract session store (typically a database).
//! The crate is not meant to be used directly, but should be wrapped into middleware for your
//! web-framework of choice.
//!
//! This crate handles all the typical plumbing associated with session handling, including:
//!  * change tracking
//!  * expiry and automatic renewal
//!  * generation of session ids
//!
//! On the "front-end" of this crate, the [`SessionStore`](SessionStore) provides a simple interface
//! to load and store sessions given an identifying string, typically the value of a cookie.
//! The [`Session`](Session) type has a type parameter `SessionData` that decides what session-specific
//! data is stored in the database.
//!
//! On the "back-end" of this crate, the trait [`SessionStoreConnector`](SessionStoreConnector)
//! provides a simple [*CRUD*](https://en.wikipedia.org/wiki/Create,_read,_update_and_delete)-based
//! interface for handling sessions in a database.
//!
//! ## Change tracking
//!
//! Changes are tracked automatically.
//! Whenever the data or expiry of a session is accessed mutably, the session is marked as changed.
//! The session store only forwards updates to its implementation when a change has happened.
//! Further, the session store is responsible for deciding whether the session cookie should be
//! renewed, hence its functions return a [`SessionCookieCommand`] where applicable.
//! This command should be executed by the web framework, adding a `Set-Cookie` header to the HTTP
//! response as required.
//!
//! As a small optimisation, sessions that contain "default" data are never stored in the session store
//! or communicated to the client, unless their data or expiry is accessed mutably.
//!
//! ## Session expiry
//!
//! Session expiry is only checked when the session is loaded from the store. If it is expired, the
//! store ignores it and returns no session.
//! The session's expiry can be updated manually, or automatically with a [`SessionRenewalStrategy`].
//! In case the session is renewed automatically, the session may be updated by the session store,
//! even if neither its data nor expiry was accessed mutably.
//!
//! Note that **expired sessions are not deleted** from the session store. This is left to a background
//! job that needs to be set up independently of this crate. Also, expired cookies are not deleted,
//! it is left to the browser to take care of that.
//!
//! ## Manual session removal
//!
//! While expiry does not actually delete sessions, but leaves this up to external jobs, sessions can
//! be deleted from both the session store and the client using the [`Session::delete`] function.
//! This marks the session for deletion, such that it is deleted from the store when [`SessionStore::store_session`]
//! is called. The return value of `store_session` is then [`SessionCookieCommand::Delete`],
//! indicating to the web framework to set the `Set-Cookie` header such that the cookie is deleted.
//!
//! ## Security
//!
//! Sessions are identified by an alphanumeric string including upper and lower case letters from the
//! English alphabet. This gives `log_2(26+26+10) ≥ 5.95` bits of entropy per character. The default
//! cookie length is `64`, resulting in `log_2(26+26+10) * 64 ≥ 381` bits of entropy. If a cookie
//! length of `32` is used, then the cookie contains `log_2(26+26+10) * 32 ≥ 190` bits of entropy.
//! [The OWASP® Foundation](https://owasp.org) recommends session ids to have at least
//! [`128` bits of entropy](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html#session-id-entropy).
//! That is, `64` bits of actual entropy, where a good PRNG is assumed to produce `0.5` bits of entropy per bit.
//! The random source used by default is [`rand::rngs::ThreadRng`], version 0.8.5, which is _secure_.
//!
//! Session data is stored only in the session store, the client only stores the unhashed session id.
//! The session store only stores the hashed session id.
//!
//! Note that the OWASP® Foundation does not require session ids to be hashed. We anyways use the
//! fast and secure hash function provided by crate [blake3] for additional security in case the
//! session store gets compromised.
//!
//! ## Example
//!
//! ```
//! use typed_session::{Session, SessionStore, MemoryStore};
//!
//! # fn main() -> typed_session::Result {
//! # use rand::thread_rng;
//! # use typed_session::{SessionCookieCommand, SessionRenewalStrategy};
//! # async_std::task::block_on(async {
//! #
//! // Init a new session store we can persist sessions to.
//! let mut store: SessionStore<_, _> = SessionStore::new(MemoryStore::new(), SessionRenewalStrategy::Ignore);
//!
//! // Create and store a new session.
//! // The session can hold arbitrary data, but session stores are type safe,
//! // i.e. all sessions must hold data of the same type.
//! // Use e.g. an enum to distinguish session states like "anonymous" or "logged-in as user X".
//! let session = Session::new_with_data(15);
//! let SessionCookieCommand::Set { cookie_value, .. } = store.store_session(session).await? else {unreachable!("New sessions without expiry always set the cookie")};
//! // The set_cookie_command contains the cookie value and the expiry to be sent to the client.
//!
//! // Retrieve the session using the cookie.
//! let session = store.load_session(cookie_value).await?.unwrap();
//! assert_eq!(*session.data(), 15);
//! #
//! # Ok(()) }) }
//! ```
//!
//! ## Comparison with crate [async-session](https://crates.io/crates/async-session)
//!
//! This crate was designed after `async-session`. The main difference is that the session data is
//! generic, such that the user can plug in any type they want. If needed, the same `HashMap<String, String>`
//! can be used as session data type.

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

mod memory_store;
mod session;
mod session_store;

pub use memory_store::{
    DefaultLogger, MemoryStore, MemoryStoreOperationLogger, NoLogger, Operation,
};
pub use session::{Session, SessionExpiry, SessionId, SessionIdType};
pub use session_store::{
    cookie_generator::{
        DebugSessionCookieGenerator, DefaultSessionCookieGenerator, SessionCookieGenerator,
    },
    SessionCookieCommand, SessionRenewalStrategy, SessionStore, SessionStoreConnector,
};

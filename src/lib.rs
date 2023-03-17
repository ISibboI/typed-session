//! # Async strongly-typed HTTP sessions.
//!
//! This crate provides a session handling mechanism with abstract session store (typically a database).
//! The crate is not meant to be used directly, but should be wrapped into middleware for your
//! web-framework of choice.
//!
//! This crate handles all the typical plumbing associated with session handling, including:
//!  * change tracking,
//!  * expiry and automatic renewal and
//!  * generation of session ids.
//!
//! On the "front-end" of this crate, the [`SessionStore`] provides a simple interface
//! to load and store sessions given an identifying string, typically the value of a cookie.
//! The [`Session`] type has a type parameter `SessionData` that decides what session-specific
//! data is stored in the database.
//! The user on the front-end is responsible for communicating session cookies to the client by performing the [`SessionCookieCommand`] returned by [`SessionStore::store_session`].
//!
//! On the "back-end" of this crate, the trait [`SessionStoreConnector`]
//! expects a simple [*CRUD*](https://en.wikipedia.org/wiki/Create,_read,_update_and_delete)-based
//! interface for handling sessions in a database.
//!
//! ## Change tracking
//!
//! Changes are tracked automatically in an efficient way.
//! If a client has no session or an invalid session, a new session is created for that client.
//! However, only if the session contains meaningful data it is stored and communicated to the client.
//! Session data is assumed to be meaningful when it has been accessed mutably or the session was explicitly constructed with non-default data.
//! Mutably accessing or mutating the expiry is not considered enough for the session to actually be stored.
//!
//! Once the session is stored, if either the data or expiry is accessed mutably by a future request, it is updated.
//! Each update generates a new session id to prevent simultaneous updates of the same session from producing unexpected results.
//! If the session is not updated, then we neither touch the session store, nor do we communicate any session cookie to the client.
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
//! cookie length is `32`, resulting in `log_2(26+26+10) * 32 ≥ 190` bits of entropy.
//! [The OWASP® Foundation](https://owasp.org) recommends session ids to have at least
//! [`128` bits of entropy](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html#session-id-entropy).
//! That is, `64` bits of actual entropy, where a good PRNG is assumed to produce `0.5` bits of entropy per bit.
//! The random source used by default is [`rand::rngs::ThreadRng`], from the [rand] crate, which is secure.
//!
//! Session data is stored only in the session store along with a hashed session id, while the client
//! only stores the unhashed session id.
//!
//! Note that the OWASP® Foundation does not require session ids to be hashed. We anyways use the
//! fast and secure hash function provided by crate [blake3] for additional security in case the
//! session store gets compromised.
//!
//! This crate updates the session id whenever the session data has changed or the session is expired.
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
//! ## Debugging
//!
//! To aid in debugging, this crate offers a debug backend implementation called [`MemoryStore`]
//! under the feature flag `memory-store`.
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
/// An [`anyhow::Result`] with default return type of `()`.
pub type Result<T = ()> = anyhow::Result<T>;

#[cfg(feature = "memory-store")]
mod memory_store;
mod session;
mod session_store;

#[cfg(feature = "memory-store")]
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

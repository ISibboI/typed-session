# Async Typed Session Management in Rust

[![Version](https://img.shields.io/crates/v/typed-session.svg?style=flat-square)](https://crates.io/crates/typed-session)
[![Downloads](https://img.shields.io/crates/d/typed-session.svg?style=flat-square)](https://crates.io/crates/typed-session)
[![Docs](https://img.shields.io/badge/docs-latest-blue.svg?style=flat-square)](https://docs.rs/typed-session)

API documentation: [docs.rs](https://docs.rs/typed-session)

Use typed-session to outsource all the low-level details of session management, such as session **expiration** and automatic **renewal** as well as **change tracking** of session data.
Typed-session was designed to live up to the [OWASP® Foundation's](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html) session **security** standards, with **efficiency** and **usability** in mind.
With typed-session, you can take full advantage of Rust's type system to model your users' sessions.

## Compatibility

Typed session acts as a middleware in a web framework, injecting session information into HTTP requests as required, and storing sessions in a database.

Currently, the following **session stores** are available:

 * `MemoryStore`, a debug session store available under the feature flag `memory-store`.

Currently, typed-session is integrated into the following **web frameworks**:

 * none so far

Typed-session has no dependency to any specific async runtime, and hence can be used with any.

## Security

We have designed and implemented the crate with security in mind.
Our design fulfils the requirements stated in [The OWASP® Foundation](https://owasp.org)'s cheat sheet on [session management](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html).
We additionally hash the session ids using the fast and secure hash function [blake3](https://en.wikipedia.org/wiki/BLAKE_(hash_function)#BLAKE3) before storing them.
To mitigate exploitable bugs we use ``#![deny(unsafe_code)]`` to ensure everything is implemented in 100% safe Rust.

For further details, refer to the [crate-level documentation](https://docs.rs/typed-session).

So far, this crate has not been reviewed for security.
If you have the necessary skills and wish to contribute to an open source project, please get in touch.

## Contributing

Want to join us? Check out our ["Contributing" guide][contributing] and take a
look at some of these issues:

- [Issues labeled "good first issue"][good-first-issue]
- [Issues labeled "help wanted"][help-wanted]

Any contribution you intentionally submit for inclusion in the work shall be licensed under the [BSD-2-Clause](https://opensource.org/license/bsd-2-clause/) license.

[contributing]: https://github.com/http-rs/typed-session/blob/main/.github/CONTRIBUTING.md
[good-first-issue]: https://github.com/http-rs/typed-session/labels/good%20first%20issue
[help-wanted]: https://github.com/http-rs/typed-session/labels/help%20wanted

## Acknowledgements

This work is based on the crate [async-session](https://crates.io/crate/async-session) by 
[Yoshua Wuyts](https://github.com/yoshuawuyts) and
[Jacob Rothstein](https://github.com/jbr).

## License

This crate is licensed under the [BSD-2-Clause](https://opensource.org/license/bsd-2-clause/) license.

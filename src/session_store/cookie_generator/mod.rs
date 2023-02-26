use rand::distributions::{Alphanumeric, DistString};
use std::fmt::Write;
use std::sync::Mutex;
use tracing::warn;

/// A type with the ability to generate cookies.
/// If the generator needs some mutable state, then it has to be behind a mutex or similar.
/// This is to allow handling concurrent requests with a single session store instance.
pub trait SessionCookieGenerator<const COOKIE_LENGTH: usize> {
    /// Generate a cookie, i.e. a string that is a valid HTTP cookie value.
    fn generate_cookie(&self) -> String;
}

/// The default cookie generator with focus on security.
/// It uses [`ThreadRng`](rand::rngs::ThreadRng) as a random source and the [`Alphanumeric`] distribution
/// to generate cookie strings. This gives `log_2(26+26+10) ≥ 5.95` bits of entropy per character.
#[derive(Debug, Default, Clone)]
pub struct DefaultSessionCookieGenerator<const COOKIE_LENGTH: usize = 32>;

impl<const COOKIE_LENGTH: usize> SessionCookieGenerator<COOKIE_LENGTH>
    for DefaultSessionCookieGenerator<COOKIE_LENGTH>
{
    fn generate_cookie(&self) -> String {
        let mut cookie = String::new();
        Alphanumeric.append_string(&mut rand::thread_rng(), &mut cookie, COOKIE_LENGTH);
        cookie
    }
}

/// A debug cookie generator that generates an ascending sequence of integers, formatted as strings padded with zeroes.
#[derive(Debug, Default)]
pub struct DebugSessionCookieGenerator<const COOKIE_LENGTH: usize> {
    next_index: Mutex<usize>,
}

impl<const COOKIE_LENGTH: usize> SessionCookieGenerator<COOKIE_LENGTH>
    for DebugSessionCookieGenerator<COOKIE_LENGTH>
{
    fn generate_cookie(&self) -> String {
        warn!("Using debug session cookie generator. This is not secure.");
        let mut cookie = String::new();
        write!(
            &mut cookie,
            "{:0width$}",
            self.next_index.lock().unwrap(),
            width = COOKIE_LENGTH
        )
        .unwrap();
        assert_eq!(cookie.len(), COOKIE_LENGTH);
        *self.next_index.lock().unwrap() += 1;
        cookie
    }
}

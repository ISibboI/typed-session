use rand::distributions::{Alphanumeric, DistString};
use rand::rngs::ThreadRng;
use std::fmt::Write;

/// A type with the ability to generate cookies.
pub trait SessionCookieGenerator<const COOKIE_LENGTH: usize> {
    /// Generate a cookie, i.e. a string that is a valid HTTP cookie value.
    fn generate_cookie(&mut self) -> String;
}

/// The default cookie generator with focus on security.
/// It uses [rand::ThreadRng] as a random source and the [Alphanumeric] distribution to generate cookie strings.
/// This gives `log_2(26+26+10) ≥ 5.95` bits of entropy per character.
#[derive(Debug, Default)]
pub struct DefaultSessionCookieGenerator<const COOKIE_LENGTH: usize = 64> {
    rng: ThreadRng,
}

impl<const COOKIE_LENGTH: usize> SessionCookieGenerator<COOKIE_LENGTH>
    for DefaultSessionCookieGenerator<COOKIE_LENGTH>
{
    fn generate_cookie(&mut self) -> String {
        let mut cookie = String::new();
        Alphanumeric.append_string(&mut self.rng, &mut cookie, COOKIE_LENGTH);
        cookie
    }
}

/// A debug cookie generator that generates an ascending sequence of integers, formatted as strings padded with zeroes.
#[derive(Debug, Default)]
pub struct DebugSessionCookieGenerator<const COOKIE_LENGTH: usize> {
    next_index: usize,
}

impl<const COOKIE_LENGTH: usize> SessionCookieGenerator<COOKIE_LENGTH>
    for DebugSessionCookieGenerator<COOKIE_LENGTH>
{
    fn generate_cookie(&mut self) -> String {
        let mut cookie = String::new();
        write!(&mut cookie, "{:0width$}", self.next_index, width = COOKIE_LENGTH).unwrap();
        assert_eq!(cookie.len(), COOKIE_LENGTH);
        self.next_index += 1;
        cookie
    }
}

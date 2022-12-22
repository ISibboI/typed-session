use std::collections::BTreeSet;
use typed_session::{
    DebugSessionCookieGenerator, MemoryStore, Operation, Session, SessionCookieCommand,
    SessionCookieGenerator, SessionExpiry, SessionId, SessionRenewalStrategy, SessionStore,
};

/// If a new session is created but never mutated, then there is no need to store it on the client side or session store.
#[async_std::test]
async fn test_dont_store_default_session() {
    let mut store: SessionStore<(), _, 64, _> = SessionStore::new_with_cookie_generator(
        MemoryStore::new_with_logger(),
        DebugSessionCookieGenerator::default(),
        SessionRenewalStrategy::Ignore,
    );
    let session = Session::new();
    matches!(
        store.store_session(session).await.unwrap(),
        SessionCookieCommand::DoNothing
    );
    assert_eq!(
        store.into_inner().into_logger().into_inner().as_slice(),
        &[]
    );
}

/// If a new session is created and mutated, then store it on the client side and in the session store.
#[async_std::test]
async fn test_store_updated_default_session() {
    let mut store: SessionStore<(), _, 64, _> = SessionStore::new_with_cookie_generator(
        MemoryStore::new_with_logger(),
        DebugSessionCookieGenerator::default(),
        SessionRenewalStrategy::Ignore,
    );
    let mut session = Session::new();
    *session.data_mut() = ();
    matches!(
        store.store_session(session).await.unwrap(),
        SessionCookieCommand::Set {
            expiry: SessionExpiry::Never,
            ..
        }
    );
    let memory_store = store.into_inner();
    assert_eq!(
        memory_store
            .iter()
            .map(Session::into_data_expiry_pair)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([(Some(()), Some(SessionExpiry::Never))])
    );
    let mut cookie_generator = DebugSessionCookieGenerator::<64>::default();
    let cookie_0 = cookie_generator.generate_cookie();
    assert_eq!(
        memory_store.into_logger().into_inner().as_slice(),
        &[Operation::CreateSession {
            id: SessionId::from_cookie_value(&cookie_0),
            expiry: SessionExpiry::Never,
            data: (),
        }]
    );
}

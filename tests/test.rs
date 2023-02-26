use std::collections::BTreeSet;
use typed_session::{
    DebugSessionCookieGenerator, MemoryStore, Operation, Session, SessionCookieCommand,
    SessionCookieGenerator, SessionExpiry, SessionId, SessionRenewalStrategy, SessionStore,
};

/// If a new session is created but never mutated, then no cookie is set and the session is not stored in the session store.
#[async_std::test]
async fn test_dont_store_default_session() {
    let store: SessionStore<(), _, 32, _> = SessionStore::new_with_cookie_generator(
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

/// If a new session is created and mutated, then a cookie is set and the session is stored in the session store.
#[async_std::test]
async fn test_store_updated_default_session() {
    let cookie_generator = DebugSessionCookieGenerator::<32>::default();
    let cookie_0 = cookie_generator.generate_cookie();

    let store: SessionStore<i32, _, 32, _> = SessionStore::new_with_cookie_generator(
        MemoryStore::new_with_logger(),
        DebugSessionCookieGenerator::default(),
        SessionRenewalStrategy::Ignore,
    );
    let mut session = Session::new();
    *session.data_mut() = 1;
    let SessionCookieCommand::Set {
        expiry: SessionExpiry::Never,
        cookie_value,
    } = store.store_session(session).await.unwrap() else {panic! ()};
    assert_eq!(cookie_value, cookie_0);
    let memory_store = store.into_inner();
    // Also check if memory store works correctly here. That is not the main thing we test, but why not.
    let mut data_expiry_pairs = BTreeSet::new();
    memory_store.for_each(|session| {
        data_expiry_pairs.insert(session.into_data_expiry_pair());
    });
    assert_eq!(
        data_expiry_pairs,
        BTreeSet::from([(Some(1), Some(SessionExpiry::Never))])
    );
    assert_eq!(
        memory_store.into_logger().into_inner().as_slice(),
        &[Operation::CreateSession {
            id: SessionId::from_cookie_value(&cookie_0),
            expiry: SessionExpiry::Never,
            data: 1,
        }]
    );
}

/// If a session is loaded from the store and stored without change, then the cookie is not updated and the session is not updated in the session store.
#[async_std::test]
async fn test_dont_update_unchanged_session() {
    let cookie_generator = DebugSessionCookieGenerator::<32>::default();
    let cookie_0 = cookie_generator.generate_cookie();

    let store: SessionStore<i32, _, 32, _> = SessionStore::new_with_cookie_generator(
        MemoryStore::new_with_logger(),
        DebugSessionCookieGenerator::default(),
        SessionRenewalStrategy::Ignore,
    );
    let mut session = Session::new();
    *session.data_mut() = 1;
    let SessionCookieCommand::Set {cookie_value, expiry: SessionExpiry::Never} = store.store_session(session).await.unwrap() else {panic!()};
    assert_eq!(cookie_value, cookie_0);
    let session = store.load_session(cookie_value).await.unwrap().unwrap();
    assert_eq!(
        store.store_session(session).await.unwrap(),
        SessionCookieCommand::DoNothing
    );
    assert_eq!(
        store.into_inner().into_logger().into_inner().as_slice(),
        &[
            Operation::CreateSession {
                id: SessionId::from_cookie_value(&cookie_0),
                expiry: SessionExpiry::Never,
                data: 1,
            },
            Operation::ReadSession {
                id: SessionId::from_cookie_value(&cookie_0)
            }
        ]
    );
}

/// If a session is loaded from the store and stored with change, then the cookie is updated and the session is updated in the session store.
#[async_std::test]
async fn test_update_changed_session() {
    let cookie_generator = DebugSessionCookieGenerator::<32>::default();
    let cookie_0 = cookie_generator.generate_cookie();
    let cookie_1 = cookie_generator.generate_cookie();
    let store: SessionStore<i32, _, 32, _> = SessionStore::new_with_cookie_generator(
        MemoryStore::new_with_logger(),
        DebugSessionCookieGenerator::default(),
        SessionRenewalStrategy::Ignore,
    );
    let mut session = Session::new();
    *session.data_mut() = 1;
    let SessionCookieCommand::Set {cookie_value, expiry: SessionExpiry::Never} = store.store_session(session).await.unwrap() else {panic!()};
    assert_eq!(cookie_value, cookie_0);
    let mut session = store.load_session(cookie_value).await.unwrap().unwrap();
    assert_eq!(*session.data(), 1);
    *session.data_mut() = 2;
    assert_eq!(
        store.store_session(session).await.unwrap(),
        SessionCookieCommand::Set {
            cookie_value: cookie_1.clone(),
            expiry: SessionExpiry::Never
        }
    );
    assert_eq!(
        store.into_inner().into_logger().into_inner().as_slice(),
        &[
            Operation::CreateSession {
                id: SessionId::from_cookie_value(&cookie_0),
                expiry: SessionExpiry::Never,
                data: 1,
            },
            Operation::ReadSession {
                id: SessionId::from_cookie_value(&cookie_0)
            },
            Operation::UpdateSession {
                current_id: SessionId::from_cookie_value(&cookie_1),
                previous_id: SessionId::from_cookie_value(&cookie_0),
                deletable_id: None,
                expiry: SessionExpiry::Never,
                data: 2,
            }
        ]
    );
}

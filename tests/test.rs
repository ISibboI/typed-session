use chrono::{Duration, Utc};
use std::collections::BTreeSet;
use typed_session::{
    DebugSessionCookieGenerator, Error, MemoryStore, Operation, Session, SessionCookieCommand,
    SessionCookieGenerator, SessionExpiry, SessionId, SessionRenewalStrategy, SessionStore,
};

/// If a new session is created but never mutated, then no cookie is set and the session is not stored in the session store.
#[async_std::test]
async fn test_dont_store_default_session() {
    let store: SessionStore<(), _, _> = SessionStore::new_with_cookie_generator(
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

/// If a new session is created but only its expiry is mutated and not its data, then no cookie is set and the session is not stored in the session store.
#[async_std::test]
async fn test_dont_store_default_session_with_expiry_change() {
    let store: SessionStore<(), _, _> = SessionStore::new_with_cookie_generator(
        MemoryStore::new_with_logger(),
        DebugSessionCookieGenerator::default(),
        SessionRenewalStrategy::Ignore,
    );
    let mut session = Session::new();
    session.set_expiry(Utc::now() + Duration::days(1));

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
    let cookie_generator = DebugSessionCookieGenerator::default();
    let cookie_0 = cookie_generator.generate_cookie();

    let store: SessionStore<i32, _, _> = SessionStore::new_with_cookie_generator(
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
    let cookie_generator = DebugSessionCookieGenerator::default();
    let cookie_0 = cookie_generator.generate_cookie();

    let store: SessionStore<i32, _, _> = SessionStore::new_with_cookie_generator(
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
    let cookie_generator = DebugSessionCookieGenerator::default();
    let cookie_0 = cookie_generator.generate_cookie();
    let cookie_1 = cookie_generator.generate_cookie();
    let store: SessionStore<i32, _, _> = SessionStore::new_with_cookie_generator(
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
                expiry: SessionExpiry::Never,
                data: 2,
            }
        ]
    );
}

/// If a session is deleted, then the cookie is deleted and the session is deleted from the session store.
#[async_std::test]
async fn test_delete_deleted_session() {
    let cookie_generator = DebugSessionCookieGenerator::default();
    let cookie_0 = cookie_generator.generate_cookie();
    let store: SessionStore<i32, _, _> = SessionStore::new_with_cookie_generator(
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
    session.delete();
    assert_eq!(
        store.store_session(session).await.unwrap(),
        SessionCookieCommand::Delete,
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
            Operation::DeleteSession {
                current_id: SessionId::from_cookie_value(&cookie_0),
            }
        ]
    );
}

/// If a session is changed, the old session id becomes invalid.
#[async_std::test]
async fn test_prevent_using_old_session_id() {
    let cookie_generator = DebugSessionCookieGenerator::default();
    let cookie_0 = cookie_generator.generate_cookie();
    let cookie_1 = cookie_generator.generate_cookie();
    // true represents being logged in
    let store: SessionStore<bool, _, _> = SessionStore::new_with_cookie_generator(
        MemoryStore::new_with_logger(),
        DebugSessionCookieGenerator::default(),
        SessionRenewalStrategy::Ignore,
    );
    let mut session = Session::new();
    *session.data_mut() = false;
    let SessionCookieCommand::Set {cookie_value, expiry: SessionExpiry::Never} = store.store_session(session).await.unwrap() else {panic!()};
    assert_eq!(cookie_value, cookie_0);
    let mut session = store.load_session(cookie_value).await.unwrap().unwrap();
    assert!(!*session.data());
    *session.data_mut() = true;
    assert_eq!(
        store.store_session(session).await.unwrap(),
        SessionCookieCommand::Set {
            cookie_value: cookie_1.clone(),
            expiry: SessionExpiry::Never
        }
    );

    // Check if we can upgrade the old session.
    assert!(store.load_session(&cookie_0).await.unwrap().is_none());

    assert_eq!(
        store.into_inner().into_logger().into_inner().as_slice(),
        &[
            Operation::CreateSession {
                id: SessionId::from_cookie_value(&cookie_0),
                expiry: SessionExpiry::Never,
                data: false,
            },
            Operation::ReadSession {
                id: SessionId::from_cookie_value(&cookie_0)
            },
            Operation::UpdateSession {
                current_id: SessionId::from_cookie_value(&cookie_1),
                previous_id: SessionId::from_cookie_value(&cookie_0),
                expiry: SessionExpiry::Never,
                data: true,
            },
            Operation::ReadSession {
                id: SessionId::from_cookie_value(&cookie_0)
            },
        ]
    );
}

/// If a session is changed concurrently, then only the first modification is successful.
#[async_std::test]
async fn test_concurrent_modification() {
    let cookie_generator = DebugSessionCookieGenerator::default();
    let cookie_0 = cookie_generator.generate_cookie();
    let cookie_1 = cookie_generator.generate_cookie();
    let cookie_2 = cookie_generator.generate_cookie();
    let store: SessionStore<i32, _, _> = SessionStore::new_with_cookie_generator(
        MemoryStore::new_with_logger(),
        DebugSessionCookieGenerator::default(),
        SessionRenewalStrategy::Ignore,
    );
    let mut session = Session::new();
    *session.data_mut() = 1;
    let SessionCookieCommand::Set {cookie_value, expiry: SessionExpiry::Never} = store.store_session(session).await.unwrap() else {panic!()};
    assert_eq!(cookie_value, cookie_0);
    let mut session1 = store.load_session(&cookie_value).await.unwrap().unwrap();
    let mut session2 = store.load_session(&cookie_value).await.unwrap().unwrap();
    assert_eq!(*session1.data(), 1);
    assert_eq!(*session2.data(), 1);
    *session1.data_mut() = 2;
    *session2.data_mut() = 3;
    assert_eq!(
        store.store_session(session1).await.unwrap(),
        SessionCookieCommand::Set {
            cookie_value: cookie_1.clone(),
            expiry: SessionExpiry::Never
        }
    );
    assert!(matches!(
        store.store_session(session2).await,
        Err(Error::UpdatedSessionDoesNotExist)
    ));

    // Check if we can upgrade the old session.
    assert!(store.load_session(&cookie_0).await.unwrap().is_none());

    let actual = store.into_inner().into_logger().into_inner();
    let actual = actual.as_slice();
    let expected = &[
        Operation::CreateSession {
            id: SessionId::from_cookie_value(&cookie_0),
            expiry: SessionExpiry::Never,
            data: 1,
        },
        Operation::ReadSession {
            id: SessionId::from_cookie_value(&cookie_0),
        },
        Operation::ReadSession {
            id: SessionId::from_cookie_value(&cookie_0),
        },
        Operation::UpdateSession {
            current_id: SessionId::from_cookie_value(&cookie_1),
            previous_id: SessionId::from_cookie_value(&cookie_0),
            expiry: SessionExpiry::Never,
            data: 2,
        },
        Operation::UpdateSession {
            current_id: SessionId::from_cookie_value(&cookie_2),
            previous_id: SessionId::from_cookie_value(&cookie_0),
            expiry: SessionExpiry::Never,
            data: 3,
        },
        Operation::ReadSession {
            id: SessionId::from_cookie_value(&cookie_0),
        },
    ];
    assert_eq!(actual, expected, "{actual:#?}\n!=\n{expected:#?}",);
}

/// Ensure that creating a session store with default parameters results in long enough session tokens.
#[async_std::test]
async fn test_default_cookie_length() {
    let session_store: SessionStore<bool, _, _> =
        SessionStore::new(MemoryStore::new(), SessionRenewalStrategy::Ignore);
    let mut session = Session::new();
    *session.data_mut() = true;
    if let SessionCookieCommand::Set { cookie_value, .. } =
        session_store.store_session(session).await.unwrap()
    {
        assert!(cookie_value.len() >= 32);
    } else {
        panic!("Unexpected session cookie command.");
    }
}

/// Ensure that the expiry of sessions that expire automatically is set correctly.
#[async_std::test]
async fn test_automatic_setting_of_session_expiry() {
    let ttl = Duration::hours(24);
    let mut session_store: SessionStore<bool, _, _> = SessionStore::new(
        MemoryStore::new(),
        SessionRenewalStrategy::AutomaticRenewal {
            time_to_live: ttl,
            maximum_remaining_time_to_live_for_renewal: Duration::hours(12),
        },
    );
    let mut session = Session::new();
    *session.data_mut() = true;

    let now = Utc::now();
    let now_lower = now - Duration::minutes(1);
    let now_upper = now + Duration::minutes(1);

    if let SessionCookieCommand::Set { expiry, .. } =
        session_store.store_session(session).await.unwrap()
    {
        if let SessionExpiry::DateTime(expiry) = expiry {
            assert!(expiry >= now_lower + ttl && expiry <= now_upper + ttl);
        } else {
            panic!("Expiry not set");
        }
    } else {
        panic!("Unexpected session cookie command.");
    }

    let ttl = Duration::hours(12);
    *session_store.session_renewal_strategy_mut() = SessionRenewalStrategy::AutomaticRenewal {
        time_to_live: ttl,
        maximum_remaining_time_to_live_for_renewal: Duration::hours(6),
    };
    let mut session = Session::new();
    *session.data_mut() = true;

    let now = Utc::now();
    let now_lower = now - Duration::minutes(1);
    let now_upper = now + Duration::minutes(1);

    if let SessionCookieCommand::Set { expiry, .. } =
        session_store.store_session(session).await.unwrap()
    {
        if let SessionExpiry::DateTime(expiry) = expiry {
            assert!(expiry >= now_lower + ttl && expiry <= now_upper + ttl);
        } else {
            panic!("Expiry not set");
        }
    } else {
        panic!("Unexpected session cookie command.");
    }
}

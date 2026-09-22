// Integration tests for the event store's public API (AD-1, AD-2).
// These exercise the store from outside the crate, the way shells and
// future domain modules will use it.
use research_core_lib::eventstore::{Actor, EventStore, NewEvent};
use rusqlite::Connection;
use serde_json::json;
use std::sync::Arc;

fn temp_db_path() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("rc-eventstore-test-{}.sqlite", uuid::Uuid::new_v4()));
    p
}

fn open_conn(path: &std::path::Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=10000;")
        .unwrap();
    conn
}

#[test]
fn concurrent_appends_from_two_threads_stay_monotonic_and_unique() {
    let path = temp_db_path();
    {
        let conn = open_conn(&path);
        EventStore::init(&conn).unwrap();
    }

    const PER_THREAD: usize = 50;
    let path = Arc::new(path);
    let mut handles = Vec::new();
    for t in 0..2 {
        let path = Arc::clone(&path);
        handles.push(std::thread::spawn(move || {
            let conn = open_conn(&path);
            let store = EventStore::new(&conn);
            for i in 0..PER_THREAD {
                let ev = NewEvent::new("test.appended", Actor::User, json!({"thread": t, "i": i}))
                    .unwrap();
                store.append(ev).unwrap();
            }
        }));
    }
    for h in handles {
        h.join().expect("appender thread panicked");
    }

    let conn = open_conn(&path);
    let store = EventStore::new(&conn);
    let all = store.events_all().unwrap();
    assert_eq!(all.len(), 2 * PER_THREAD);

    // seq: strictly monotonic 1..=100, no gaps, no duplicates — the sole
    // ordering (AD-2), even across concurrent appends.
    let seqs: Vec<i64> = all.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, (1..=(2 * PER_THREAD) as i64).collect::<Vec<_>>());

    // ids: 100 distinct uuid v4s.
    let mut ids = std::collections::HashSet::new();
    for e in &all {
        assert_eq!(e.id.get_version_num(), 4);
        assert!(ids.insert(e.id), "duplicate event id across threads");
    }
    assert_eq!(ids.len(), 2 * PER_THREAD);

    // every envelope complete on read-back
    assert_eq!(store.head_seq().unwrap(), (2 * PER_THREAD) as i64);
    for e in &all {
        assert_eq!(e.kind, "test.appended");
        assert_eq!(e.actor, Actor::User);
        assert!(e.payload.get("thread").is_some());
        assert!(e.causes.is_empty());
    }

    drop(conn);
    let _ = std::fs::remove_file(&*path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
}

#[test]
fn events_from_returns_the_tail_from_a_named_seq() {
    let path = temp_db_path();
    let conn = open_conn(&path);
    EventStore::init(&conn).unwrap();
    let store = EventStore::new(&conn);
    for i in 0..5 {
        store
            .append(NewEvent::new("test.appended", Actor::User, json!({"i": i})).unwrap())
            .unwrap();
    }
    let tail = store.events_from(4).unwrap();
    assert_eq!(tail.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![4, 5]);
    drop(conn);
    let _ = std::fs::remove_file(&path);
}

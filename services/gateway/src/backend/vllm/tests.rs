use super::{CacheEvents, Source};
use crate::{
    backend::{CacheEvent, CacheIndex},
    gateway::pool::tests::input,
};
use std::time::Duration;
use zeromq::{DealerSocket, PubSocket, RouterSocket, Socket, SocketRecv, SocketSend, ZmqMessage};

const STORED: &[u8] = include_bytes!("../../testdata/kv-events/stored.msgpack");
const REMOVED: &[u8] = include_bytes!("../../testdata/kv-events/removed.msgpack");
const CLEARED: &[u8] = include_bytes!("../../testdata/kv-events/cleared.msgpack");

fn message(sequence: u64, payload: &[u8]) -> ZmqMessage {
    vec![
        axum::body::Bytes::from_static(b"cache"),
        sequence.to_be_bytes().to_vec().into(),
        payload.to_vec().into(),
    ]
    .try_into()
    .unwrap()
}

#[test]
fn native_wire_events_update_residency_and_filter_other_storage() {
    let input = input();
    let events = cache_events(String::new(), String::new());
    let mut cache = CacheIndex::new(&[crate::gateway::pool::tests::full_group(8)], 8).unwrap();
    for (payload, expected) in [(STORED, 24), (REMOVED, 8), (STORED, 24), (CLEARED, 0)] {
        let (sequence, updates) = events
            .decode(message(258, payload), false)
            .unwrap()
            .unwrap();
        assert_eq!(sequence, 258);
        cache.apply(updates).unwrap();
        assert_eq!(cache.cached_tokens(&input), expected);
    }
    for payload in [
        include_bytes!("../../testdata/kv-events/cpu.msgpack").as_slice(),
        include_bytes!("../../testdata/kv-events/remote.msgpack"),
    ] {
        assert!(
            events
                .decode(message(0, payload), false)
                .unwrap()
                .unwrap()
                .1
                .is_empty()
        );
    }
}

#[test]
fn malformed_hashes_topics_sequences_and_frames_are_rejected() {
    let mut events = cache_events(String::new(), String::new());
    for payload in [
        include_bytes!("../../testdata/kv-events/integer-hash.msgpack").as_slice(),
        include_bytes!("../../testdata/kv-events/short-hash.msgpack"),
        b"bad",
    ] {
        assert!(events.decode(message(0, payload), false).is_err());
    }
    events.sources[0].topic = "other".into();
    assert!(events.decode(message(0, STORED), false).is_err());
    events.sources[0].topic = "cache".into();
    assert!(
        events
            .decode(ZmqMessage::from("only one frame"), false)
            .is_err()
    );
    let short = vec![
        axum::body::Bytes::from_static(b"cache"),
        vec![0_u8].into(),
        STORED.to_vec().into(),
    ]
    .try_into()
    .unwrap();
    assert!(events.decode(short, false).is_err());
}

fn cache_events(endpoint: String, replay_endpoint: String) -> CacheEvents {
    CacheEvents {
        instance_id: "instance".into(),
        cache_groups: vec![crate::gateway::pool::tests::full_group(8)],
        sources: vec![Source {
            data_parallel_rank: 0,
            enable_kv_cache_events: true,
            publisher: "zmq".into(),
            endpoint,
            replay_endpoint,
            topic: "cache".into(),
        }],
    }
}

async fn reply(router: &mut RouterSocket, request: &ZmqMessage, batches: &[(u64, &[u8])]) {
    assert_eq!(request.len(), 3);
    assert!(request.get(1).unwrap().is_empty());
    assert_eq!(request.get(2).unwrap().as_ref(), 0_u64.to_be_bytes());
    for &(sequence, payload) in batches {
        let mut response = message(sequence, payload);
        response.push_front(Vec::new().into());
        response.push_front(request.get(0).unwrap().clone());
        router.send(response).await.unwrap();
    }
    let end = vec![
        request.get(0).unwrap().clone(),
        Vec::new().into(),
        Vec::new().into(),
        u64::MAX.to_be_bytes().to_vec().into(),
        Vec::new().into(),
    ]
    .try_into()
    .unwrap();
    router.send(end).await.unwrap();
}

#[tokio::test]
async fn truncated_replay_rebuilds_atomically_and_a_gap_discards_old_blocks() {
    let mut router = RouterSocket::new();
    let address = router.bind("tcp://127.0.0.1:0").await.unwrap().to_string();
    let cache_events = cache_events(String::new(), address.clone());
    let mut dealer = DealerSocket::new();
    dealer.connect(&address).await.unwrap();
    let input = input();
    let recover = async {
        let (cache, next) = cache_events
            .replay_cache(&mut dealer, 8, Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(next, 7);
        assert_eq!(cache.cached_tokens(&input), 8);
        let (cache, next) = cache_events
            .replay_cache(&mut dealer, 8, Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(next, 8);
        assert_eq!(cache.cached_tokens(&input), 0);
    };
    let publish = async {
        let request = router.recv().await.unwrap();
        reply(&mut router, &request, &[(5, STORED), (6, REMOVED)]).await;
        let request = router.recv().await.unwrap();
        reply(&mut router, &request, &[(5, STORED), (7, REMOVED)]).await;
    };
    tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(recover, publish);
    })
    .await
    .unwrap();
}

async fn next_event(events: &mut tokio::sync::mpsc::UnboundedReceiver<CacheEvent>) -> CacheEvent {
    tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn live_gap_withdraws_before_replay_and_duplicates_are_ignored() {
    let mut publisher = PubSocket::new();
    let endpoint = publisher
        .bind("tcp://127.0.0.1:0")
        .await
        .unwrap()
        .to_string();
    let mut router = RouterSocket::new();
    let replay_endpoint = router.bind("tcp://127.0.0.1:0").await.unwrap().to_string();
    let cache_events = cache_events(endpoint, replay_endpoint);
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        cache_events
            .consume(8, Duration::from_secs(2), |event| {
                tx.send(event).unwrap();
                Ok(())
            })
            .await
    });
    assert!(matches!(
        next_event(&mut events).await,
        CacheEvent::Unavailable
    ));
    let request = tokio::time::timeout(Duration::from_secs(2), router.recv())
        .await
        .unwrap()
        .unwrap();
    reply(&mut router, &request, &[(0, STORED)]).await;
    let CacheEvent::Snapshot(cache) = next_event(&mut events).await else {
        panic!("completed replay must publish a snapshot")
    };
    assert_eq!(cache.cached_tokens(&input()), 24);
    publisher.send(message(2, REMOVED)).await.unwrap();
    assert!(matches!(
        next_event(&mut events).await,
        CacheEvent::Unavailable
    ));
    let request = tokio::time::timeout(Duration::from_secs(2), router.recv())
        .await
        .unwrap()
        .unwrap();
    reply(
        &mut router,
        &request,
        &[(0, STORED), (1, CLEARED), (2, REMOVED)],
    )
    .await;
    let CacheEvent::Snapshot(mut cache) = next_event(&mut events).await else {
        panic!("completed replay must publish a snapshot")
    };
    assert_eq!(cache.cached_tokens(&input()), 0);
    publisher.send(message(0, STORED)).await.unwrap(); // Buffered replay duplicate.
    publisher.send(message(3, STORED)).await.unwrap();
    let CacheEvent::Batch(batch) = next_event(&mut events).await else {
        panic!("next live sequence must publish a batch")
    };
    cache.apply(batch).unwrap();
    assert_eq!(cache.cached_tokens(&input()), 24);
    drop(publisher);
    let error = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(events.recv().await.is_none());
}

#[tokio::test]
async fn gap_replay_must_cover_the_observed_sequence_before_restoring_credit() {
    let mut publisher = PubSocket::new();
    let endpoint = publisher
        .bind("tcp://127.0.0.1:0")
        .await
        .unwrap()
        .to_string();
    let mut router = RouterSocket::new();
    let replay_endpoint = router.bind("tcp://127.0.0.1:0").await.unwrap().to_string();
    let cache_events = cache_events(endpoint, replay_endpoint);
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        cache_events
            .consume(8, Duration::from_secs(2), |event| {
                tx.send(event).unwrap();
                Ok(())
            })
            .await
    });
    assert!(matches!(
        next_event(&mut events).await,
        CacheEvent::Unavailable
    ));
    let request = tokio::time::timeout(Duration::from_secs(2), router.recv())
        .await
        .unwrap()
        .unwrap();
    reply(&mut router, &request, &[(0, STORED)]).await;
    assert!(matches!(
        next_event(&mut events).await,
        CacheEvent::Snapshot(_)
    ));
    publisher.send(message(2, STORED)).await.unwrap();
    assert!(matches!(
        next_event(&mut events).await,
        CacheEvent::Unavailable
    ));
    let request = tokio::time::timeout(Duration::from_secs(2), router.recv())
        .await
        .unwrap()
        .unwrap();
    reply(&mut router, &request, &[(0, STORED)]).await;
    let error = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(
        events.recv().await.is_none(),
        "an incomplete replay must not publish a snapshot"
    );
}

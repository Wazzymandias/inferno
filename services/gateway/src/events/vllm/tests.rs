use super::{Discovery, Source, Subscription, decode};
use crate::{
    backend::{
        CacheIndex,
        tests::{input, pool, selected},
    },
    inference::ModelConfig,
};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
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
    let mut cache = CacheIndex::new(&[8], 8).unwrap();
    for (payload, expected) in [(STORED, 24), (REMOVED, 8), (STORED, 24), (CLEARED, 0)] {
        let (sequence, updates) = decode(message(258, payload), "cache", false)
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
            decode(message(0, payload), "cache", false)
                .unwrap()
                .unwrap()
                .1
                .is_empty()
        );
    }
}

#[test]
fn malformed_hashes_topics_sequences_and_frames_are_rejected() {
    for payload in [
        include_bytes!("../../testdata/kv-events/integer-hash.msgpack").as_slice(),
        include_bytes!("../../testdata/kv-events/short-hash.msgpack"),
        b"bad",
    ] {
        assert!(decode(message(0, payload), "cache", false).is_err());
    }
    assert!(decode(message(0, STORED), "other", false).is_err());
    assert!(decode(ZmqMessage::from("only one frame"), "cache", false).is_err());
    let short = vec![
        axum::body::Bytes::from_static(b"cache"),
        vec![0_u8].into(),
        STORED.to_vec().into(),
    ]
    .try_into()
    .unwrap();
    assert!(decode(short, "cache", false).is_err());
}

fn subscription(endpoint: String, replay_endpoint: String) -> Subscription {
    let pool = pool(1);
    Subscription {
        backend: pool.rank(None).unwrap().0.clone(),
        model: "fixture".into(),
        config: Arc::new(
            ModelConfig::parse(
                include_bytes!("../../testdata/input-string/model-config.json"),
                "fixture",
            )
            .unwrap(),
        ),
        timeout: Duration::from_secs(2),
        discovery: Discovery {
            instance_id: "instance".into(),
            cache_groups: vec![8],
            sources: vec![Source {
                data_parallel_rank: 0,
                enable_kv_cache_events: true,
                publisher: "zmq".into(),
                endpoint,
                replay_endpoint,
                topic: "cache".into(),
            }],
        },
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
    let subscription = subscription(String::new(), address.clone());
    let mut dealer = DealerSocket::new();
    dealer.connect(&address).await.unwrap();
    let input = input();
    let pool = pool(2);
    let recover = async {
        let next = subscription
            .replay(&pool, 1, &subscription.discovery, &mut dealer)
            .await
            .unwrap();
        assert_eq!(next, 7);
        assert_eq!(selected(&pool, &input), "replica-1");
        let next = subscription
            .replay(&pool, 1, &subscription.discovery, &mut dealer)
            .await
            .unwrap();
        assert_eq!(next, 8);
        assert_eq!(selected(&pool, &input), "replica-0");
    };
    let publish = async {
        let request = router.recv().await.unwrap();
        assert_eq!(selected(&pool, &input), "replica-0");
        reply(&mut router, &request, &[(5, STORED), (6, REMOVED)]).await;
        let request = router.recv().await.unwrap();
        assert_eq!(selected(&pool, &input), "replica-0");
        reply(&mut router, &request, &[(5, STORED), (7, REMOVED)]).await;
    };
    tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(recover, publish);
    })
    .await
    .unwrap();
}

async fn wait_selected(pool: &crate::backend::Pool, expected: &str) {
    let input = input();
    tokio::time::timeout(Duration::from_secs(2), async {
        while selected(pool, &input) != expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn live_gap_replays_and_disconnect_clears_residency() {
    let mut publisher = PubSocket::new();
    let endpoint = publisher
        .bind("tcp://127.0.0.1:0")
        .await
        .unwrap()
        .to_string();
    let mut router = RouterSocket::new();
    let replay_endpoint = router.bind("tcp://127.0.0.1:0").await.unwrap().to_string();
    let subscription = subscription(endpoint, replay_endpoint);
    let pool = Arc::new(pool(2));
    let observed = Arc::clone(&pool);
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    let task = tokio::spawn(async move { subscription.run(&observed, 1, stop).await });
    let request = tokio::time::timeout(Duration::from_secs(2), router.recv())
        .await
        .unwrap()
        .unwrap();
    reply(&mut router, &request, &[(0, STORED)]).await;
    wait_selected(&pool, "replica-1").await;
    publisher.send(message(2, REMOVED)).await.unwrap();
    let request = tokio::time::timeout(Duration::from_secs(2), router.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(selected(&pool, &input()), "replica-0");
    reply(
        &mut router,
        &request,
        &[(0, STORED), (1, CLEARED), (2, REMOVED)],
    )
    .await;
    publisher.send(message(0, STORED)).await.unwrap(); // Buffered replay duplicate.
    publisher.send(message(3, STORED)).await.unwrap();
    wait_selected(&pool, "replica-1").await;
    drop(publisher);
    wait_selected(&pool, "replica-0").await;
    shutdown.cancel();
    task.await.unwrap().unwrap();
}

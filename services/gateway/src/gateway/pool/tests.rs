use super::{Pool, RequestLease};
use crate::backend::{CacheEvent, CacheGroup, CacheIndex, CacheUpdate};
use crate::inference::{InputProcessor, ModelConfig, ModelInput};
use std::time::Duration;

pub(crate) fn input() -> ModelInput {
    let processor = InputProcessor::load(
        &ModelConfig::parse(
            include_bytes!("../../testdata/input-string/model-config.json"),
            "fixture",
        )
        .unwrap(),
    )
    .unwrap();
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(include_bytes!("../../testdata/input-string/parity.json")).unwrap();
    let case = cases
        .iter()
        .find(|case| case["name"] == "unicode-whitespace")
        .unwrap();
    processor
        .prepare(&serde_json::from_value(case["request"].clone()).unwrap())
        .unwrap()
}

pub(crate) fn pool(count: usize) -> Pool {
    let pool = Pool {
        model: "fixture".into(),
        model_config: ModelConfig::parse(
            include_bytes!("../../testdata/input-string/model-config.json"),
            "fixture",
        )
        .unwrap(),
        timeout: Duration::from_secs(2),
        load_penalty: 8,
        backends: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    for id in 0..count {
        pool.add(
            format!("http://replica-{id}/v1").parse().unwrap(),
            Duration::from_secs(1),
            None,
        )
        .unwrap();
    }
    pool
}

pub(crate) fn full_group(block_size: usize) -> CacheGroup {
    CacheGroup {
        block_size,
        required_blocks: None,
    }
}

fn store(input: &ModelInput, blocks: usize) -> CacheIndex {
    let mut cache = CacheIndex::new(&[full_group(input.block_size())], input.block_size()).unwrap();
    cache
        .apply(vec![CacheUpdate::Store {
            group: 0,
            block_size: input.block_size(),
            hashes: input.prefix_hashes()[..blocks].to_vec(),
        }])
        .unwrap();
    cache
}

pub(crate) fn selected(pool: &Pool, input: &ModelInput) -> String {
    reserved_host(&pool.rank(Some(input)).unwrap())
}

/// Observe the selected destination through [`RequestLease::request`], which
/// must return an owned builder without retaining the pool's lock.
fn reserved_host(lease: &RequestLease) -> String {
    lease
        .request(reqwest::Method::POST, "responses", None)
        .build()
        .unwrap()
        .url()
        .host_str()
        .unwrap()
        .to_owned()
}

#[test]
fn longest_prefix_competes_with_active_requests_and_releases_on_drop() {
    let input = input();
    let pool = pool(3);
    for (id, count) in [3, 4, 2].into_iter().enumerate() {
        pool.update_cache(id, CacheEvent::Snapshot(store(&input, count)))
            .unwrap();
    }
    let lease = pool.rank(Some(&input)).unwrap();
    assert_eq!(reserved_host(&lease), "replica-1");
    assert_eq!(selected(&pool, &input), "replica-0");
    drop(lease);
    assert_eq!(selected(&pool, &input), "replica-1");
}

#[test]
fn cache_updates_preserve_reservations_and_leases_outlive_the_pool() {
    let input = input();
    let pool = pool(1);
    let backends = std::sync::Arc::clone(&pool.backends);
    let first = pool.rank(None).unwrap();
    let second = pool.rank(None).unwrap();
    let request = first.request(reqwest::Method::POST, "responses", None);

    pool.update_cache(0, CacheEvent::Snapshot(store(&input, 3)))
        .unwrap();
    assert_eq!(
        backends.lock().unwrap()[0]
            .cache
            .as_ref()
            .unwrap()
            .cached_tokens(&input),
        3 * input.block_size(),
    );
    pool.update_cache(0, CacheEvent::Unavailable).unwrap();
    assert!(backends.lock().unwrap()[0].cache.is_none());
    assert_eq!(backends.lock().unwrap()[0].active_requests, 2);

    drop(pool);
    drop(first);
    assert_eq!(backends.lock().unwrap()[0].active_requests, 1);
    assert_eq!(request.build().unwrap().url().host_str(), Some("replica-0"));
    drop(second);
    assert_eq!(backends.lock().unwrap()[0].active_requests, 0);
}

#[tokio::test]
async fn shutdown_during_replay_withdraws_cache_and_preserves_requests() {
    use tokio_util::sync::CancellationToken;
    use zeromq::{PubSocket, RouterSocket, Socket, SocketRecv};

    let mut publisher = PubSocket::new();
    let endpoint = publisher
        .bind("tcp://127.0.0.1:0")
        .await
        .unwrap()
        .to_string();
    let mut replay = RouterSocket::new();
    let replay_endpoint = replay.bind("tcp://127.0.0.1:0").await.unwrap().to_string();
    let cache_events = serde_json::from_value(serde_json::json!({
        "instance_id": "replica-1",
        "cache_groups": [{"block_size": 8, "required_blocks": null}],
        "sources": [{
            "data_parallel_rank": 0, "enable_kv_cache_events": true,
            "publisher": "zmq", "endpoint": endpoint,
            "replay_endpoint": replay_endpoint, "topic": "cache"
        }]
    }))
    .unwrap();
    let input = input();
    let pool = std::sync::Arc::new(pool(2));
    pool.backends.lock().unwrap()[1].cache_events = Some(std::sync::Arc::new(cache_events));
    pool.update_cache(0, CacheEvent::Snapshot(store(&input, 2)))
        .unwrap();
    pool.update_cache(1, CacheEvent::Snapshot(store(&input, 3)))
        .unwrap();
    let lease = pool.rank(Some(&input)).unwrap();
    assert_eq!(reserved_host(&lease), "replica-1");
    let shutdown = CancellationToken::new();
    let observed = std::sync::Arc::clone(&pool);
    let stop = shutdown.clone();
    let observation = tokio::spawn(async move { observed.subscribe_events(stop).await });
    tokio::time::timeout(Duration::from_secs(1), replay.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(pool.backends.lock().unwrap()[1].cache.is_none());
    assert_eq!(selected(&pool, &input), "replica-0");
    pool.update_cache(0, CacheEvent::Batch(vec![CacheUpdate::Clear]))
        .unwrap();
    assert_eq!(
        pool.backends.lock().unwrap()[0]
            .cache
            .as_ref()
            .unwrap()
            .cached_tokens(&input),
        0
    );
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(1), observation)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(pool.backends.lock().unwrap()[1].cache.is_none());
    assert_eq!(pool.backends.lock().unwrap()[1].active_requests, 1);
    drop(lease);
    assert_eq!(pool.backends.lock().unwrap()[1].active_requests, 0);
}

#[test]
fn concurrent_reservations_share_load_and_preserve_backend_order() {
    let pool = pool(2);
    let reserved = std::sync::Barrier::new(8);
    let chosen = std::thread::scope(|scope| {
        let requests: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    let lease = pool.rank(None).unwrap();
                    let host = reserved_host(&lease);
                    reserved.wait();
                    drop(lease);
                    host
                })
            })
            .collect();
        requests
            .into_iter()
            .map(|request| request.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(chosen.iter().filter(|host| *host == "replica-0").count(), 4);
    assert_eq!(chosen.iter().filter(|host| *host == "replica-1").count(), 4);
    assert_eq!(reserved_host(&pool.rank(None).unwrap()), "replica-0",);
}

#[test]
fn eviction_stops_at_the_missing_ancestor_and_clear_is_replica_local() {
    let input = input();
    let pool = pool(3);
    for (id, count) in [3, 4, 2].into_iter().enumerate() {
        pool.update_cache(id, CacheEvent::Snapshot(store(&input, count)))
            .unwrap();
    }
    pool.update_cache(
        1,
        CacheEvent::Batch(vec![CacheUpdate::Remove {
            group: 0,
            hashes: vec![input.prefix_hashes()[1]],
        }]),
    )
    .unwrap();
    assert_eq!(selected(&pool, &input), "replica-0");
    pool.update_cache(0, CacheEvent::Batch(vec![CacheUpdate::Clear]))
        .unwrap();
    assert_eq!(selected(&pool, &input), "replica-2");
    pool.update_cache(2, CacheEvent::Unavailable).unwrap();
    assert_eq!(selected(&pool, &input), "replica-1");
}

#[test]
fn every_cache_group_must_match_at_a_shared_block_boundary() {
    let input = input();
    let size = input.block_size();
    let mut cache = CacheIndex::new(&[full_group(size), full_group(2 * size)], size).unwrap();
    cache
        .apply(vec![CacheUpdate::Store {
            group: 0,
            block_size: size,
            hashes: input.prefix_hashes()[..3].to_vec(),
        }])
        .unwrap();
    assert_eq!(cache.cached_tokens(&input), 0);
    cache
        .apply(vec![CacheUpdate::Store {
            group: 1,
            block_size: 2 * size,
            hashes: vec![input.prefix_hashes()[1], input.prefix_hashes()[3]],
        }])
        .unwrap();
    assert_eq!(cache.cached_tokens(&input), 2 * size);
    cache
        .apply(vec![CacheUpdate::Remove {
            group: 1,
            hashes: vec![input.prefix_hashes()[1]],
        }])
        .unwrap();
    assert_eq!(cache.cached_tokens(&input), 0);
}

#[test]
fn malformed_batch_discards_all_residency_without_changing_load() {
    let input = input();
    let pool = pool(2);
    pool.update_cache(1, CacheEvent::Snapshot(store(&input, 3)))
        .unwrap();
    let lease = pool.rank(Some(&input)).unwrap();
    assert!(
        pool.update_cache(
            1,
            CacheEvent::Batch(vec![
                CacheUpdate::Clear,
                CacheUpdate::Store {
                    group: 0,
                    block_size: 999,
                    hashes: vec![]
                }
            ])
        )
        .is_err()
    );
    assert!(pool.backends.lock().unwrap()[1].cache.is_none());
    assert_eq!(pool.backends.lock().unwrap()[1].active_requests, 1);
    assert_eq!(selected(&pool, &input), "replica-0");
    drop(lease);
    let lease = pool.rank(None).unwrap();
    assert_eq!(selected(&pool, &input), "replica-1");
    drop(lease);
    assert_eq!(selected(&pool, &input), "replica-0");
}

#[test]
fn invalid_cache_granularity_is_rejected() {
    for (groups, hash) in [(vec![], 8), (vec![0], 8), (vec![7], 8), (vec![8], 0)] {
        let groups: Vec<_> = groups.into_iter().map(full_group).collect();
        assert!(CacheIndex::new(&groups, hash).is_err());
    }
}

#[test]
fn sparse_checkpoints_only_count_at_the_same_reusable_boundary() {
    let input = input();
    let size = input.block_size();
    let checkpoint = CacheGroup {
        block_size: size,
        required_blocks: std::num::NonZeroUsize::new(1),
    };
    let mut cache = CacheIndex::new(&[checkpoint, checkpoint, full_group(size)], size).unwrap();
    for (group, indices) in [(0, vec![1, 3]), (1, vec![2, 3]), (2, vec![0, 1, 2, 3])] {
        cache
            .apply(vec![CacheUpdate::Store {
                group,
                block_size: size,
                hashes: indices
                    .into_iter()
                    .map(|index| input.prefix_hashes()[index])
                    .collect(),
            }])
            .unwrap();
    }
    assert_eq!(cache.cached_tokens(&input), 4 * size);
    cache
        .apply(vec![CacheUpdate::Remove {
            group: 0,
            hashes: vec![input.prefix_hashes()[3]],
        }])
        .unwrap();
    assert_eq!(cache.cached_tokens(&input), 0);
    cache
        .apply(vec![CacheUpdate::Store {
            group: 0,
            block_size: size,
            hashes: vec![input.prefix_hashes()[3]],
        }])
        .unwrap();
    let pool = pool(2);
    pool.update_cache(0, CacheEvent::Snapshot(store(&input, 3)))
        .unwrap();
    pool.update_cache(1, CacheEvent::Snapshot(cache)).unwrap();
    assert_eq!(selected(&pool, &input), "replica-1");
    pool.update_cache(
        1,
        CacheEvent::Batch(vec![CacheUpdate::Remove {
            group: 0,
            hashes: vec![input.prefix_hashes()[3]],
        }]),
    )
    .unwrap();
    assert_eq!(selected(&pool, &input), "replica-0");
}

#[test]
fn windows_require_contiguous_tail_blocks_but_allow_shorter_initial_prefixes() {
    let input = input();
    let size = input.block_size();
    let window = CacheGroup {
        block_size: size,
        required_blocks: std::num::NonZeroUsize::new(3),
    };
    let mut cache = CacheIndex::new(&[full_group(size), window], size).unwrap();
    cache
        .apply(vec![
            CacheUpdate::Store {
                group: 0,
                block_size: size,
                hashes: input.prefix_hashes()[..4].to_vec(),
            },
            CacheUpdate::Store {
                group: 1,
                block_size: size,
                hashes: input.prefix_hashes()[1..4].to_vec(),
            },
        ])
        .unwrap();
    assert_eq!(cache.cached_tokens(&input), 4 * size);
    cache
        .apply(vec![CacheUpdate::Remove {
            group: 1,
            hashes: vec![input.prefix_hashes()[1]],
        }])
        .unwrap();
    assert_eq!(cache.cached_tokens(&input), 0);
    cache
        .apply(vec![CacheUpdate::Store {
            group: 1,
            block_size: size,
            hashes: vec![input.prefix_hashes()[0]],
        }])
        .unwrap();
    assert_eq!(cache.cached_tokens(&input), size);
}

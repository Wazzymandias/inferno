use super::{CacheGroup, CacheIndex, CacheUpdate, Pool};
use crate::inference::{InputProcessor, ModelConfig, ModelInput};
use std::time::Duration;

pub(crate) fn input() -> ModelInput {
    let processor = InputProcessor::load(
        ModelConfig::parse(
            include_bytes!("../testdata/input-string/model-config.json"),
            "fixture",
        )
        .unwrap(),
    )
    .unwrap();
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(include_bytes!("../testdata/input-string/parity.json")).unwrap();
    let case = cases
        .iter()
        .find(|case| case["name"] == "unicode-whitespace")
        .unwrap();
    processor
        .prepare(&serde_json::from_value(case["request"].clone()).unwrap())
        .unwrap()
}

pub(crate) fn pool(count: usize) -> Pool {
    let mut pool = Pool::new().with_load_penalty(8);
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
    pool.rank(Some(input))
        .unwrap()
        .0
        .url("responses", None)
        .host_str()
        .unwrap()
        .to_owned()
}

#[test]
fn longest_prefix_competes_with_active_requests_and_releases_on_drop() {
    let input = input();
    let pool = pool(3);
    for (id, count) in [3, 4, 2].into_iter().enumerate() {
        pool.replace_cache(id, Some(store(&input, count)));
    }
    let (backend, lease) = pool.rank(Some(&input)).unwrap();
    assert_eq!(backend.url("responses", None).host_str(), Some("replica-1"));
    assert_eq!(selected(&pool, &input), "replica-0");
    drop(lease);
    assert_eq!(selected(&pool, &input), "replica-1");
}

#[test]
fn eviction_stops_at_the_missing_ancestor_and_clear_is_replica_local() {
    let input = input();
    let pool = pool(3);
    for (id, count) in [3, 4, 2].into_iter().enumerate() {
        pool.replace_cache(id, Some(store(&input, count)));
    }
    pool.apply_batch(
        1,
        vec![CacheUpdate::Remove {
            group: 0,
            hashes: vec![input.prefix_hashes()[1]],
        }],
    )
    .unwrap();
    assert_eq!(selected(&pool, &input), "replica-0");
    pool.apply_batch(0, vec![CacheUpdate::Clear]).unwrap();
    assert_eq!(selected(&pool, &input), "replica-2");
    pool.replace_cache(2, None);
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
    pool.replace_cache(1, Some(store(&input, 3)));
    assert!(
        pool.apply_batch(
            1,
            vec![CacheUpdate::Store {
                group: 0,
                block_size: 999,
                hashes: vec![]
            }]
        )
        .is_err()
    );
    assert_eq!(selected(&pool, &input), "replica-0");
    let (_, lease) = pool.rank(None).unwrap();
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
    pool.replace_cache(0, Some(store(&input, 3)));
    pool.replace_cache(1, Some(cache));
    assert_eq!(selected(&pool, &input), "replica-1");
    pool.apply_batch(
        1,
        vec![CacheUpdate::Remove {
            group: 0,
            hashes: vec![input.prefix_hashes()[3]],
        }],
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

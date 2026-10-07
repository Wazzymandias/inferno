//! Prefix residency at a boundary satisfying every cache group's retained history.

use crate::inference::{BlockHash, ModelInput};
use std::{collections::HashSet, io};

#[derive(Clone, Copy, serde::Deserialize)]
pub(crate) struct CacheGroup {
    pub(crate) block_size: usize,
    /// None requires the entire prefix; otherwise require this many trailing blocks.
    pub(crate) required_blocks: Option<std::num::NonZeroUsize>,
}

pub(crate) enum CacheUpdate {
    Store {
        group: usize,
        block_size: usize,
        hashes: Vec<BlockHash>,
    },
    Remove {
        group: usize,
        hashes: Vec<BlockHash>,
    },
    Clear,
}

impl std::fmt::Debug for CacheUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store {
                group,
                block_size,
                hashes,
            } => f
                .debug_struct("Store")
                .field("group", group)
                .field("block_size", block_size)
                .field("blocks", &hashes.len())
                .finish(),
            Self::Remove { group, hashes } => f
                .debug_struct("Remove")
                .field("group", group)
                .field("blocks", &hashes.len())
                .finish(),
            Self::Clear => f.write_str("Clear"),
        }
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(crate) struct CacheIndex {
    groups: Vec<(CacheGroup, HashSet<BlockHash>)>,
    alignment: usize,
}

impl std::fmt::Debug for CacheIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CacheIndex")
            .field("groups", &self.groups.len())
            .finish_non_exhaustive()
    }
}

impl CacheIndex {
    pub(crate) fn new(groups: &[CacheGroup], hash_size: usize) -> io::Result<Self> {
        if hash_size == 0
            || groups.is_empty()
            || groups
                .iter()
                .any(|group| group.block_size == 0 || !group.block_size.is_multiple_of(hash_size))
        {
            return Err(invalid(
                "cache groups must use multiples of the hash block size",
            ));
        }
        Ok(Self {
            alignment: groups.iter().try_fold(1, |alignment, group| {
                let size = group.block_size;
                let (mut a, mut b) = (alignment, size);
                while b != 0 {
                    (a, b) = (b, a % b);
                }
                (alignment / a)
                    .checked_mul(size)
                    .ok_or_else(|| invalid("cache group alignment overflow"))
            })?,
            groups: groups
                .iter()
                .map(|group| (*group, HashSet::new()))
                .collect(),
        })
    }

    pub(crate) fn cached_tokens(&self, input: &ModelInput) -> usize {
        let mut runs = vec![0; self.groups.len()];
        let mut cached = 0;
        for (index, hash) in input.prefix_hashes().iter().enumerate() {
            let tokens = (index + 1) * input.block_size();
            // A physical block uses the hash of its final hash-sized subblock.
            for ((group, hashes), run) in self.groups.iter().zip(&mut runs) {
                if tokens.is_multiple_of(group.block_size) {
                    *run = if hashes.contains(hash) { *run + 1 } else { 0 };
                }
            }
            if tokens.is_multiple_of(self.alignment)
                && self.groups.iter().zip(&runs).all(|((group, _), &run)| {
                    let blocks = tokens / group.block_size;
                    run >= group
                        .required_blocks
                        .map_or(blocks, |required| blocks.min(required.get()))
                })
            {
                cached = tokens;
            }
        }
        cached
    }

    pub(crate) fn apply(&mut self, events: Vec<CacheUpdate>) -> io::Result<()> {
        for event in events {
            match event {
                CacheUpdate::Clear => self.clear(),
                CacheUpdate::Store {
                    hashes: block_hashes,
                    block_size,
                    group: group_idx,
                } => {
                    let (group, hashes) = self
                        .groups
                        .get_mut(group_idx)
                        .ok_or_else(|| invalid("unknown cache group"))?;
                    if block_size != group.block_size {
                        return Err(invalid(
                            "event block size differs from the discovered cache group",
                        ));
                    }
                    hashes.extend(block_hashes);
                }
                CacheUpdate::Remove {
                    hashes: block_hashes,
                    group: group_idx,
                } => {
                    let (_, hashes) = self
                        .groups
                        .get_mut(group_idx)
                        .ok_or_else(|| invalid("unknown cache group"))?;
                    for hash in block_hashes {
                        hashes.remove(&hash);
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn clear(&mut self) {
        for (_, hashes) in &mut self.groups {
            hashes.clear();
        }
    }
}

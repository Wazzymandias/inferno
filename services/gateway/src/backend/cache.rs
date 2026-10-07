//! Contiguous prefix residency across a replica's cache groups.

use crate::inference::{BlockHash, ModelInput};
use std::{collections::HashSet, io};

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
    groups: Vec<(usize, HashSet<BlockHash>)>,
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
    pub(crate) fn new(group_sizes: &[usize], hash_size: usize) -> io::Result<Self> {
        if hash_size == 0
            || group_sizes.is_empty()
            || group_sizes
                .iter()
                .any(|size| *size == 0 || !size.is_multiple_of(hash_size))
        {
            return Err(invalid(
                "cache groups must use multiples of the hash block size",
            ));
        }
        Ok(Self {
            alignment: group_sizes.iter().try_fold(1, |alignment, &size| {
                let (mut a, mut b) = (alignment, size);
                while b != 0 {
                    (a, b) = (b, a % b);
                }
                (alignment / a)
                    .checked_mul(size)
                    .ok_or_else(|| invalid("cache group alignment overflow"))
            })?,
            groups: group_sizes
                .iter()
                .map(|size| (*size, HashSet::new()))
                .collect(),
        })
    }

    pub(crate) fn cached_tokens(&self, input: &ModelInput) -> usize {
        // A group's larger block uses the hash of its final hash-sized subblock.
        // Missing ancestors stop that group's match, even if descendants remain.
        let tokens = self
            .groups
            .iter()
            .map(|(size, hashes)| {
                input
                    .prefix_hashes()
                    .chunks_exact(size / input.block_size())
                    .take_while(|chunk| hashes.contains(chunk.last().unwrap()))
                    .count()
                    * size
            })
            .min()
            .unwrap_or(0);
        tokens / self.alignment * self.alignment
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
                    let (size, hashes) = self
                        .groups
                        .get_mut(group_idx)
                        .ok_or_else(|| invalid("unknown cache group"))?;
                    if block_size != *size {
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

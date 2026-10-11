//! SHA-256 over canonical CBOR: (parent, token tuple, extra keys).
//! Only complete blocks participate; each invocation starts a fresh chain.

use std::num::NonZeroUsize;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{InputError, hugging_face::EncodedInput};

pub(crate) type BlockHash = [u8; 32];

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct PrefixConfig {
    algorithm: String,
    block_size: NonZeroUsize,
    initial_parent: BlockHash,
}

impl PrefixConfig {
    pub(super) const fn block_size(&self) -> usize {
        self.block_size.get()
    }
}

#[derive(Debug)]
pub(super) struct PrefixHasher {
    block_size: NonZeroUsize,
    initial_parent: BlockHash,
}

impl PrefixHasher {
    pub(crate) const fn block_size(&self) -> usize {
        self.block_size.get()
    }

    pub(super) fn new(config: PrefixConfig) -> Result<Self, InputError> {
        if config.algorithm != "sha256_cbor" {
            return Err(InputError::new(
                "prefix.algorithm",
                "only sha256_cbor is supported",
            ));
        }
        Ok(Self {
            block_size: config.block_size,
            initial_parent: config.initial_parent,
        })
    }

    pub(super) fn hash(&self, encoded: &EncodedInput) -> Result<Vec<BlockHash>, InputError> {
        let mut parent = self.initial_parent;
        let mut hashes = Vec::with_capacity(encoded.token_ids.len() / self.block_size);
        for (index, block) in encoded
            .token_ids
            .chunks_exact(self.block_size.get())
            .enumerate()
        {
            let mut cbor = minicbor::Encoder::new(Vec::new());
            // This value contains only arrays, bytes, unsigned integers, text,
            // and null; minicbor emits their shortest canonical encodings.
            let result = (|| {
                cbor.array(3)?.bytes(&parent)?.array(block.len() as u64)?;
                for &token in block {
                    cbor.u32(token)?;
                }
                if let Some(salt) = encoded
                    .cache_salt
                    .as_deref()
                    .filter(|s| !s.is_empty() && index == 0)
                {
                    cbor.array(1)?.array(2)?.str("cache_salt")?.str(salt)?;
                } else {
                    cbor.null()?;
                }
                Ok::<_, minicbor::encode::Error<std::convert::Infallible>>(())
            })();
            result.map_err(|error| {
                InputError::with_source(
                    "prefix",
                    "cannot encode prefix hash input",
                    Box::new(error),
                )
            })?;
            parent = Sha256::digest(cbor.into_writer()).into();
            hashes.push(parent);
        }
        Ok(hashes)
    }
}

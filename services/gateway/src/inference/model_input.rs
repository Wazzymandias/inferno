//! Completed local preparation. Owns its results without request or processor
//! references; accessors never render, tokenize, or hash.

use super::prefix::BlockHash;

pub(crate) struct ModelInput {
    tokens: Vec<u32>,
    prefix_hashes: Vec<BlockHash>,
}

impl ModelInput {
    pub(super) const fn new(tokens: Vec<u32>, prefix_hashes: Vec<BlockHash>) -> Self {
        Self {
            tokens,
            prefix_hashes,
        }
    }

    pub(crate) fn tokens(&self) -> &[u32] {
        &self.tokens
    }

    pub(crate) fn prefix_hashes(&self) -> &[BlockHash] {
        &self.prefix_hashes
    }
}

impl std::fmt::Debug for ModelInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Tokens and hashes can reveal request content; format counts only.
        f.debug_struct("ModelInput")
            .field("token_count", &self.tokens.len())
            .field("prefix_block_count", &self.prefix_hashes.len())
            .finish()
    }
}

//! One reusable owner of a loaded encoder and its matching prefix hash policy.

use super::{
    CreateResponseRequest, InputError, ModelConfig, ModelInput, hugging_face::HuggingFaceEncoder,
    prefix::PrefixHasher,
};

/// Loaded model assets for reproducible request tokenization and prefix hashes.
/// Share this processor across requests; [`InputProcessor::prepare`] performs CPU
/// work and should run outside an async runtime's executor threads.
#[derive(Debug)]
pub(crate) struct InputProcessor {
    encoder: HuggingFaceEncoder,
    prefix_hasher: PrefixHasher,
}

impl InputProcessor {
    /// Compile resolved assets from [`inference::ModelConfig`](crate::inference::ModelConfig)
    /// while leaving it available for reconnect validation. The encoder and hasher
    /// own their preparation settings; request preparation performs no IO.
    pub(crate) fn load(config: &ModelConfig) -> Result<Self, InputError> {
        Ok(Self {
            encoder: HuggingFaceEncoder::load(
                config.tokenizer_json.as_bytes(),
                config.encoder.clone(),
            )?,
            prefix_hasher: PrefixHasher::new(config.prefix.clone())?,
        })
    }

    /// Tokenize and hash a validated [`inference::CreateResponseRequest`](crate::inference::CreateResponseRequest)
    /// for backend selection. Unsupported inputs return [`inference::InputError`](crate::inference::InputError).
    pub(crate) fn prepare(
        &self,
        request: &CreateResponseRequest,
    ) -> Result<ModelInput, InputError> {
        request.validate()?;
        let encoded = self.encoder.encode(request)?;
        let prefix_hashes = self.prefix_hasher.hash(&encoded)?;
        Ok(ModelInput::new(
            encoded.token_ids,
            prefix_hashes,
            self.prefix_hasher.block_size(),
        ))
    }
}

#[cfg(test)]
mod tests;

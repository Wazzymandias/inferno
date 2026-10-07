//! One reusable owner of a loaded encoder and its matching prefix hash policy.

use super::{
    CreateResponseRequest, InputError, ModelConfig, ModelInput, hugging_face::HuggingFaceEncoder,
    prefix::PrefixHasher,
};

#[derive(Debug)]
pub(crate) struct InputProcessor {
    encoder: HuggingFaceEncoder,
    prefix_hasher: PrefixHasher,
}

impl InputProcessor {
    /// Compile the selected model's resolved assets and preparation policy once.
    /// Storage and discovery belong to `ModelConfig`; request preparation does no IO.
    pub(crate) fn load(config: ModelConfig) -> Result<Self, InputError> {
        Ok(Self {
            encoder: HuggingFaceEncoder::load(config.tokenizer_json.as_bytes(), config.encoder)?,
            prefix_hasher: PrefixHasher::new(config.prefix)?,
        })
    }

    pub(crate) fn prepare(
        &self,
        request: &CreateResponseRequest,
    ) -> Result<ModelInput, InputError> {
        request.validate()?;
        let encoded = self.encoder.encode(request)?;
        let prefix_hashes = self.prefix_hasher.hash(&encoded)?;
        Ok(ModelInput::new(encoded.token_ids, prefix_hashes))
    }
}

#[cfg(test)]
mod tests;

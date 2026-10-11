//! Discover the selected deployment's model assets once, retaining an offline
//! inspection copy. Cache identity includes both the backend and served model.

use std::{
    env, fs,
    io::{self, Write},
    path::PathBuf,
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{InputError, hugging_face::EncoderConfig, prefix::PrefixConfig};
use crate::backend::Backend;

/// Tokenizer assets and request/hash settings published by the selected model.
/// Serving backends must agree on these settings for [`super::InputProcessor::prepare`]
/// to produce matching token IDs and prefix hashes.
#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelConfig {
    format: String,
    pub(super) encoder: EncoderConfig,
    pub(super) prefix: PrefixConfig,
    pub(super) tokenizer_json: String,
}

impl std::fmt::Debug for ModelConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelConfig").finish_non_exhaustive()
    }
}

impl ModelConfig {
    /// Tokens in each hashed prefix block. Physical cache groups may span
    /// several of these blocks; incomplete final blocks receive no cache credit.
    pub(crate) const fn block_size(&self) -> usize {
        self.prefix.block_size()
    }
    /// Serving always discovers the active deployment's configuration before
    /// accepting requests. A previous cache entry cannot authorize stale hashes.
    /// The returned future owns its request, allowing a caller to release the
    /// lock protecting [`backend::Backend`](crate::backend::Backend) before
    /// awaiting network or cache-file IO.
    pub(crate) fn discover(
        backend: &Backend,
        model: &str,
    ) -> impl Future<Output = Result<Self, InputError>> + Send + use<> {
        let cache = ModelCache::new(backend, model);
        let request = backend.client.get(config_url(backend, model));
        let model = model.to_owned();
        async move { cache?.discover(request, &model).await }
    }

    /// Inspection uses the last discovered configuration without a connection.
    /// On the first use, obtain the assets automatically from the configured backend.
    pub(crate) async fn for_inspection(backend: &Backend, model: &str) -> Result<Self, InputError> {
        ModelCache::new(backend, model)?
            .for_inspection(backend, model)
            .await
    }

    pub(crate) fn parse(bytes: &[u8], model: &str) -> Result<Self, InputError> {
        let config: Self = serde_json::from_slice(bytes).map_err(|error| {
            InputError::with_source("model", "invalid model configuration", Box::new(error))
        })?;
        if config.format != "inferno-vllm-0.31.0-model-config-v1" {
            return Err(InputError::new(
                "model",
                "unsupported model configuration version",
            ));
        }
        if !config.encoder.models.iter().any(|name| name == model) {
            return Err(InputError::new(
                "model",
                "configured model is not served by this deployment",
            ));
        }
        Ok(config)
    }
}

struct ModelCache {
    path: PathBuf,
}

impl ModelCache {
    fn new(backend: &Backend, model: &str) -> Result<Self, InputError> {
        let root = env::var_os("XDG_CACHE_HOME")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("HOME")
                    .filter(|path| !path.is_empty())
                    .map(|home| PathBuf::from(home).join(".cache"))
            })
            .ok_or_else(|| {
                InputError::new(
                    "model",
                    "a user cache location requires HOME or XDG_CACHE_HOME",
                )
            })?;
        Ok(Self::in_directory(
            root.join("inferno/models"),
            backend,
            model,
        ))
    }

    fn in_directory(directory: PathBuf, backend: &Backend, model: &str) -> Self {
        // The source URL includes the model and deployment identity; neither
        // model names nor credentials become filesystem path components.
        let digest = Sha256::digest(config_url(backend, model).as_str().as_bytes());
        Self {
            path: directory.join(format!(
                "{}.json",
                digest
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            )),
        }
    }

    async fn for_inspection(
        &self,
        backend: &Backend,
        model: &str,
    ) -> Result<ModelConfig, InputError> {
        match fs::read(&self.path) {
            Ok(bytes) => ModelConfig::parse(&bytes, model),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.discover(backend.client.get(config_url(backend, model)), model)
                    .await
            }
            Err(error) => Err(InputError::with_source(
                "model",
                "cannot read cached model configuration",
                Box::new(error),
            )),
        }
    }

    /// Refresh the inspection cache from an owned [`reqwest::RequestBuilder`].
    /// The request carries the backend identity without borrowing routing data.
    async fn discover(
        &self,
        request: reqwest::RequestBuilder,
        model: &str,
    ) -> Result<ModelConfig, InputError> {
        let bytes = request.send().await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| InputError::with_source(
                "model", "cannot discover model configuration; the backend must enable the inferno model configuration integration", Box::new(error),
            ))?.bytes().await?;
        let config = ModelConfig::parse(&bytes, model)?;
        self.write(&bytes).map_err(|error| {
            InputError::with_source("model", "cannot cache model configuration", Box::new(error))
        })?;
        Ok(config)
    }

    fn write(&self, bytes: &[u8]) -> io::Result<()> {
        let directory = self.path.parent().expect("model cache has a directory");
        fs::create_dir_all(directory)?;
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        file.write_all(bytes)?;
        file.persist(&self.path).map_err(|error| error.error)?;
        Ok(())
    }
}

fn config_url(backend: &Backend, model: &str) -> reqwest::Url {
    let mut url = backend.url("inferno/model-config", None);
    url.query_pairs_mut().append_pair("model", model);
    url
}

#[cfg(test)]
mod tests;

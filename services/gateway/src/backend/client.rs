//! Inference backend HTTP client and endpoint resolution.

use std::{error::Error, sync::Arc, time::Duration};

use reqwest::{
    Client, Url,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};

use super::{CacheIndex, tls::install_crypto_provider, vllm::CacheEvents};

/// One inference destination and its routing observations. Once admitted to
/// [`gateway::Pool`](crate::gateway::Pool), its cache and active count are accessed
/// under the pool's mutex.
#[derive(Debug)]
pub(crate) struct Backend {
    pub(crate) client: Client,
    endpoint: Url,
    pub(crate) cache_events: Option<Arc<CacheEvents>>,
    pub(crate) cache: Option<CacheIndex>,
    pub(crate) active_requests: usize,
}

impl Backend {
    /// Configure HTTP access to one destination, retaining the API prefix in
    /// [`reqwest::Url`]. The client applies authentication and the full-response
    /// deadline; [`Backend::discover_events`] attaches event metadata for serving.
    pub(crate) fn new(
        mut endpoint: Url,
        timeout: Duration,
        authorization: Option<HeaderValue>,
    ) -> Result<Self, Box<dyn Error>> {
        endpoint.set_path(&format!("{}/", endpoint.path().trim_end_matches('/')));
        install_crypto_provider();
        // Keep bundled trust roots rather than depending on the host's certificate store.
        let roots = webpki_root_certs::TLS_SERVER_ROOT_CERTS
            .iter()
            .map(|cert| reqwest::Certificate::from_der(cert.as_ref()))
            .collect::<Result<Vec<_>, _>>()?;
        let mut headers = HeaderMap::new();
        if let Some(value) = authorization {
            headers.insert(AUTHORIZATION, value);
        }
        Ok(Self {
            client: Client::builder()
                .default_headers(headers)
                .tls_certs_only(roots)
                .timeout(timeout)
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            endpoint,
            cache_events: None,
            cache: None,
            active_requests: 0,
        })
    }

    /// Address an operation beneath the configured API prefix. The caller's
    /// query replaces the endpoint query on the returned [`reqwest::Url`].
    pub(crate) fn url(&self, path: &str, query: Option<&str>) -> Url {
        let mut url = self.endpoint.clone();
        // Keep the configured API prefix; an absolute join would discard it.
        url.set_path(&format!("{}{}", self.endpoint.path(), path));
        url.set_query(query);
        url
    }
}

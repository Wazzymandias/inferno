//! Inference backend HTTP client and endpoint resolution.

use std::{error::Error, time::Duration};

use reqwest::{Client, Url};

use super::tls::install_crypto_provider;

#[derive(Debug)]
pub(crate) struct Pool {
    backends: Vec<Backend>,
}

impl Pool {
    pub(crate) const fn new() -> Self {
        Self {
            backends: Vec::new(),
        }
    }

    pub(crate) fn add(&mut self, endpoint: Url, timeout: Duration) -> Result<(), Box<dyn Error>> {
        self.backends.push(Backend::new(endpoint, timeout)?);
        Ok(())
    }

    pub(crate) fn first(&self) -> Option<&Backend> {
        self.backends.first()
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Backend {
    pub(crate) client: Client,
    endpoint: Url,
}

impl Backend {
    pub(crate) fn new(mut endpoint: Url, timeout: Duration) -> Result<Self, Box<dyn Error>> {
        endpoint.set_path(&format!("{}/", endpoint.path().trim_end_matches('/')));
        install_crypto_provider();
        // Keep bundled trust roots rather than depending on the host's certificate store.
        let roots = webpki_root_certs::TLS_SERVER_ROOT_CERTS
            .iter()
            .map(|cert| reqwest::Certificate::from_der(cert.as_ref()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            client: Client::builder()
                .tls_certs_only(roots)
                .timeout(timeout)
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            endpoint,
        })
    }

    pub(crate) fn url(&self, path: &str, query: Option<&str>) -> Url {
        let mut url = self.endpoint.clone();
        // Keep the configured API prefix; an absolute join would discard it.
        url.set_path(&format!("{}{}", self.endpoint.path(), path));
        url.set_query(query);
        url
    }
}

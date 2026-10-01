//! Inference backend connectivity and TLS setup.

mod client;
mod tls;

pub(crate) use client::Backend;

#[cfg(test)]
pub(crate) use tls::install_crypto_provider;

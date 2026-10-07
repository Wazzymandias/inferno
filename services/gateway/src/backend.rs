//! Inference backend connectivity and TLS setup.

mod cache;
mod client;
mod pool;
mod tls;

pub(crate) use cache::{CacheIndex, CacheUpdate};
pub(crate) use client::Backend;
pub(crate) use pool::{Pool, RequestLease};

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
pub(crate) use tls::install_crypto_provider;

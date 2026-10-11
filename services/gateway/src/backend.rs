//! Inference backend connectivity and TLS setup.

mod cache;
mod client;
mod tls;
pub(crate) mod vllm;

pub(crate) use cache::{CacheEvent, CacheGroup, CacheIndex, CacheUpdate};
pub(crate) use client::Backend;

#[cfg(test)]
pub(crate) use tls::install_crypto_provider;

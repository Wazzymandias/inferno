//! Prefix residency and active requests share one backend selection policy.

use std::{
    collections::HashSet,
    error::Error,
    io,
    sync::{Arc, Mutex},
    time::Duration,
};

use reqwest::{Method, RequestBuilder, Url, header::HeaderValue};
use tokio_util::sync::CancellationToken;

use crate::backend::{Backend, CacheEvent, vllm::CacheEvents};
use crate::inference::{ModelConfig, ModelInput};

#[cfg(test)]
pub(crate) mod tests;

impl crate::gateway::Gateway {
    /// Reserve a backend for dispatch. A prepared [`crate::inference::ModelInput`]
    /// contributes cached-prefix credit; `None` selects using current load alone.
    /// Keep the [`RequestLease`] until the response body finishes so that request
    /// completion, failure, or cancellation releases its reservation.
    pub(in crate::gateway) fn select(&self, input: Option<&ModelInput>) -> Option<RequestLease> {
        self.pool.as_ref()?.rank(input)
    }
}

/// Holds the backend observations used by [`super::Gateway::select`]. Cache
/// updates and request reservations share a mutex; no guard survives network I/O.
#[derive(Debug)]
pub(crate) struct Pool {
    model: String,
    pub(crate) model_config: ModelConfig,
    timeout: Duration,
    load_penalty: usize,
    backends: Arc<Mutex<Vec<Backend>>>,
}

impl Pool {
    /// Validate every destination against one [`crate::inference::ModelConfig`]
    /// and distinct cache-publisher identities before routing can begin. Retain
    /// that configuration so reconnects cannot accept incompatible hashes.
    /// Each active request costs `load_penalty` cached tokens when ranking.
    pub(crate) async fn connect(
        endpoints: Vec<Url>,
        model: String,
        timeout: Duration,
        authorization: Option<HeaderValue>,
        load_penalty: usize,
    ) -> Result<Self, Box<dyn Error>> {
        let mut backends = endpoints
            .into_iter()
            .map(|endpoint| Backend::new(endpoint, timeout, authorization.clone()))
            .collect::<Result<Vec<_>, _>>()?;
        let mut configs = futures_util::future::try_join_all(
            backends
                .iter()
                .map(|backend| ModelConfig::discover(backend, &model)),
        )
        .await?
        .into_iter();
        let config = configs
            .next()
            .ok_or("at least one inference endpoint is required")?;
        if configs.any(|other| other != config) {
            return Err(
                "replicas must share the same model preparation and prefix hash configuration"
                    .into(),
            );
        }
        futures_util::future::try_join_all(
            backends
                .iter_mut()
                .map(|backend| backend.discover_events(&model, config.block_size())),
        )
        .await?;
        let mut identities = HashSet::with_capacity(backends.len());
        if backends
            .iter()
            .any(|backend| !identities.insert(backend.instance_id()))
        {
            return Err("inference endpoints must identify distinct replicas".into());
        }
        Ok(Self {
            model,
            model_config: config,
            timeout,
            load_penalty,
            backends: Arc::new(Mutex::new(backends)),
        })
    }

    /// Observe each backend until shutdown. Each future owns its connection
    /// metadata, so waiting for events does not lock routing through [`Pool::rank`].
    pub(crate) async fn subscribe_events(
        &self,
        shutdown: CancellationToken,
    ) -> std::io::Result<()> {
        let subscriptions = {
            let backends = self.backends.lock().unwrap();
            backends
                .iter()
                .enumerate()
                .filter_map(|(id, backend)| {
                    let cache_events = Arc::clone(backend.cache_events.as_ref()?);
                    Some(self.observe_cache(id, cache_events, shutdown.clone()))
                })
                .collect::<Vec<_>>()
        };
        futures_util::future::try_join_all(subscriptions).await?;
        Ok(())
    }

    /// Keep one backend's cache evidence current across connection loss and
    /// shutdown. Rediscovery must agree with [`Pool::model_config`] before a
    /// replacement publisher can restore cache credit.
    async fn observe_cache(
        &self,
        id: usize,
        mut cache_events: Arc<CacheEvents>,
        shutdown: CancellationToken,
    ) -> io::Result<()> {
        let observe = async {
            loop {
                let _ = cache_events
                    .consume(self.model_config.block_size(), self.timeout, |event| {
                        self.update_cache(id, event)
                    })
                    .await;
                self.update_cache(id, CacheEvent::Unavailable)?;
                println!(
                    "{}",
                    serde_json::json!({"event":"kv_events.disconnected", "replica":id})
                );
                loop {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    let (events, model) = {
                        let backends = self.backends.lock().unwrap();
                        let backend = &backends[id];
                        (
                            CacheEvents::discover(
                                backend,
                                &self.model,
                                self.model_config.block_size(),
                            ),
                            ModelConfig::discover(backend, &self.model),
                        )
                    };
                    let (Ok(next), Ok(config)) = tokio::join!(events, model) else {
                        continue;
                    };
                    if config != self.model_config {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "replica model configuration changed; restart the gateway",
                        ));
                    }
                    cache_events = Arc::new(next);
                    self.backends.lock().unwrap()[id].cache_events =
                        Some(Arc::clone(&cache_events));
                    break;
                }
            }
        };
        let result = tokio::select! {
            _ = shutdown.cancelled() => Ok(()),
            result = observe => result,
        };
        self.update_cache(id, CacheEvent::Unavailable)?;
        result
    }

    #[cfg(test)]
    /// Add an HTTP destination for proxy fixtures. Serving uses [`Pool::connect`]
    /// to validate model agreement and cache events before admitting backends.
    pub(crate) fn add(
        &self,
        endpoint: Url,
        timeout: Duration,
        authorization: Option<HeaderValue>,
    ) -> Result<(), Box<dyn Error>> {
        self.backends
            .lock()
            .unwrap()
            .push(Backend::new(endpoint, timeout, authorization)?);
        Ok(())
    }

    /// Report whether this pool has any destination for [`super::Gateway::serve`].
    pub(super) fn is_empty(&self) -> bool {
        self.backends.lock().unwrap().is_empty()
    }

    /// Reserve the highest scoring backend while its observed cache and load
    /// remain stable. [`RequestLease::request`] builds requests after this lock
    /// has been released; dropping the lease releases the reservation.
    fn rank(&self, input: Option<&ModelInput>) -> Option<RequestLease> {
        let mut backends = self.backends.lock().unwrap();
        let (id, backend) = backends
            .iter_mut()
            .enumerate()
            .max_by_key(|(id, backend)| {
                let cached = input
                    .zip(backend.cache.as_ref())
                    .map_or(0, |(input, cache)| cache.cached_tokens(input));
                let active_requests = backend.active_requests;
                (
                    cached as i128 - active_requests as i128 * self.load_penalty as i128,
                    std::cmp::Reverse(active_requests),
                    std::cmp::Reverse(*id),
                )
            })?;
        backend.active_requests += 1;
        Some(RequestLease {
            backends: Arc::clone(&self.backends),
            id,
        })
    }

    /// Commit one [`backend::CacheEvent`](crate::backend::CacheEvent) under the
    /// same mutex used by [`Pool::rank`]. Invalid live batches withdraw all cache
    /// evidence for this backend without affecting request reservations.
    fn update_cache(&self, id: usize, event: CacheEvent) -> io::Result<()> {
        let replayed = matches!(&event, CacheEvent::Snapshot(_));
        let mut backends = self.backends.lock().unwrap();
        let cache = &mut backends[id].cache;
        let result = match event {
            CacheEvent::Unavailable => {
                *cache = None;
                Ok(())
            }
            CacheEvent::Snapshot(snapshot) => {
                *cache = Some(snapshot);
                Ok(())
            }
            CacheEvent::Batch(batch) => cache
                .as_mut()
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "cache replay must complete before live events",
                    )
                })
                .and_then(|cache| cache.apply(batch)),
        };
        if result.is_err() {
            *cache = None;
        }
        drop(backends);
        if replayed {
            println!(
                "{}",
                serde_json::json!({"event":"kv_events.subscribed", "replica":id})
            );
        }
        result
    }
}

/// A reservation on the backend selected by [`Pool::rank`]. It keeps the backend
/// records alive without holding their mutex. Retain it through the response
/// body's lifetime so completion, failure, or cancellation releases the load.
#[derive(Debug)]
pub(crate) struct RequestLease {
    backends: Arc<Mutex<Vec<Backend>>>,
    id: usize,
}

impl RequestLease {
    /// Build an owned HTTP request for the reserved backend. The returned
    /// [`reqwest::RequestBuilder`] holds no routing lock and preserves the
    /// backend's authentication, deadline, and API path prefix.
    pub(crate) fn request(
        &self,
        method: Method,
        path: &str,
        query: Option<&str>,
    ) -> RequestBuilder {
        let backends = self.backends.lock().unwrap();
        let backend = &backends[self.id];
        backend.client.request(method, backend.url(path, query))
    }
}

impl Drop for RequestLease {
    fn drop(&mut self) {
        self.backends.lock().unwrap()[self.id].active_requests -= 1;
    }
}

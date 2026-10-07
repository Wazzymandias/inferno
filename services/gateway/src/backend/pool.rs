//! Prefix residency and active requests share one backend selection policy.

use std::{
    collections::HashSet,
    error::Error,
    sync::{Arc, Mutex},
    time::Duration,
};

use reqwest::{Url, header::HeaderValue};

use super::{Backend, CacheIndex, CacheUpdate};
use crate::{
    events::Subscription,
    inference::{ModelConfig, ModelInput},
};

#[derive(Debug, Default)]
pub(crate) struct Pool {
    backends: Vec<Backend>,
    subscriptions: Vec<Subscription>,
    state: Arc<Mutex<Vec<Replica>>>,
    load_penalty: usize,
}

#[derive(Debug, Default)]
struct Replica {
    cache: Option<CacheIndex>,
    running: usize,
}

impl Pool {
    /// Discover and validate the complete replica set before making it routable.
    pub(crate) async fn connect(
        endpoints: Vec<Url>,
        model: &str,
        timeout: Duration,
        authorization: Option<HeaderValue>,
        load_penalty: usize,
    ) -> Result<(Self, ModelConfig), Box<dyn Error>> {
        let backends = endpoints
            .into_iter()
            .map(|endpoint| Backend::new(endpoint, timeout, authorization.clone()))
            .collect::<Result<Vec<_>, _>>()?;
        let mut configs = futures_util::future::try_join_all(
            backends
                .iter()
                .map(|backend| ModelConfig::discover(backend, model)),
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
        let expected = Arc::new(config.clone());
        let subscriptions = futures_util::future::try_join_all(backends.iter().map(|backend| {
            Subscription::discover(
                backend.clone(),
                model.to_owned(),
                Arc::clone(&expected),
                timeout,
            )
        }))
        .await?;
        let mut identities = HashSet::with_capacity(subscriptions.len());
        if subscriptions
            .iter()
            .any(|subscription| !identities.insert(subscription.instance_id()))
        {
            return Err("inference endpoints must identify distinct replicas".into());
        }
        let state = Arc::new(Mutex::new(
            (0..backends.len()).map(|_| Replica::default()).collect(),
        ));
        Ok((
            Self {
                backends,
                subscriptions,
                state,
                load_penalty,
            },
            config,
        ))
    }

    pub(crate) fn subscriptions(&self) -> &[Subscription] {
        &self.subscriptions
    }

    pub(crate) fn new() -> Self {
        Self {
            load_penalty: 256,
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(crate) const fn with_load_penalty(mut self, tokens: usize) -> Self {
        self.load_penalty = tokens;
        self
    }

    #[cfg(test)]
    pub(crate) fn add(
        &mut self,
        endpoint: Url,
        timeout: Duration,
        authorization: Option<HeaderValue>,
    ) -> Result<&Backend, Box<dyn Error>> {
        self.backends
            .push(Backend::new(endpoint, timeout, authorization)?);
        self.state.lock().unwrap().push(Replica::default());
        Ok(self.backends.last().expect("backend was just added"))
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.backends.is_empty()
    }

    /// Reserve the highest scoring replica atomically with observing its load.
    pub(crate) fn rank(&self, input: Option<&ModelInput>) -> Option<(&Backend, RequestLease)> {
        let mut replicas = self.state.lock().unwrap();
        let id = replicas
            .iter()
            .enumerate()
            .max_by_key(|(id, replica)| {
                let cached = input
                    .zip(replica.cache.as_ref())
                    .map_or(0, |(input, cache)| cache.cached_tokens(input));
                (
                    cached as i128 - replica.running as i128 * self.load_penalty as i128,
                    std::cmp::Reverse(replica.running),
                    std::cmp::Reverse(*id),
                )
            })?
            .0;
        replicas[id].running += 1;
        Some((
            &self.backends[id],
            RequestLease {
                state: Arc::clone(&self.state),
                id,
            },
        ))
    }

    pub(crate) fn replace_cache(&self, id: usize, cache: Option<CacheIndex>) {
        self.state.lock().unwrap()[id].cache = cache;
    }

    pub(crate) fn apply_batch(&self, id: usize, batch: Vec<CacheUpdate>) -> std::io::Result<()> {
        let mut state = self.state.lock().unwrap();
        let result = state[id]
            .cache
            .as_mut()
            .expect("replay completed before live events")
            .apply(batch);
        if result.is_err() {
            state[id].cache = None;
        }
        result
    }
}

/// Held until the upstream response is consumed, cancelled, or fails.
#[derive(Debug)]
pub(crate) struct RequestLease {
    state: Arc<Mutex<Vec<Replica>>>,
    id: usize,
}

impl Drop for RequestLease {
    fn drop(&mut self) {
        self.state.lock().unwrap()[self.id].running -= 1;
    }
}

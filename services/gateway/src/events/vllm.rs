//! vLLM discovery, ZMQ replay protocol, and `MessagePack` event adapter.

use futures_util::StreamExt;
use serde::Deserialize;
use std::{io, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use zeromq::{DealerSocket, Socket, SocketEvent, SocketRecv, SocketSend, SubSocket, ZmqMessage};

use super::invalid;
use crate::{
    backend::{Backend, CacheGroup, CacheIndex, CacheUpdate, Pool},
    inference::{BlockHash, ModelConfig},
};

#[derive(Deserialize)]
struct Discovery {
    instance_id: String,
    cache_groups: Vec<CacheGroup>,
    sources: Vec<Source>,
}

#[derive(Deserialize)]
struct Source {
    data_parallel_rank: u32,
    enable_kv_cache_events: bool,
    publisher: String,
    endpoint: String,
    replay_endpoint: String,
    topic: String,
}

pub(crate) struct Subscription {
    backend: Backend,
    model: String,
    config: Arc<ModelConfig>,
    timeout: Duration,
    discovery: Discovery,
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscription").finish_non_exhaustive()
    }
}

impl Subscription {
    pub(crate) async fn discover(
        backend: Backend,
        model: String,
        config: Arc<ModelConfig>,
        timeout: Duration,
    ) -> io::Result<Self> {
        let discovery = discover(&backend, &model, config.block_size()).await?;
        Ok(Self {
            backend,
            model,
            config,
            timeout,
            discovery,
        })
    }

    pub(crate) fn instance_id(&self) -> &str {
        &self.discovery.instance_id
    }

    pub(crate) async fn run(
        &self,
        pool: &Pool,
        id: usize,
        shutdown: CancellationToken,
    ) -> io::Result<()> {
        tokio::select! {
            _ = shutdown.cancelled() => { pool.replace_cache(id, None); Ok(()) },
            result = self.reconnect(pool, id) => result,
        }
    }

    async fn reconnect(&self, pool: &Pool, id: usize) -> io::Result<()> {
        let mut discovery = None;
        loop {
            let active = discovery.as_ref().unwrap_or(&self.discovery);
            let _ = self.consume(pool, id, active).await;
            pool.replace_cache(id, None);
            println!(
                "{}",
                serde_json::json!({"event":"kv_events.disconnected", "replica":id})
            );
            // Retry only after a failed connection. Idle publishers need no polling.
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let Ok(next) = discover(&self.backend, &self.model, self.config.block_size()).await
                else {
                    continue;
                };
                let Ok(config) = ModelConfig::discover(&self.backend, &self.model).await else {
                    continue;
                };
                if config != *self.config {
                    return Err(invalid(
                        "replica model configuration changed; restart the gateway",
                    ));
                }
                discovery = Some(next);
                break;
            }
        }
    }

    async fn consume(&self, pool: &Pool, id: usize, discovery: &Discovery) -> io::Result<()> {
        let source = &discovery.sources[0];
        let mut subscriber = SubSocket::new();
        let mut monitor = subscriber.monitor();
        let mut replay = DealerSocket::new();
        tokio::time::timeout(self.timeout, async {
            subscriber
                .subscribe(&source.topic)
                .await
                .map_err(connection_error)?;
            subscriber
                .connect(&source.endpoint)
                .await
                .map_err(connection_error)?;
            replay
                .connect(&source.replay_endpoint)
                .await
                .map_err(connection_error)
        })
        .await
        .map_err(|_| invalid("KV connection timed out"))??;
        let mut next = self.replay(pool, id, discovery, &mut replay).await?;
        println!(
            "{}",
            serde_json::json!({"event":"kv_events.subscribed", "replica":id})
        );
        loop {
            tokio::select! {
                biased;
                event = monitor.next() => {
                    if matches!(event, None | Some(SocketEvent::Disconnected(_) | SocketEvent::Closed)) {
                        return Err(invalid("KV publisher disconnected"));
                    }
                }
                message = subscriber.recv() => {
                    let (sequence, batch) = decode(message.map_err(connection_error)?, &source.topic, false)?
                        .ok_or_else(|| invalid("unexpected live replay terminator"))?;
                    if sequence < next { continue; } // Buffered PUB duplicates of replayed batches.
                    if sequence != next {
                        next = self.replay(pool, id, discovery, &mut replay).await?;
                        if sequence >= next { return Err(invalid("replay did not cover the missing event")); }
                        continue;
                    }
                    pool.apply_batch(id, batch)?;
                    next = sequence.checked_add(1).ok_or_else(|| invalid("KV sequence overflow"))?;
                }
            }
        }
    }

    async fn replay(
        &self,
        pool: &Pool,
        id: usize,
        discovery: &Discovery,
        socket: &mut DealerSocket,
    ) -> io::Result<u64> {
        pool.replace_cache(id, None);
        let recovery = async {
            let mut request = ZmqMessage::from(Vec::<u8>::new());
            request.push_back(0_u64.to_be_bytes().to_vec().into());
            socket.send(request).await.map_err(connection_error)?;
            let mut cache = CacheIndex::new(&discovery.cache_groups, self.config.block_size())?;
            let mut next = 0;
            while let Some((sequence, batch)) = decode(
                socket.recv().await.map_err(connection_error)?,
                &discovery.sources[0].topic,
                true,
            )? {
                if sequence < next {
                    return Err(invalid("replay sequence moved backwards"));
                }
                // A bounded replay buffer may have discarded earlier history.
                // Retain only residency established since the last missing batch.
                if sequence != next {
                    cache.clear();
                }
                cache.apply(batch)?;
                next = sequence
                    .checked_add(1)
                    .ok_or_else(|| invalid("KV sequence overflow"))?;
            }
            pool.replace_cache(id, Some(cache));
            Ok(next)
        };
        tokio::time::timeout(self.timeout, recovery)
            .await
            .map_err(|_| invalid("KV replay timed out"))?
    }
}

async fn discover(backend: &Backend, model: &str, hash_size: usize) -> io::Result<Discovery> {
    let mut url = backend.url("inferno/kv-events", None);
    url.query_pairs_mut().append_pair("model", model);
    let discovery: Discovery = backend
        .client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| invalid("KV discovery failed"))?
        .json()
        .await
        .map_err(|_| invalid("invalid KV discovery"))?;
    let [source] = discovery.sources.as_slice() else {
        return Err(invalid(
            "each backend must expose one data parallel replica",
        ));
    };
    if discovery.instance_id.is_empty()
        || source.data_parallel_rank != 0
        || !source.enable_kv_cache_events
        || source.publisher != "zmq"
    {
        return Err(invalid(
            "backend requires native ZMQ KV events for rank zero",
        ));
    }
    CacheIndex::new(&discovery.cache_groups, hash_size)?;
    for endpoint in [&source.endpoint, &source.replay_endpoint] {
        let endpoint: zeromq::Endpoint = endpoint
            .parse()
            .map_err(|_| invalid("invalid ZMQ endpoint"))?;
        if matches!(endpoint, zeromq::Endpoint::Tcp(_, 0)) {
            return Err(invalid("KV discovery must publish bound ports"));
        }
    }
    Ok(discovery)
}

fn decode(
    message: ZmqMessage,
    topic: &str,
    replay: bool,
) -> io::Result<Option<(u64, Vec<CacheUpdate>)>> {
    let frames = message.into_vec();
    let frames = if replay {
        if frames.first().is_none_or(|frame| !frame.is_empty()) {
            return Err(invalid("invalid replay envelope"));
        }
        &frames[1..]
    } else {
        &frames[..]
    };
    let [received_topic, sequence, payload] = frames else {
        return Err(invalid("KV event requires three frames"));
    };
    let sequence = u64::from_be_bytes(
        sequence
            .as_ref()
            .try_into()
            .map_err(|_| invalid("KV sequence must be eight bytes"))?,
    );
    if replay && sequence == u64::MAX && received_topic.is_empty() && payload.is_empty() {
        return Ok(None);
    }
    if received_topic.as_ref() != topic.as_bytes() {
        return Err(invalid("KV topic mismatch"));
    }
    let KVEventBatch(timestamp, events, rank) =
        rmp_serde::from_slice(payload).map_err(|_| invalid("invalid KV event payload"))?;
    if !timestamp.is_finite() || rank != Some(0) {
        return Err(invalid(
            "KV batch must belong to the discovered data parallel rank",
        ));
    }
    Ok(Some((
        sequence,
        events
            .into_iter()
            .filter_map(|event| event.into_update().transpose())
            .collect::<io::Result<Vec<_>>>()?,
    )))
}

fn connection_error(_: zeromq::ZmqError) -> io::Error {
    invalid("KV transport failed")
}

// vLLM uses an array for the batch and tagged maps for individual events.
#[derive(Deserialize)]
struct KVEventBatch(f64, Vec<KVEvent>, Option<u32>);

#[derive(Deserialize)]
struct Hash(#[serde(with = "serde_bytes")] BlockHash);

#[derive(Deserialize)]
#[serde(tag = "type")]
enum KVEvent {
    BlockStored {
        block_hashes: Vec<Hash>,
        block_size: usize,
        group_idx: Option<usize>,
        medium: Option<String>,
        locality: Option<String>,
    },
    BlockRemoved {
        block_hashes: Vec<Hash>,
        group_idx: Option<usize>,
        medium: Option<String>,
        locality: Option<String>,
    },
    AllBlocksCleared,
}

impl KVEvent {
    fn into_update(self) -> io::Result<Option<CacheUpdate>> {
        match self {
            Self::AllBlocksCleared => Ok(Some(CacheUpdate::Clear)),
            Self::BlockStored {
                block_hashes,
                block_size,
                group_idx,
                medium,
                locality,
            } => {
                if !local_gpu(medium.as_deref(), locality.as_deref()) {
                    return Ok(None);
                }
                Ok(Some(CacheUpdate::Store {
                    hashes: block_hashes.into_iter().map(|hash| hash.0).collect(),
                    block_size,
                    group: group_idx
                        .ok_or_else(|| invalid("local GPU event requires a cache group"))?,
                }))
            }
            Self::BlockRemoved {
                block_hashes,
                group_idx,
                medium,
                locality,
            } => {
                if !local_gpu(medium.as_deref(), locality.as_deref()) {
                    return Ok(None);
                }
                Ok(Some(CacheUpdate::Remove {
                    hashes: block_hashes.into_iter().map(|hash| hash.0).collect(),
                    group: group_idx
                        .ok_or_else(|| invalid("local GPU event requires a cache group"))?,
                }))
            }
        }
    }
}

fn local_gpu(medium: Option<&str>, locality: Option<&str>) -> bool {
    medium == Some("GPU") && matches!(locality, None | Some("LOCAL"))
}

#[cfg(test)]
mod tests;

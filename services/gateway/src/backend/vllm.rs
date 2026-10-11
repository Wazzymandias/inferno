//! vLLM discovery, ZMQ replay protocol, and `MessagePack` event adapter.

use futures_util::StreamExt;
use serde::Deserialize;
use std::{io, sync::Arc, time::Duration};
use zeromq::{DealerSocket, Socket, SocketEvent, SocketRecv, SocketSend, SubSocket, ZmqMessage};

use crate::{
    backend::{Backend, CacheEvent, CacheGroup, CacheIndex, CacheUpdate},
    inference::BlockHash,
};

/// The backend identity, cache layout, and addresses needed to observe its KV
/// cache. [`CacheEvents::consume`] delivers ordered cache evidence from the
/// publisher; a connection failure ends observation and requires rediscovery.
#[derive(Deserialize)]
pub(crate) struct CacheEvents {
    instance_id: String,
    cache_groups: Vec<CacheGroup>,
    sources: Vec<Source>,
}

impl std::fmt::Debug for CacheEvents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CacheEvents").finish_non_exhaustive()
    }
}

/// One publisher's live and replay addresses. Admission requires enabled events
/// for a single data-parallel rank so cache credit describes this HTTP backend.
#[derive(Deserialize)]
struct Source {
    data_parallel_rank: u32,
    enable_kv_cache_events: bool,
    publisher: String,
    endpoint: String,
    replay_endpoint: String,
    topic: String,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

impl Backend {
    /// Validate the publisher's identity, cache layout, and connection addresses
    /// before this backend can be admitted for serving.
    pub(crate) async fn discover_events(
        &mut self,
        model: &str,
        block_size: usize,
    ) -> io::Result<()> {
        let cache_events = CacheEvents::discover(self, model, block_size).await?;
        self.cache_events = Some(Arc::new(cache_events));
        Ok(())
    }

    /// Identify the cache publisher so duplicate destinations can be rejected.
    /// `None` means this backend has not completed event discovery.
    pub(crate) fn instance_id(&self) -> Option<&str> {
        self.cache_events
            .as_ref()
            .map(|cache_events| cache_events.instance_id.as_str())
    }
}

impl CacheEvents {
    /// Observe one connection until it fails. Deliver [`CacheEvent::Snapshot`]
    /// after a complete replay and [`CacheEvent::Batch`] for subsequent live
    /// events. Gaps withdraw evidence before replay; duplicates are ignored.
    ///
    /// The synchronous callback must apply each change before returning. The
    /// caller withdraws cache evidence when this future fails or is cancelled.
    pub(crate) async fn consume(
        &self,
        block_size: usize,
        timeout: Duration,
        mut update_cache: impl FnMut(CacheEvent) -> io::Result<()>,
    ) -> io::Result<()> {
        update_cache(CacheEvent::Unavailable)?;
        let source = &self.sources[0];
        let mut subscriber = SubSocket::new();
        let mut monitor = subscriber.monitor();
        let mut replay = DealerSocket::new();
        tokio::time::timeout(timeout, async {
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
        let (cache, mut next) = self.replay_cache(&mut replay, block_size, timeout).await?;
        update_cache(CacheEvent::Snapshot(cache))?;
        loop {
            tokio::select! {
                biased;
                event = monitor.next() => {
                    if matches!(event, None | Some(SocketEvent::Disconnected(_) | SocketEvent::Closed)) {
                        return Err(invalid("KV publisher disconnected"));
                    }
                }
                message = subscriber.recv() => {
                    let (sequence, batch) = self.decode(message.map_err(connection_error)?, false)?
                        .ok_or_else(|| invalid("unexpected live replay terminator"))?;
                    if sequence < next { continue; } // Buffered PUB duplicates of replayed batches.
                    if sequence != next {
                        update_cache(CacheEvent::Unavailable)?;
                        let (cache, recovered_next) = self.replay_cache(&mut replay, block_size, timeout).await?;
                        if sequence >= recovered_next { return Err(invalid("replay did not cover the missing event")); }
                        next = recovered_next;
                        update_cache(CacheEvent::Snapshot(cache))?;
                        continue;
                    }
                    let following = sequence.checked_add(1).ok_or_else(|| invalid("KV sequence overflow"))?;
                    update_cache(CacheEvent::Batch(batch))?;
                    next = following;
                }
            }
        }
    }

    /// Return cache evidence from a completed replay. Truncated history retains
    /// only blocks established after the last missing batch.
    async fn replay_cache(
        &self,
        socket: &mut DealerSocket,
        block_size: usize,
        timeout: Duration,
    ) -> io::Result<(CacheIndex, u64)> {
        let recovery = async {
            let mut request = ZmqMessage::from(Vec::<u8>::new());
            request.push_back(0_u64.to_be_bytes().to_vec().into());
            socket.send(request).await.map_err(connection_error)?;
            let mut cache = CacheIndex::new(&self.cache_groups, block_size)?;
            let mut next = 0;
            while let Some((sequence, batch)) =
                self.decode(socket.recv().await.map_err(connection_error)?, true)?
            {
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
            Ok((cache, next))
        };
        tokio::time::timeout(timeout, recovery)
            .await
            .map_err(|_| invalid("KV replay timed out"))?
    }

    /// Validate one enabled publisher against the model's hash block size.
    /// The returned future owns its HTTP request, allowing callers to release
    /// the lock protecting [`backend::Backend`](crate::backend::Backend) before IO.
    pub(crate) fn discover(
        backend: &Backend,
        model: &str,
        hash_size: usize,
    ) -> impl Future<Output = io::Result<Self>> + Send + use<> {
        let mut url = backend.url("inferno/kv-events", None);
        url.query_pairs_mut().append_pair("model", model);
        let request = backend.client.get(url);
        async move {
            let cache_events: Self = request
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|_| invalid("KV discovery failed"))?
                .json()
                .await
                .map_err(|_| invalid("invalid KV discovery"))?;
            let [source] = cache_events.sources.as_slice() else {
                return Err(invalid(
                    "each backend must expose one data parallel replica",
                ));
            };
            if cache_events.instance_id.is_empty()
                || source.data_parallel_rank != 0
                || !source.enable_kv_cache_events
                || source.publisher != "zmq"
            {
                return Err(invalid(
                    "backend requires native ZMQ KV events for rank zero",
                ));
            }
            CacheIndex::new(&cache_events.cache_groups, hash_size)?;
            for endpoint in [&source.endpoint, &source.replay_endpoint] {
                let endpoint: zeromq::Endpoint = endpoint
                    .parse()
                    .map_err(|_| invalid("invalid ZMQ endpoint"))?;
                if matches!(endpoint, zeromq::Endpoint::Tcp(_, 0)) {
                    return Err(invalid("KV discovery must publish bound ports"));
                }
            }
            Ok(cache_events)
        }
    }
    /// Reject malformed frames, hashes, topics, and ranks before they can become
    /// cache evidence. The replay terminator is represented by `None`.
    fn decode(
        &self,
        message: ZmqMessage,
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
        if received_topic.as_ref() != self.sources[0].topic.as_bytes() {
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

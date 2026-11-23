//! Profiling data read from the Flutter VM service.

use std::collections::VecDeque;

use floem::reactive::{RwSignal, Scope, SignalUpdate};
use serde_json::{Value, json};

const SAMPLE_LIMIT: usize = 120;
const REQUEST_LIMIT: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sample {
    Memory,
    Network,
    Recording(bool),
    ClearNetwork,
}

impl Sample {
    pub(super) fn rpc(
        self,
        isolate: &str,
        since: Option<u64>,
    ) -> (&'static str, Value) {
        match self {
            Self::Memory => ("getMemoryUsage", json!({"isolateId": isolate})),
            Self::Network => {
                let mut params = json!({"isolateId": isolate});
                if let Some(since) = since {
                    params["updatedSince"] = json!(since);
                }
                ("ext.dart.io.getHttpProfile", params)
            }
            Self::Recording(enabled) => (
                "ext.dart.io.httpEnableTimelineLogging",
                json!({"isolateId": isolate, "enabled": enabled}),
            ),
            Self::ClearNetwork => (
                "ext.dart.io.clearHttpProfile",
                json!({"isolateId": isolate}),
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub number: u64,
    pub build_ms: f64,
    pub raster_ms: f64,
    pub elapsed_ms: f64,
}

impl Frame {
    pub(super) fn from_event(message: &Value, isolate: &str) -> Option<Self> {
        let event = &message["params"]["event"];
        if message["method"] != "streamNotify"
            || message["params"]["streamId"] != "Extension"
            || event["extensionKind"] != "Flutter.Frame"
            || event["isolate"]["id"].as_str() != Some(isolate)
        {
            return None;
        }
        let timing = &event["extensionData"];
        let ms = |key: &str| timing[key].as_u64().map(|us| us as f64 / 1000.0);
        Some(Self {
            number: timing["number"].as_u64()?,
            build_ms: ms("build")?,
            raster_ms: ms("raster")?,
            elapsed_ms: ms("elapsed")?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Memory {
    pub heap: u64,
    pub capacity: u64,
    pub external: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Request {
    pub id: String,
    pub method: String,
    pub url: String,
    pub status: Option<u64>,
    pub start: u64,
    pub duration_us: Option<u64>,
}

impl Request {
    fn parse(value: &Value) -> Option<Self> {
        let start = value["startTime"].as_u64()?;
        let mut url = url::Url::parse(value["uri"].as_str()?).ok()?;
        let _ = url.set_username("");
        let _ = url.set_password(None);
        if url.query().is_some() {
            url.set_query(Some("…"));
        }
        url.set_fragment(None);
        Some(Self {
            id: value["id"].as_str()?.into(),
            method: value["method"].as_str()?.into(),
            url: url.into(),
            status: value["response"]["statusCode"].as_u64(),
            start,
            duration_us: value["endTime"]
                .as_u64()
                .and_then(|end| end.checked_sub(start)),
        })
    }
}

#[derive(Clone, Debug)]
pub enum PerformanceUpdate {
    Frames(Vec<Frame>),
    Reply(SampleReply),
}

#[derive(Clone, Debug)]
pub struct SampleReply {
    pub(crate) sample: Sample,
    pub(crate) result: Result<Value, String>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PerformanceData {
    pub frames: RwSignal<VecDeque<Frame>>,
    pub memory: RwSignal<VecDeque<Memory>>,
    pub network: RwSignal<Vec<Request>>,
    pub recording: RwSignal<bool>,
    pub network_busy: RwSignal<bool>,
    pub network_refresh: RwSignal<bool>,
    pub paused: RwSignal<bool>,
    pub memory_error: RwSignal<Option<String>>,
    pub network_error: RwSignal<Option<String>>,
}

impl PerformanceData {
    pub fn new(cx: Scope) -> Self {
        Self {
            frames: cx.create_rw_signal(VecDeque::new()),
            memory: cx.create_rw_signal(VecDeque::new()),
            network: cx.create_rw_signal(Vec::new()),
            recording: cx.create_rw_signal(false),
            network_busy: cx.create_rw_signal(false),
            network_refresh: cx.create_rw_signal(false),
            paused: cx.create_rw_signal(false),
            memory_error: cx.create_rw_signal(None),
            network_error: cx.create_rw_signal(None),
        }
    }

    pub fn reset(self) {
        floem::reactive::batch(|| {
            self.frames.set(VecDeque::new());
            self.memory.set(VecDeque::new());
            self.network.set(Vec::new());
            self.end_session();
            self.paused.set(false);
            self.memory_error.set(None);
            self.network_error.set(None);
        });
    }

    pub fn end_session(self) {
        self.recording.set(false);
        self.network_busy.set(false);
        self.network_refresh.set(false);
    }

    pub fn apply(self, update: &PerformanceUpdate) {
        use floem::reactive::SignalGet;
        let PerformanceUpdate::Reply(reply) = update else {
            if let PerformanceUpdate::Frames(frames) = update
                && !self.paused.get_untracked()
            {
                self.frames
                    .update(|history| append(history, frames.iter().copied()));
            }
            return;
        };
        if matches!(reply.sample, Sample::Recording(_) | Sample::ClearNetwork) {
            self.network_busy.set(false);
        }
        if reply.sample == Sample::Network {
            self.network_refresh.set(false);
        }
        let error = if reply.sample == Sample::Memory {
            self.memory_error
        } else {
            self.network_error
        };
        match &reply.result {
            Err(message) => error.set(Some(message.clone())),
            Ok(value) => {
                error.set(None);
                match reply.sample {
                    Sample::Memory => {
                        let memory = (|| {
                            Some(Memory {
                                heap: value["heapUsage"].as_u64()?,
                                capacity: value["heapCapacity"].as_u64()?,
                                external: value["externalUsage"].as_u64()?,
                            })
                        })();
                        if let Some(memory) = memory {
                            self.memory.update(|history| append(history, [memory]));
                        }
                    }
                    Sample::Network => {
                        if let Some(rows) = value["requests"].as_array() {
                            self.network.update(|history| {
                                for request in rows.iter().filter_map(Request::parse)
                                {
                                    if let Some(existing) = history
                                        .iter_mut()
                                        .find(|row| row.id == request.id)
                                    {
                                        *existing = request;
                                    } else {
                                        history.push(request);
                                    }
                                }
                                history
                                    .sort_unstable_by(|a, b| b.start.cmp(&a.start));
                                history.truncate(REQUEST_LIMIT);
                            });
                        }
                    }
                    Sample::Recording(enabled) => {
                        let enabled = value["enabled"].as_bool().unwrap_or(enabled);
                        self.recording.set(enabled);
                        self.network_refresh.set(!enabled);
                    }
                    Sample::ClearNetwork => self.network.set(Vec::new()),
                }
            }
        }
    }
}

fn append<T>(history: &mut VecDeque<T>, samples: impl IntoIterator<Item = T>) {
    history.extend(samples);
    history.drain(..history.len().saturating_sub(SAMPLE_LIMIT));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_flutter_timing_and_only_accepts_the_current_isolate() {
        let event = json!({"method":"streamNotify","params":{"streamId":"Extension","event":{
            "extensionKind":"Flutter.Frame","isolate":{"id":"isolates/1"},
            "extensionData":{"number":4,"build":3800,"raster":1900,"elapsed":7700}
        }}});
        let frame = Frame::from_event(&event, "isolates/1").unwrap();
        assert_eq!(
            (
                frame.number,
                frame.build_ms,
                frame.raster_ms,
                frame.elapsed_ms
            ),
            (4, 3.8, 1.9, 7.7)
        );
        assert!(Frame::from_event(&event, "isolates/2").is_none());
    }

    #[test]
    fn bounds_history_and_redacts_request_credentials() {
        let mut series = VecDeque::new();
        append(&mut series, 0..200);
        assert_eq!(series.len(), SAMPLE_LIMIT);
        assert_eq!(series.back(), Some(&199));
        let row = Request::parse(&json!({"id":"1","method":"GET","uri":"https://user:secret@example.com/api?token=hidden",
            "startTime": 1000,"endTime": 2600,"response":{"statusCode":200}})).unwrap();
        assert_eq!(row.duration_us, Some(1600));
        assert_eq!(row.status, Some(200));
        for secret in ["user", "secret", "token", "hidden"] {
            assert!(!row.url.contains(secret));
        }
    }
}

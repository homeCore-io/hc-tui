//! Server-Sent Events consumer for streaming plugin actions.
//!
//! `GET /api/v1/plugins/{id}/command/{request_id}/stream` sends
//! `event: stream` messages whose `data:` is JSON with at least `stage`
//! and `request_id`. Terminal stages are `complete`, `error`,
//! `canceled` and `timeout`; the server closes after one.
//!
//! Bytes are buffered and split on newlines before any UTF-8 decoding,
//! so a multi-byte character split across two network chunks is never
//! mangled.

use super::http::HomeCoreClient;
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;

#[derive(Debug)]
pub enum StreamKind {
    Connected,
    Event(Value),
    Closed,
    Error(String),
}

/// A stream message, tagged with the request it belongs to so stale
/// messages from an earlier run are ignored.
#[derive(Debug)]
pub struct StreamMsg {
    pub request_id: String,
    pub kind: StreamKind,
}

pub fn is_terminal_stage(stage: &str) -> bool {
    matches!(stage, "complete" | "error" | "canceled" | "timeout")
}

/// Incremental SSE parser: feed it raw bytes, get complete event payloads
/// (the joined `data:` lines) back.
#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
    data_lines: Vec<String>,
}

impl SseParser {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(pos) = self.buf.iter().position(|b| *b == b'\n') {
            let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
            line.pop(); // the '\n'
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = String::from_utf8_lossy(&line);
            if line.is_empty() {
                if !self.data_lines.is_empty() {
                    out.push(self.data_lines.join("\n"));
                    self.data_lines.clear();
                }
                continue;
            }
            if line.starts_with(':') {
                continue; // comment / keep-alive
            }
            let (field, value) = match line.split_once(':') {
                Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
                None => (line.as_ref(), ""),
            };
            if field == "data" {
                self.data_lines.push(value.to_string());
            }
        }
        out
    }
}

/// Open the stream and pump events into `tx` until a terminal stage, the
/// end of the stream, an error, or the task being aborted.
pub fn spawn_streaming_action<T: From<StreamMsg> + Send + 'static>(
    client: HomeCoreClient,
    plugin_id: String,
    request_id: String,
    tx: UnboundedSender<T>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let send = |kind: StreamKind| {
            tx.send(T::from(StreamMsg {
                request_id: request_id.clone(),
                kind,
            }))
            .is_ok()
        };
        let mut resp = match client.open_action_stream(&plugin_id, &request_id).await {
            Ok(r) => r,
            Err(e) => {
                send(StreamKind::Error(e.to_string()));
                return;
            }
        };
        if !send(StreamKind::Connected) {
            return;
        }
        let mut parser = SseParser::default();
        loop {
            match resp.chunk().await {
                Ok(Some(bytes)) => {
                    for data in parser.push(&bytes) {
                        let Ok(value) = serde_json::from_str::<Value>(&data) else {
                            continue;
                        };
                        let terminal = value
                            .get("stage")
                            .and_then(Value::as_str)
                            .map(is_terminal_stage)
                            .unwrap_or(false);
                        if !send(StreamKind::Event(value)) || terminal {
                            send(StreamKind::Closed);
                            return;
                        }
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    send(StreamKind::Error(format!(
                        "read failed: {}",
                        e.without_url()
                    )));
                    return;
                }
            }
        }
        send(StreamKind::Closed);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_events() {
        let mut p = SseParser::default();
        let out = p.push(b"event: stream\ndata: {\"stage\":\"progress\"}\n\n");
        assert_eq!(out, vec![r#"{"stage":"progress"}"#.to_string()]);
    }

    #[test]
    fn handles_crlf_comments_and_multi_line_data() {
        let mut p = SseParser::default();
        let out = p.push(b": keep-alive\r\ndata: a\r\ndata: b\r\n\r\n");
        assert_eq!(out, vec!["a\nb".to_string()]);
    }

    #[test]
    fn events_split_across_chunks() {
        let mut p = SseParser::default();
        assert!(p.push(b"data: {\"x\":").is_empty());
        assert!(p.push(b"1}\n").is_empty());
        assert_eq!(p.push(b"\n"), vec![r#"{"x":1}"#.to_string()]);
    }

    #[test]
    fn utf8_split_across_chunks_survives() {
        // "é" is 0xC3 0xA9. Split it between two chunks.
        let full = "data: {\"m\":\"caf\u{e9}\"}\n\n".as_bytes().to_vec();
        let cut = full.iter().position(|b| *b == 0xC3).unwrap() + 1;
        let mut p = SseParser::default();
        assert!(p.push(&full[..cut]).is_empty());
        let out = p.push(&full[cut..]);
        assert_eq!(out, vec!["{\"m\":\"caf\u{e9}\"}".to_string()]);
        let v: Value = serde_json::from_str(&out[0]).unwrap();
        assert_eq!(v["m"], "caf\u{e9}");
    }

    #[test]
    fn terminal_stages() {
        for s in ["complete", "error", "canceled", "timeout"] {
            assert!(is_terminal_stage(s));
        }
        assert!(!is_terminal_stage("progress"));
    }
}

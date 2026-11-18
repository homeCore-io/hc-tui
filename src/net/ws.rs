//! WebSocket consumers for the live event stream and the log stream.
//!
//! Core's WebSocket routes authenticate with `?token=` only, so the URL
//! carries the token. Every connect builds the URL from the client's
//! current token (so a refreshed token is used after a reconnect), and
//! no error string sent to the screen ever contains that URL.

use super::http::HomeCoreClient;
use crate::model::LogLine;
use futures_util::StreamExt;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

const BACKOFF_START: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(30);
/// A connection must stay up this long before the backoff resets, so a
/// server that accepts and drops at once is not hammered.
const STABLE_AFTER: Duration = Duration::from_secs(10);
/// Core pings every 30s; this much silence means the link is dead.
const READ_TIMEOUT: Duration = Duration::from_secs(75);

/// Next frame, or `None` when the stream ended or went silent too long.
async fn next_frame<S>(
    read: &mut S,
) -> Option<Result<Message, tokio_tungstenite::tungstenite::Error>>
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    match tokio::time::timeout(READ_TIMEOUT, read.next()).await {
        Ok(v) => v,
        Err(_) => Some(Err(tokio_tungstenite::tungstenite::Error::Io(
            std::io::Error::new(std::io::ErrorKind::TimedOut, "no frames"),
        ))),
    }
}

#[derive(Debug)]
pub enum WsMsg {
    /// Connected. `attempt` is 0 for the first connection of this task
    /// and counts up on every reconnect.
    Connected {
        attempt: u32,
    },
    Disconnected(String),
    Event(Value),
    /// The server dropped `missed` events because the client fell behind.
    Lagged {
        missed: u64,
    },
}

#[derive(Debug)]
pub enum LogMsg {
    Connected,
    Disconnected(String),
    Line(LogLine),
}

/// Exponential backoff from 1s, doubling, capped at 30s.
#[derive(Debug, Clone)]
pub struct Backoff {
    next: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            next: BACKOFF_START,
        }
    }
}

impl Backoff {
    pub fn next_delay(&mut self) -> Duration {
        let d = self.next;
        self.next = (self.next * 2).min(BACKOFF_MAX);
        d
    }

    pub fn reset(&mut self) {
        self.next = BACKOFF_START;
    }
}

/// Describe a connection error without echoing the URL (which holds the
/// token).
fn describe_ws_error(err: &tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::Error as E;
    match err {
        E::Http(resp) => format!("server answered {}", resp.status()),
        E::Url(_) => "bad stream address".to_string(),
        E::Io(e) => format!("network error: {}", e.kind()),
        E::Tls(_) => "TLS error".to_string(),
        E::ConnectionClosed | E::AlreadyClosed => "connection closed".to_string(),
        E::Protocol(p) => format!("protocol error: {p}"),
        other => {
            let s = other.to_string();
            if s.contains("token=") {
                "connection error".to_string()
            } else {
                s
            }
        }
    }
}

/// Sends messages converted into the caller's channel type.
struct Forward<T>(UnboundedSender<T>);

impl<T> Forward<T> {
    fn send<M: Into<T>>(&self, m: M) -> Result<(), ()> {
        self.0.send(m.into()).map_err(|_| ())
    }
}

fn with_token(url: &str, token: &str, extra: &[(&str, &str)]) -> String {
    let mut out = format!("{url}?token={}", urlencoding::encode(token));
    for (k, v) in extra {
        if !v.is_empty() {
            out.push_str(&format!("&{k}={}", urlencoding::encode(v)));
        }
    }
    out
}

/// Interpret one text frame from `/events/stream`.
pub fn classify_event_frame(text: &str) -> Option<WsMsg> {
    let value: Value = serde_json::from_str(text).ok()?;
    if value.get("type").and_then(Value::as_str) == Some("lagged") {
        let missed = value.get("missed").and_then(Value::as_u64).unwrap_or(0);
        return Some(WsMsg::Lagged { missed });
    }
    Some(WsMsg::Event(value))
}

/// Run the live event stream until the task is aborted. Reconnects with
/// backoff; sends `Connected` on each successful connect.
pub fn spawn_events_stream<T: From<WsMsg> + Send + 'static>(
    client: HomeCoreClient,
    tx: UnboundedSender<T>,
) -> JoinHandle<()> {
    let tx = Forward(tx);
    tokio::spawn(async move {
        let mut backoff = Backoff::default();
        let mut attempt = 0u32;
        loop {
            let Some(token) = client.token() else {
                let _ = tx.send(WsMsg::Disconnected("not signed in".into()));
                tokio::time::sleep(backoff.next_delay()).await;
                continue;
            };
            let url = with_token(&client.ws_url("/events/stream"), &token, &[]);
            let reason = match connect_async(url.as_str()).await {
                Ok((stream, _)) => {
                    let connected_at = std::time::Instant::now();
                    if tx.send(WsMsg::Connected { attempt }).is_err() {
                        return;
                    }
                    attempt += 1;
                    let (_, mut read) = stream.split();
                    let mut reason = "connection closed".to_string();
                    while let Some(msg) = next_frame(&mut read).await {
                        match msg {
                            Ok(Message::Text(text)) => {
                                if let Some(m) = classify_event_frame(&text)
                                    && tx.send(m).is_err()
                                {
                                    return;
                                }
                            }
                            Ok(Message::Close(frame)) => {
                                reason = frame
                                    .map(|f| format!("closed: {}", f.reason))
                                    .unwrap_or_else(|| "closed".to_string());
                                break;
                            }
                            Ok(_) => {}
                            Err(err) => {
                                reason = format!("stream error: {}", describe_ws_error(&err));
                                break;
                            }
                        }
                    }
                    if connected_at.elapsed() >= STABLE_AFTER {
                        backoff.reset();
                    }
                    reason
                }
                Err(err) => format!("connect failed: {}", describe_ws_error(&err)),
            };
            if tx.send(WsMsg::Disconnected(reason)).is_err() {
                return;
            }
            tokio::time::sleep(backoff.next_delay()).await;
        }
    })
}

/// Parameters of the log stream; a change reconnects.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LogParams {
    /// `error`, `warn`, `info`, `debug` or `trace`.
    pub level: String,
    /// Prefix match on the log target.
    pub target: String,
    /// Exact plugin id.
    pub plugin_id: String,
}

/// Run `/logs/stream` until aborted.
pub fn spawn_log_stream<T: From<LogMsg> + Send + 'static>(
    client: HomeCoreClient,
    params: LogParams,
    tx: UnboundedSender<T>,
) -> JoinHandle<()> {
    let tx = Forward(tx);
    tokio::spawn(async move {
        let mut backoff = Backoff::default();
        loop {
            let Some(token) = client.token() else {
                let _ = tx.send(LogMsg::Disconnected("not signed in".into()));
                tokio::time::sleep(backoff.next_delay()).await;
                continue;
            };
            let url = with_token(
                &client.ws_url("/logs/stream"),
                &token,
                &[
                    ("level", params.level.as_str()),
                    ("target", params.target.as_str()),
                    ("plugin_id", params.plugin_id.as_str()),
                ],
            );
            let reason = match connect_async(url.as_str()).await {
                Ok((stream, _)) => {
                    let connected_at = std::time::Instant::now();
                    if tx.send(LogMsg::Connected).is_err() {
                        return;
                    }
                    let (_, mut read) = stream.split();
                    let mut reason = "connection closed".to_string();
                    while let Some(msg) = next_frame(&mut read).await {
                        match msg {
                            Ok(Message::Text(text)) => {
                                if let Ok(line) = serde_json::from_str::<LogLine>(&text)
                                    && tx.send(LogMsg::Line(line)).is_err()
                                {
                                    return;
                                }
                            }
                            Ok(Message::Close(_)) => break,
                            Ok(_) => {}
                            Err(err) => {
                                reason = format!("stream error: {}", describe_ws_error(&err));
                                break;
                            }
                        }
                    }
                    if connected_at.elapsed() >= STABLE_AFTER {
                        backoff.reset();
                    }
                    reason
                }
                Err(err) => format!("connect failed: {}", describe_ws_error(&err)),
            };
            if tx.send(LogMsg::Disconnected(reason)).is_err() {
                return;
            }
            tokio::time::sleep(backoff.next_delay()).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_to_a_cap() {
        let mut b = Backoff::default();
        let secs: Vec<u64> = (0..7).map(|_| b.next_delay().as_secs()).collect();
        assert_eq!(secs, vec![1, 2, 4, 8, 16, 30, 30]);
        b.reset();
        assert_eq!(b.next_delay().as_secs(), 1);
    }

    #[test]
    fn lagged_frames_are_recognised() {
        match classify_event_frame(r#"{"type":"lagged","missed":12}"#) {
            Some(WsMsg::Lagged { missed }) => assert_eq!(missed, 12),
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(
            classify_event_frame(r#"{"type":"device_added","device":{}}"#),
            Some(WsMsg::Event(_))
        ));
        assert!(classify_event_frame("not json").is_none());
    }

    #[test]
    fn log_urls_carry_only_set_params() {
        let url = with_token(
            "ws://h/api/v1/logs/stream",
            "t k",
            &[("level", "warn"), ("target", ""), ("plugin_id", "plugin.x")],
        );
        assert_eq!(
            url,
            "ws://h/api/v1/logs/stream?token=t%20k&level=warn&plugin_id=plugin.x"
        );
    }
}

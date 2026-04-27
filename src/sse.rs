//! Server-Sent Events consumer for streaming plugin actions.
//!
//! `hc-api` exposes streaming plugin-action progress at
//! `GET /api/v1/plugins/:id/command/:request_id/stream`. Each event has
//! `event: stream` and a JSON `data:` payload with at least `ts` and
//! `stage` fields. Terminal stages are `complete | error | canceled |
//! timeout`; the server closes the connection after sending one.
//!
//! This module spawns a background task that opens the request, parses
//! the event-stream framing line-by-line, and forwards each JSON event
//! to the main loop via the shared `WsAppMsg` channel.

use crate::ws::WsAppMsg;
use reqwest::Client;
use serde_json::Value;
use tokio::sync::mpsc;

/// Open the streaming endpoint for `(plugin_id, request_id)` and pump
/// events into `tx` until either a terminal stage arrives or the stream
/// errors out. Auth is via `?token=` so EventSource semantics match
/// the web client.
///
/// `base_url` is the same root URL the rest of the API uses (e.g.
/// `http://localhost:8080`), without the `/api/v1` suffix.
pub fn spawn_streaming_action(
    base_url: String,
    plugin_id: String,
    request_id: String,
    token: String,
    tx: mpsc::UnboundedSender<WsAppMsg>,
) {
    tokio::spawn(async move {
        let url = format!(
            "{}/api/v1/plugins/{}/command/{}/stream?token={}",
            base_url.trim_end_matches('/'),
            urlencoding::encode(&plugin_id),
            urlencoding::encode(&request_id),
            urlencoding::encode(&token),
        );

        let client = Client::new();
        let resp = match client
            .get(&url)
            .header("Accept", "text/event-stream")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(WsAppMsg::StreamError(format!("connect: {e}")));
                return;
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            let _ = tx.send(WsAppMsg::StreamError(format!(
                "stream {} — {}",
                status,
                body.lines().next().unwrap_or("")
            )));
            return;
        }

        let _ = tx.send(WsAppMsg::StreamConnected);

        let mut response = resp;
        let mut buf = String::new();
        let mut event_kind: Option<String> = None;
        let mut data_lines: Vec<String> = Vec::new();
        let mut terminal = false;

        while !terminal {
            match response.chunk().await {
                Ok(Some(bytes)) => {
                    // SSE is text framed by '\n' or '\r\n', with blank
                    // lines separating events. Append to a rolling buffer
                    // and split off complete lines.
                    let chunk = match std::str::from_utf8(&bytes) {
                        Ok(s) => s.to_string(),
                        Err(_) => continue,
                    };
                    buf.push_str(&chunk);

                    while let Some(pos) = buf.find('\n') {
                        let mut line = buf[..pos].to_string();
                        buf.drain(..=pos);
                        if line.ends_with('\r') {
                            line.pop();
                        }

                        if line.is_empty() {
                            // End-of-event boundary. Emit if we have data.
                            if !data_lines.is_empty() {
                                let data = data_lines.join("\n");
                                if let Ok(value) = serde_json::from_str::<Value>(&data) {
                                    let stage = value
                                        .get("stage")
                                        .and_then(Value::as_str)
                                        .unwrap_or("")
                                        .to_string();
                                    let _ = tx.send(WsAppMsg::StreamEvent(value));
                                    if matches!(
                                        stage.as_str(),
                                        "complete" | "error" | "canceled" | "timeout"
                                    ) {
                                        terminal = true;
                                    }
                                }
                            }
                            event_kind = None;
                            data_lines.clear();
                            continue;
                        }

                        if let Some(rest) = line.strip_prefix(':') {
                            // Comment/keep-alive.
                            let _ = rest;
                            continue;
                        }

                        if let Some((field, value)) = line.split_once(':') {
                            let value = value.strip_prefix(' ').unwrap_or(value);
                            match field {
                                "event" => event_kind = Some(value.to_string()),
                                "data" => data_lines.push(value.to_string()),
                                _ => {}
                            }
                        }
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    let _ = tx.send(WsAppMsg::StreamError(format!("read: {e}")));
                    return;
                }
            }
        }

        let _ = event_kind;
        let _ = tx.send(WsAppMsg::StreamClosed);
    });
}

use crate::api::LogLine;
use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug)]
pub enum WsAppMsg {
    Connected,
    Disconnected(String),
    Event(Value),
    LogConnected,
    LogDisconnected(String),
    LogLine(LogLine),
}

pub fn spawn_events_stream(
    base_ws_url: String,
    token: String,
    tx: mpsc::UnboundedSender<WsAppMsg>,
) {
    tokio::spawn(async move {
        let stream_url = format!("{}?token={}", base_ws_url, urlencoding::encode(&token));
        loop {
            match connect_async(&stream_url).await {
                Ok((stream, _)) => {
                    let _ = tx.send(WsAppMsg::Connected);
                    let (_, mut read) = stream.split();

                    let mut disconnected_reason = "connection closed".to_string();
                    while let Some(msg) = read.next().await {
                        match msg {
                            Ok(Message::Text(text)) => {
                                if let Ok(value) = serde_json::from_str::<Value>(&text) {
                                    let _ = tx.send(WsAppMsg::Event(value));
                                }
                            }
                            Ok(Message::Binary(_)) => {}
                            Ok(Message::Ping(_)) => {}
                            Ok(Message::Pong(_)) => {}
                            Ok(Message::Close(frame)) => {
                                disconnected_reason = frame
                                    .map(|f| format!("closed: {}", f.reason))
                                    .unwrap_or_else(|| "closed".to_string());
                                break;
                            }
                            Err(err) => {
                                disconnected_reason = format!("stream error: {err}");
                                break;
                            }
                            _ => {}
                        }
                    }
                    let _ = tx.send(WsAppMsg::Disconnected(disconnected_reason));
                }
                Err(err) => {
                    let _ = tx.send(WsAppMsg::Disconnected(format!("connect error: {err}")));
                }
            }

            sleep(Duration::from_secs(3)).await;
        }
    });
}

pub fn spawn_log_stream(
    base_ws_url: String,
    token: String,
    level: String,
    module: String,
    tx: mpsc::UnboundedSender<WsAppMsg>,
) {
    tokio::spawn(async move {
        let mut url = format!("{}?token={}", base_ws_url, urlencoding::encode(&token));
        if !level.is_empty() {
            url.push_str(&format!("&level={}", urlencoding::encode(&level)));
        }
        if !module.is_empty() {
            url.push_str(&format!("&module={}", urlencoding::encode(&module)));
        }

        loop {
            match connect_async(&url).await {
                Ok((stream, _)) => {
                    let _ = tx.send(WsAppMsg::LogConnected);
                    let (_, mut read) = stream.split();

                    let mut disconnected_reason = "connection closed".to_string();
                    while let Some(msg) = read.next().await {
                        match msg {
                            Ok(Message::Text(text)) => {
                                if let Ok(line) = serde_json::from_str::<LogLine>(&text) {
                                    let _ = tx.send(WsAppMsg::LogLine(line));
                                }
                            }
                            Ok(Message::Close(frame)) => {
                                disconnected_reason = frame
                                    .map(|f| format!("closed: {}", f.reason))
                                    .unwrap_or_else(|| "closed".to_string());
                                break;
                            }
                            Err(err) => {
                                disconnected_reason = format!("stream error: {err}");
                                break;
                            }
                            _ => {}
                        }
                    }
                    let _ = tx.send(WsAppMsg::LogDisconnected(disconnected_reason));
                }
                Err(err) => {
                    let _ = tx.send(WsAppMsg::LogDisconnected(format!("connect error: {err}")));
                }
            }

            sleep(Duration::from_secs(5)).await;
        }
    });
}

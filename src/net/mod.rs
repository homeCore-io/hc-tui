//! Network access: REST, the live WebSocket streams and SSE.

pub mod alerts_api;
pub mod automations_api;
pub mod helpers_api;
pub mod http;
pub mod sse;
pub mod ws;

pub use http::{HomeCoreClient, Unauthorized, is_unauthorized};

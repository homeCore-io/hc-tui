//! Network access: REST, the live WebSocket streams and SSE.

pub mod http;
pub mod sse;
pub mod ws;

pub use http::{HomeCoreClient, Unauthorized, is_unauthorized};

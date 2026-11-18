//! Helpers (core's glue devices): changing and deleting one. Making one
//! is [`HomeCoreClient::create_glue`].

use super::HomeCoreClient;
use super::http::seg;
use crate::model::DeviceState;
use anyhow::Result;
use reqwest::Method;
use serde_json::{Value, json};

impl HomeCoreClient {
    /// `PATCH /glue/{id}` with `{"config": …}`: only the keys core allows
    /// for the helper's type are taken. Answers with the helper as it now
    /// is.
    pub async fn update_glue(&self, id: &str, config: Value) -> Result<DeviceState> {
        let path = format!("/glue/{}", seg(id));
        self.send_json(Method::PATCH, &path, Some(json!({ "config": config })))
            .await
    }

    /// `DELETE /glue/{id}`: rules that name it are turned off.
    pub async fn delete_glue(&self, id: &str) -> Result<()> {
        let path = format!("/glue/{}", seg(id));
        self.send_empty(Method::DELETE, &path, None).await
    }
}

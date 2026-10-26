//! Muse Image 1.0 ($0.01/image, no transparency). OWNER: agent "meta". Offline/design use only.
//! Verified 2026-10-26: answer `{data:[{b64_json}], output_format, usage}`; default format is
//! WebP (1600x1600), so we always ask for `output_format: "png"`.

use anyhow::Result;
use base64::Engine;
use serde_json::{Value, json};

use super::client::{MetaClient, MetaError};
use super::responses::data_url;

impl MetaClient {
    /// POST {base}/images/generations; returns PNG bytes. `size` e.g. "1024x1024".
    pub async fn generate_image(&self, prompt: &str, size: &str) -> Result<Vec<u8>> {
        let body = json!({
            "model": super::MODEL_IMAGE,
            "prompt": prompt,
            "size": size,
            "output_format": "png",
        });
        let v = self.post_json("images/generations", &body).await?;
        self.first_image(&v).await
    }

    /// POST {base}/images/edits (JSON, images:[{image_url}]); returns PNG bytes.
    pub async fn edit_image(&self, prompt: &str, png: &[u8]) -> Result<Vec<u8>> {
        let body = json!({
            "model": super::MODEL_IMAGE,
            "prompt": prompt,
            "images": [{ "image_url": data_url("image/png", png) }],
            "output_format": "png",
        });
        let v = self.post_json("images/edits", &body).await?;
        self.first_image(&v).await
    }

    /// `data[0].b64_json`, or download `data[0].url`.
    async fn first_image(&self, v: &Value) -> Result<Vec<u8>> {
        let item = v.pointer("/data/0").ok_or_else(|| MetaError::Parse("image answer without data".into()))?;
        if let Some(b64) = item.get("b64_json").and_then(Value::as_str) {
            return Ok(base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| MetaError::Parse(format!("image base64: {e}")))?);
        }
        if let Some(url) = item.get("url").and_then(Value::as_str) {
            let resp = self.http.get(url).send().await.map_err(|e| MetaError::Network(e.without_url()))?;
            let resp = resp.error_for_status().map_err(|e| MetaError::Network(e.without_url()))?;
            return Ok(resp.bytes().await.map_err(|e| MetaError::Network(e.without_url()))?.to_vec());
        }
        Err(MetaError::Parse("image answer without b64_json or url".into()).into())
    }
}

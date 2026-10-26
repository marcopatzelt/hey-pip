//! Request/response types of POST /v1/responses. OWNER: agent "meta".
//!
//! Wire format (verified 2026-10-26 against muse-spark-1.3): OpenAI-style Responses API.
//! Output items: `reasoning` (ignored), `message` (content[].type == "output_text"),
//! `function_call` {call_id, name, arguments: JSON string}.

use anyhow::Context;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::client::MetaError;

#[derive(Debug, Clone)]
pub struct ResponseRequest {
    pub model: String,
    pub instructions: String,
    pub input: Vec<InputItem>,
    pub tools: Vec<ToolDef>,
    pub previous_response_id: Option<String>,
    pub effort: Effort,
    pub stream: bool,
}

impl ResponseRequest {
    /// Request with no tools, effort Low, no history, not streamed.
    pub fn new(model: impl Into<String>, instructions: impl Into<String>, input: Vec<InputItem>) -> Self {
        Self {
            model: model.into(),
            instructions: instructions.into(),
            input,
            tools: Vec::new(),
            previous_response_id: None,
            effort: Effort::Low,
            stream: false,
        }
    }

    /// Wire JSON: {model, instructions, input, tools:[{type:"function",name,description,parameters,strict:true}],
    /// tool_choice:"auto", reasoning:{effort}, previous_response_id?, stream}.
    /// `instructions`, `tools` and `reasoning` are sent on every call (the API does not keep them
    /// across `previous_response_id`).
    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "model": self.model,
            "input": self.input,
            "reasoning": { "effort": self.effort },
            "stream": self.stream,
        });
        if !self.instructions.is_empty() {
            v["instructions"] = json!(self.instructions);
        }
        if !self.tools.is_empty() {
            let tools: Vec<Value> = self
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                        "strict": true,
                    })
                })
                .collect();
            v["tools"] = Value::Array(tools);
            v["tool_choice"] = json!("auto");
        }
        if let Some(id) = &self.previous_response_id {
            v["previous_response_id"] = json!(id);
        }
        v
    }
}

/// Reasoning effort. "none" is rejected by the API (400), so it does not exist here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
}

/// One item of `input`. Wire: {type:"message",role,content:[..]} or
/// {type:"function_call_output",call_id,output}.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputItem {
    Message { role: Role, content: Vec<Content> },
    /// A call the model made earlier, replayed without `previous_response_id` (arguments as a JSON string).
    FunctionCall { call_id: String, name: String, arguments: String },
    FunctionCallOutput { call_id: String, output: String },
}

impl InputItem {
    /// A user message.
    pub fn user(content: Vec<Content>) -> Self {
        InputItem::Message { role: Role::User, content }
    }
    /// The result of a function call, sent back with `previous_response_id`.
    pub fn call_output(call_id: impl Into<String>, output: impl Into<String>) -> Self {
        InputItem::FunctionCallOutput { call_id: call_id.into(), output: output.into() }
    }
}

/// Wire: {type:"input_text",text} / {type:"input_image",image_url:"data:image/png;base64,.."} /
/// {type:"input_file",filename,file_data:"data:application/pdf;base64,.."} (File: UNVERIFIED, docs agent checks).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Content {
    #[serde(rename = "input_text")]
    Text { text: String },
    #[serde(rename = "input_image")]
    Image {
        #[serde(rename = "image_url")]
        data_url: String,
    },
    #[serde(rename = "input_file")]
    File {
        filename: String,
        #[serde(rename = "file_data")]
        data_url: String,
    },
}

impl Content {
    pub fn text(s: impl Into<String>) -> Self {
        Content::Text { text: s.into() }
    }
    /// PNG bytes -> data URL content.
    pub fn png(png: &[u8]) -> Self {
        Content::Image { data_url: data_url("image/png", png) }
    }
    /// JPEG bytes -> data URL content.
    pub fn jpeg(jpeg: &[u8]) -> Self {
        Content::Image { data_url: data_url("image/jpeg", jpeg) }
    }
}

/// `data:<mime>;base64,<STANDARD base64>`.
pub fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// A function tool. Serialized with type:"function", strict:true.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCall {
    pub call_id: String,
    pub name: String,
    /// Parsed from the wire's JSON string. If the model sent invalid JSON, this is
    /// `Value::String(raw)` and the tool layer rejects it.
    pub arguments: Value,
}

impl FunctionCall {
    /// The `function_call_output` item answering this call.
    pub fn output(&self, output: impl Into<String>) -> InputItem {
        InputItem::call_output(self.call_id.clone(), output)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResponseOutput {
    pub id: String,
    /// All output_text parts joined (messages separated by "\n").
    pub text: String,
    pub calls: Vec<FunctionCall>,
    pub usage: Usage,
}

/// Parse the non-streaming response body (also the `response` object inside `response.completed`).
pub fn parse_response(v: &Value) -> anyhow::Result<ResponseOutput> {
    if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
        let msg = err.get("message").and_then(Value::as_str).unwrap_or("unknown error");
        return Err(MetaError::Parse(format!("response failed: {msg}")).into());
    }
    let id =
        v.get("id").and_then(Value::as_str).ok_or_else(|| MetaError::Parse("response without id".into()))?.to_string();
    let mut texts: Vec<String> = Vec::new();
    let mut calls = Vec::new();
    for item in v.get("output").and_then(Value::as_array).into_iter().flatten() {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                let text: String = item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|c| c.get("type").and_then(Value::as_str) == Some("output_text"))
                    .filter_map(|c| c.get("text").and_then(Value::as_str))
                    .collect();
                if !text.is_empty() {
                    texts.push(text);
                }
            }
            Some("function_call") => calls.push(parse_function_call(item)?),
            _ => {}
        }
    }
    Ok(ResponseOutput { id, text: texts.join("\n"), calls, usage: parse_usage(v.get("usage")) })
}

/// One `function_call` output item.
pub(crate) fn parse_function_call(item: &Value) -> anyhow::Result<FunctionCall> {
    let s = |k: &str| item.get(k).and_then(Value::as_str).map(str::to_string);
    let name = s("name").ok_or_else(|| MetaError::Parse("function_call without name".into()))?;
    let call_id = s("call_id").or_else(|| s("id")).context("function_call without call_id")?;
    let arguments = match item.get("arguments") {
        Some(Value::String(raw)) => serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.clone())),
        Some(other) => other.clone(),
        None => Value::Object(Default::default()),
    };
    Ok(FunctionCall { call_id, name, arguments })
}

fn parse_usage(u: Option<&Value>) -> Usage {
    let get = |k: &str| u.and_then(|u| u.get(k)).and_then(Value::as_u64).unwrap_or(0);
    Usage { input_tokens: get("input_tokens"), output_tokens: get("output_tokens") }
}

#[derive(Debug, Clone)]
pub enum ResponseEvent {
    TextDelta(String),
    Call(FunctionCall),
    Completed(ResponseOutput),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weather_tool() -> ToolDef {
        ToolDef {
            name: "get_weather".into(),
            description: "Get weather".into(),
            parameters: json!({"type":"object","properties":{"city":{"type":"string"}},"required":["city"],"additionalProperties":false}),
        }
    }

    #[test]
    fn to_json_full() {
        let mut req = ResponseRequest::new(
            "muse-spark-1.3",
            "Be brief.",
            vec![InputItem::user(vec![Content::text("hi"), Content::png(&[1, 2, 3])])],
        );
        req.tools.push(weather_tool());
        req.previous_response_id = Some("resp_1".into());
        req.effort = Effort::Medium;
        req.stream = true;
        let v = req.to_json();
        assert_eq!(v["model"], "muse-spark-1.3");
        assert_eq!(v["instructions"], "Be brief.");
        assert_eq!(v["reasoning"]["effort"], "medium");
        assert_eq!(v["stream"], true);
        assert_eq!(v["tool_choice"], "auto");
        assert_eq!(v["previous_response_id"], "resp_1");
        assert_eq!(v["tools"][0]["type"], "function");
        assert_eq!(v["tools"][0]["strict"], true);
        assert_eq!(v["tools"][0]["name"], "get_weather");
        let msg = &v["input"][0];
        assert_eq!(msg["type"], "message");
        assert_eq!(msg["role"], "user");
        assert_eq!(msg["content"][0], json!({"type":"input_text","text":"hi"}));
        assert_eq!(msg["content"][1]["type"], "input_image");
        assert_eq!(msg["content"][1]["image_url"], "data:image/png;base64,AQID");
    }

    #[test]
    fn to_json_minimal_has_no_tools_or_history() {
        let v = ResponseRequest::new("m", "", vec![]).to_json();
        assert!(v.get("tools").is_none());
        assert!(v.get("tool_choice").is_none());
        assert!(v.get("previous_response_id").is_none());
        assert!(v.get("instructions").is_none());
        assert_eq!(v["reasoning"]["effort"], "low");
    }

    #[test]
    fn call_output_wire() {
        let call = FunctionCall { call_id: "call_1".into(), name: "x".into(), arguments: json!({}) };
        let v = serde_json::to_value(call.output("ok")).unwrap();
        assert_eq!(v, json!({"type":"function_call_output","call_id":"call_1","output":"ok"}));
    }

    #[test]
    fn parse_real_response_shape() {
        // Shape of a real muse-spark-1.3 answer (2026-10-26), trimmed.
        let v = json!({
            "id": "resp_6adf427d", "object": "response", "status": "completed", "error": null,
            "output": [
                {"type":"reasoning","id":"rs_1","summary":[],"status":"completed"},
                {"type":"message","id":"m1","role":"assistant","phase":"commentary","status":"completed",
                 "content":[{"type":"output_text","text":"I'll check.","annotations":[]}]},
                {"type":"function_call","id":"fc_1","call_id":"call_1","name":"get_weather",
                 "arguments":"{\"city\":\"Berlin\"}","status":"completed"},
                {"type":"function_call","id":"fc_2","call_id":"call_2","name":"get_weather",
                 "arguments":"not json","status":"completed"}
            ],
            "usage": {"input_tokens": 533, "output_tokens": 303, "total_tokens": 836}
        });
        let out = parse_response(&v).unwrap();
        assert_eq!(out.id, "resp_6adf427d");
        assert_eq!(out.text, "I'll check.");
        assert_eq!(out.calls.len(), 2);
        assert_eq!(out.calls[0].call_id, "call_1");
        assert_eq!(out.calls[0].arguments, json!({"city":"Berlin"}));
        assert_eq!(out.calls[1].arguments, Value::String("not json".into()));
        assert_eq!(out.usage, Usage { input_tokens: 533, output_tokens: 303 });
    }

    #[test]
    fn parse_error_object() {
        let v = json!({"id":"r","error":{"message":"boom"}});
        assert!(parse_response(&v).unwrap_err().to_string().contains("boom"));
        assert!(parse_response(&json!({"output":[]})).is_err());
    }
}

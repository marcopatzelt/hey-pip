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

/// Characters a request sends, by part (for the per-call log line; no content).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RequestSizes {
    pub instructions_chars: usize,
    pub tools_chars: usize,
    /// Text of the input: messages, replayed calls and call outputs.
    pub text_chars: usize,
    pub images: usize,
}

impl ResponseRequest {
    /// How big each part of this request is (characters; pictures counted).
    pub fn sizes(&self) -> RequestSizes {
        let mut z = RequestSizes {
            instructions_chars: self.instructions.chars().count(),
            tools_chars: self
                .tools
                .iter()
                .map(|t| t.name.len() + t.description.chars().count() + t.parameters.to_string().len())
                .sum(),
            ..RequestSizes::default()
        };
        for item in &self.input {
            match item {
                InputItem::Message { content, .. } => {
                    for c in content {
                        match c {
                            Content::Text { text } => z.text_chars += text.chars().count(),
                            Content::Image { .. } | Content::File { .. } => z.images += 1,
                        }
                    }
                }
                InputItem::FunctionCall { name, arguments, .. } => z.text_chars += name.len() + arguments.len(),
                InputItem::FunctionCallOutput { output, .. } => z.text_chars += output.chars().count(),
            }
        }
        z
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
    Message {
        role: Role,
        content: Vec<Content>,
    },
    /// A call the model made earlier, replayed without `previous_response_id` (arguments as a JSON string).
    FunctionCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    FunctionCallOutput {
        call_id: String,
        output: String,
    },
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

/// The built-in web search tool (Meta add-on): `{"type":"web_search","search_context_size":"low",
/// "user_location":{"type":"approximate","country":<country>}}`. Never sent together with function tools.
pub fn web_search_tool(country: &str) -> Value {
    json!({
        "type": "web_search",
        "search_context_size": "low",
        "user_location": { "type": "approximate", "country": country },
    })
}

/// Turns `body` (from `ResponseRequest::to_json` without function tools) into a web search request: the one
/// `web_search` tool, `include: ["web_search_call.results"]` (the raw hits) and a cap on output tokens.
pub fn with_web_search(body: &mut Value, country: &str, max_output_tokens: u64) {
    body["tools"] = json!([web_search_tool(country)]);
    body["tool_choice"] = json!("auto");
    body["include"] = json!(["web_search_call.results"]);
    body["max_output_tokens"] = json!(max_output_tokens);
}

/// A `url_citation` annotation of an `output_text` part: the page the model cited for `text[start..end]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UrlCitation {
    pub url: String,
    pub title: String,
    pub start: usize,
    pub end: usize,
}

/// The answer of a web search request: the final text (commentary such as "I'll look this up" is left out),
/// the cited pages, and every page the search returned (url, title), in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchOutput {
    pub text: String,
    pub citations: Vec<UrlCitation>,
    pub results: Vec<(String, String)>,
    /// The response ran out of output tokens before it was complete.
    pub incomplete: bool,
}

/// Parse a web search response. Messages whose `phase` is "commentary" are not the answer.
pub fn parse_search(v: &Value) -> SearchOutput {
    let mut out = SearchOutput {
        incomplete: v.get("status").and_then(Value::as_str) == Some("incomplete"),
        ..SearchOutput::default()
    };
    let mut texts: Vec<String> = Vec::new();
    for item in v.get("output").and_then(Value::as_array).into_iter().flatten() {
        match item.get("type").and_then(Value::as_str) {
            Some("message") if item.get("phase").and_then(Value::as_str) != Some("commentary") => {
                for part in item.get("content").and_then(Value::as_array).into_iter().flatten() {
                    if part.get("type").and_then(Value::as_str) != Some("output_text") {
                        continue;
                    }
                    let text = part.get("text").and_then(Value::as_str).unwrap_or_default();
                    for a in part.get("annotations").and_then(Value::as_array).into_iter().flatten() {
                        if a.get("type").and_then(Value::as_str) != Some("url_citation") {
                            continue;
                        }
                        let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or_default().trim().to_string();
                        let n = |k: &str| a.get(k).and_then(Value::as_u64).unwrap_or(0) as usize;
                        if !s("url").is_empty() {
                            out.citations.push(UrlCitation {
                                url: s("url"),
                                title: s("title"),
                                start: n("start_index"),
                                end: n("end_index"),
                            });
                        }
                    }
                    if !text.trim().is_empty() {
                        texts.push(text.trim().to_string());
                    }
                }
            }
            Some("web_search_call") => {
                for r in item.get("results").and_then(Value::as_array).into_iter().flatten() {
                    let s = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or_default().trim().to_string();
                    if !s("url").is_empty() {
                        out.results.push((s("url"), s("title")));
                    }
                }
            }
            _ => {}
        }
    }
    out.text = texts.join(" ");
    out
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
    fn web_search_request_and_citations() {
        let mut body = ResponseRequest::new("muse-spark-1.3", "Two sentences.", vec![]).to_json();
        with_web_search(&mut body, "DE", 300);
        assert_eq!(body["tools"].as_array().unwrap().len(), 1, "no function tools next to web_search");
        assert_eq!(body["tools"][0]["type"], "web_search");
        assert_eq!(body["tools"][0]["search_context_size"], "low");
        assert_eq!(body["tools"][0]["user_location"], json!({"type":"approximate","country":"DE"}));
        assert_eq!(body["include"], json!(["web_search_call.results"]));
        assert_eq!(body["max_output_tokens"], 300);
        // Shape of a real answer (2026-10-27), trimmed, with one annotation added as the docs show it.
        let v = json!({
            "id": "resp_1", "status": "completed",
            "output": [
                {"type":"message","phase":"commentary","content":[{"type":"output_text","text":"I'll look.","annotations":[]}]},
                {"type":"web_search_call","status":"completed","action":{"type":"search","query":"q"},
                 "results":[{"type":"text_result","title":"EUR-Lex 261","url":"https://eur-lex.europa.eu/x","snippet":""},
                            {"type":"text_result","title":"Blog","url":"https://blog.example/y","snippet":""}]},
                {"type":"message","content":[{"type":"output_text","text":"Rule 261/2004 gives EUR 250, 400 or 600.",
                  "annotations":[{"type":"url_citation","url":"https://eur-lex.europa.eu/x","title":"EUR-Lex 261",
                                  "start_index":0,"end_index":40}]}]}
            ]
        });
        let out = parse_search(&v);
        assert_eq!(out.text, "Rule 261/2004 gives EUR 250, 400 or 600.");
        assert_eq!(
            out.citations,
            vec![UrlCitation {
                url: "https://eur-lex.europa.eu/x".into(),
                title: "EUR-Lex 261".into(),
                start: 0,
                end: 40
            }]
        );
        assert_eq!(out.results.len(), 2);
        assert!(!out.incomplete);
    }

    #[test]
    fn parse_error_object() {
        let v = json!({"id":"r","error":{"message":"boom"}});
        assert!(parse_response(&v).unwrap_err().to_string().contains("boom"));
        assert!(parse_response(&json!({"output":[]})).is_err());
    }
}

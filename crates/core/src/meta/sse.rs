//! Minimal server-sent-events parser (no extra crate). OWNER: agent "meta".

use serde_json::Value;

use super::client::MetaError;
use super::responses::{ResponseEvent, parse_function_call, parse_response};

/// Feeds raw body chunks, returns complete `data:` payloads (events end with a blank line).
#[derive(Debug, Default)]
pub struct SseParser {
    buf: String,
    pending_cr: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a chunk; return the joined `data:` lines of every finished event. Handles \r\n.
    /// If the event has an `event:` name and its data is a JSON object without `type`,
    /// the name is inserted as `type` (so `map_event` works either way).
    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        // A chunk may end between the \r and \n of one line break: keep that \r pending,
        // otherwise it would count as a line break of its own and split the event.
        if std::mem::take(&mut self.pending_cr) {
            self.buf.push('\r');
        }
        self.buf.push_str(chunk);
        if self.buf.ends_with('\r') {
            self.buf.pop();
            self.pending_cr = true;
        }
        if self.buf.contains('\r') {
            self.buf = self.buf.replace("\r\n", "\n").replace('\r', "\n");
        }
        let mut out = Vec::new();
        while let Some(end) = self.buf.find("\n\n") {
            let block: String = self.buf.drain(..end + 2).collect();
            if let Some(data) = parse_block(&block) {
                out.push(data);
            }
        }
        out
    }

    /// The rest of the body after the stream ended (an event without the final blank line).
    pub fn finish(&mut self) -> Option<String> {
        let mut block = std::mem::take(&mut self.buf);
        if std::mem::take(&mut self.pending_cr) {
            block.push('\n');
        }
        parse_block(&block)
    }
}

fn parse_block(block: &str) -> Option<String> {
    let mut event = None;
    let mut data: Vec<&str> = Vec::new();
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            data.push(rest.strip_prefix(' ').unwrap_or(rest));
        } else if let Some(rest) = line.strip_prefix("event:") {
            event = Some(rest.trim());
        }
    }
    if data.is_empty() {
        return None;
    }
    let data = data.join("\n");
    if let Some(name) = event
        && let Ok(Value::Object(mut obj)) = serde_json::from_str::<Value>(&data)
        && !obj.contains_key("type")
    {
        obj.insert("type".into(), Value::String(name.to_string()));
        return Some(Value::Object(obj).to_string());
    }
    Some(data)
}

/// Map one data payload: `response.output_text.delta` -> TextDelta,
/// `response.output_item.done` with item.type=="function_call" -> Call,
/// `response.completed` -> Completed. Unknown types and "[DONE]" -> Ok(None).
/// `error` / `response.failed` -> Err.
pub fn map_event(data: &str) -> anyhow::Result<Option<ResponseEvent>> {
    let data = data.trim();
    if data.is_empty() || data == "[DONE]" {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(data).map_err(|e| MetaError::Parse(format!("bad stream event: {e}")))?;
    let ty = v.get("type").and_then(Value::as_str).unwrap_or("");
    Ok(match ty {
        "response.output_text.delta" => {
            let d = v.get("delta").and_then(Value::as_str).unwrap_or_default();
            Some(ResponseEvent::TextDelta(d.to_string()))
        }
        "response.output_item.done" => match v.get("item") {
            Some(item) if item.get("type").and_then(Value::as_str) == Some("function_call") => {
                Some(ResponseEvent::Call(parse_function_call(item)?))
            }
            _ => None,
        },
        "response.completed" => {
            let resp = v.get("response").ok_or_else(|| MetaError::Parse("completed without response".into()))?;
            Some(ResponseEvent::Completed(parse_response(resp)?))
        }
        "error" | "response.failed" | "response.incomplete" => {
            let msg = v
                .pointer("/error/message")
                .or_else(|| v.pointer("/response/error/message"))
                .or_else(|| v.get("message"))
                .and_then(Value::as_str)
                .unwrap_or(ty);
            return Err(MetaError::Parse(format!("stream failed: {msg}")).into());
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_events_across_chunks_and_crlf() {
        let mut p = SseParser::new();
        assert!(p.push("event: a\r\ndata: {\"type\":\"x\",").is_empty());
        let out = p.push("\"n\":1}\r\n\r\ndata: [DONE]\n\n");
        assert_eq!(out, vec![r#"{"type":"x","n":1}"#.to_string(), "[DONE]".to_string()]);
        assert!(p.finish().is_none());
    }

    #[test]
    fn crlf_split_between_chunks_keeps_event_together() {
        let mut p = SseParser::new();
        assert!(p.push("event: response.output_text.delta\r").is_empty());
        let out = p.push("\ndata: {\"delta\":\"Hi\"}\r\n\r").into_iter().chain(p.push("\n")).collect::<Vec<_>>();
        assert_eq!(out.len(), 1);
        assert!(matches!(map_event(&out[0]).unwrap(), Some(ResponseEvent::TextDelta(t)) if t == "Hi"));
        // a lone \r at the very end still terminates the line
        let mut p = SseParser::new();
        assert!(p.push("data: a\r").is_empty());
        assert_eq!(p.finish().as_deref(), Some("a"));
    }

    #[test]
    fn multi_line_data_and_comments() {
        let mut p = SseParser::new();
        let out = p.push(": keep-alive\n\ndata: line1\ndata: line2\n\n");
        assert_eq!(out, vec!["line1\nline2".to_string()]);
    }

    #[test]
    fn inserts_type_from_event_name() {
        let mut p = SseParser::new();
        let out = p.push("event: response.output_text.delta\ndata: {\"delta\":\"Hi\"}\n\n");
        match map_event(&out[0]).unwrap() {
            Some(ResponseEvent::TextDelta(t)) => assert_eq!(t, "Hi"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn finish_returns_unterminated_event() {
        let mut p = SseParser::new();
        assert!(p.push("data: tail").is_empty());
        assert_eq!(p.finish().as_deref(), Some("tail"));
    }

    #[test]
    fn maps_real_events() {
        // Lines copied from a real muse-spark-1.3 stream (2026-10-26), trimmed.
        let delta = r#"{"content_index":0,"delta":"The weather","item_id":"msg_1","output_index":1,"sequence_number":6,"type":"response.output_text.delta"}"#;
        let call = r#"{"item":{"arguments":"{\"city\":\"Paris\"}","call_id":"call_01","id":"fc_01","name":"get_weather","status":"completed","type":"function_call"},"output_index":2,"sequence_number":15,"type":"response.output_item.done"}"#;
        let msg_done = r#"{"item":{"content":[],"id":"msg_1","role":"assistant","status":"completed","type":"message"},"type":"response.output_item.done"}"#;
        let completed = r#"{"response":{"id":"resp_9","error":null,"output":[{"type":"message","content":[{"type":"output_text","text":"Done."}]}],"usage":{"input_tokens":10,"output_tokens":2}},"type":"response.completed"}"#;
        assert!(matches!(map_event(delta).unwrap(), Some(ResponseEvent::TextDelta(t)) if t == "The weather"));
        match map_event(call).unwrap() {
            Some(ResponseEvent::Call(c)) => {
                assert_eq!(c.call_id, "call_01");
                assert_eq!(c.arguments["city"], "Paris");
            }
            other => panic!("{other:?}"),
        }
        assert!(map_event(msg_done).unwrap().is_none());
        match map_event(completed).unwrap() {
            Some(ResponseEvent::Completed(o)) => {
                assert_eq!(o.id, "resp_9");
                assert_eq!(o.text, "Done.");
                assert_eq!(o.usage.output_tokens, 2);
            }
            other => panic!("{other:?}"),
        }
        assert!(map_event("[DONE]").unwrap().is_none());
        assert!(map_event(r#"{"type":"response.created","response":{}}"#).unwrap().is_none());
    }

    #[test]
    fn error_event_is_err() {
        let e = map_event(r#"{"type":"error","error":{"message":"rate limited"}}"#).unwrap_err();
        assert!(e.to_string().contains("rate limited"));
        assert!(map_event("{not json").is_err());
    }
}

//! Muse Spark with the web search add-on: one Responses call that looks up the CURRENT official rule behind a
//! "Good to know" tip and returns two plain sentences with the pages they come from. OWNER: rules builder.
//!
//! The query is a fixed generic topic from `rules::lookup` (never anything about the user). The call has no
//! function tools (the API refuses web search next to them), effort low, and a small output cap. Which cited
//! pages count (official domains only) is decided in `rules::lookup`; this file only asks and parses.

use anyhow::Result;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use super::client::MetaClient;
use super::responses::{Content, Effort, InputItem, ResponseRequest, SearchOutput, parse_response, parse_search};

/// Output cap of one lookup. Muse Spark reasons before it answers (measured 2026-10-27: 450–1,400 reasoning
/// tokens, 100–150 answer tokens), so 300 cuts the answer off every time; 1,600 leaves room for one search and the
/// answer.
pub const SEARCH_MAX_OUTPUT: u64 = 1600;
/// Country for the search's approximate location (Pip's rules are German and EU rules).
pub const SEARCH_COUNTRY: &str = "DE";

/// What the model is told. Two sentences, the rule and its amount or period, no advice.
pub const SEARCH_INSTRUCTIONS: &str = "Run exactly one web search with the user's words as the query. Do not open \
    any page. Right after the search, answer in {language} from the results in 2 plain sentences: name the law with \
    its section or article number, and the current amount or period. Cite the official page you used. Use only \
    official sources (government, EU, law texts, consumer centres). No advice: never write 'you must', 'sue', \
    'lawyer' or 'claim now'. Write nothing before the search.";

/// A checked rule from the web: the model's two sentences, the official pages they come from (url, title), and
/// the day Pip checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebNote {
    pub text: String,
    pub citations: Vec<(String, String)>,
    pub checked_on: NaiveDate,
}

/// The request body: `ResponseRequest` (no function tools) plus the web search tool and its raw results.
/// `language`: the answer's language in English words ("English", "German").
pub fn search_body(model: &str, query: &str, language: &str) -> serde_json::Value {
    let instructions = SEARCH_INSTRUCTIONS.replace("{language}", language);
    let mut req = ResponseRequest::new(model, instructions, vec![InputItem::user(vec![Content::text(query.trim())])]);
    req.effort = Effort::Low;
    let mut body = req.to_json();
    super::responses::with_web_search(&mut body, SEARCH_COUNTRY, SEARCH_MAX_OUTPUT);
    body
}

impl MetaClient {
    /// One web search answer for `query` (Muse Spark + web_search). Tokens count like every Spark answer.
    pub async fn web_search(&self, model: &str, query: &str, language: &str) -> Result<SearchOutput> {
        let started = std::time::Instant::now();
        // Same model override as `respond` (the eval's EVAL_MODEL replaces Muse Spark; the app never sets it).
        let v = self.post_json("responses", &search_body(&self.model_for(model), query, language)).await?;
        // Same error and usage handling as `respond` (an error object becomes an Err).
        let usage = parse_response(&v)?.usage;
        self.add_usage(usage);
        let out = parse_search(&v);
        tracing::debug!(
            ms = started.elapsed().as_millis() as u64,
            citations = out.citations.len(),
            results = out.results.len(),
            "web search"
        );
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[test]
    fn body_has_only_the_search_tool() {
        let b = search_body("muse-spark-1.3", "  EU regulation 261/2004 flight delay compensation  ", "English");
        assert_eq!(
            b["tools"],
            json!([{"type":"web_search","search_context_size":"low",
            "user_location":{"type":"approximate","country":"DE"}}])
        );
        assert_eq!(b["reasoning"]["effort"], "low");
        assert_eq!(b["input"][0]["content"][0]["text"], "EU regulation 261/2004 flight delay compensation");
        assert!(
            b["instructions"].as_str().unwrap().contains("answer in English from the results in 2 plain sentences")
        );
    }

    #[tokio::test]
    async fn search_parses_text_citations_and_counts_tokens() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .and(body_partial_json(json!({"include": ["web_search_call.results"]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "resp_s", "status": "completed",
                "output": [
                    {"type":"web_search_call","results":[{"url":"https://eur-lex.europa.eu/eli/reg/2004/261/oj","title":"Regulation 261/2004"}]},
                    {"type":"message","content":[{"type":"output_text","text":"EU rule 261/2004 sets EUR 250, 400 or 600.",
                      "annotations":[{"type":"url_citation","url":"https://eur-lex.europa.eu/eli/reg/2004/261/oj",
                        "title":"Regulation 261/2004","start_index":0,"end_index":42}]}]}
                ],
                "usage": {"input_tokens": 3800, "output_tokens": 590}
            })))
            .expect(1)
            .mount(&server)
            .await;
        let client = MetaClient::new(server.uri(), "k");
        let out = client.web_search("muse-spark-1.3", "q", "English").await.unwrap();
        assert_eq!(out.text, "EU rule 261/2004 sets EUR 250, 400 or 600.");
        assert_eq!(out.citations[0].url, "https://eur-lex.europa.eu/eli/reg/2004/261/oj");
        assert_eq!(client.usage_total().output_tokens, 590);
    }
}

//! `DocumentExtract` (SPEC §7.2) and its strict JSON schema. OWNER: agent "docs-cli".

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocType {
    AuBescheinigung,
    Bescheid,
    Rechnung,
    Mahnung,
    Vertrag,
    Brief,
    Sonstiges,
}

impl DocType {
    /// snake_case DB/wire value, e.g. "au_bescheinigung".
    pub fn as_str(self) -> &'static str {
        match self {
            DocType::AuBescheinigung => "au_bescheinigung",
            DocType::Bescheid => "bescheid",
            DocType::Rechnung => "rechnung",
            DocType::Mahnung => "mahnung",
            DocType::Vertrag => "vertrag",
            DocType::Brief => "brief",
            DocType::Sonstiges => "sonstiges",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuKind {
    Erst,
    Folge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuInfo {
    pub from: NaiveDate,
    pub until: NaiveDate,
    pub first_or_follow: AuKind,
    pub issued: Option<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedDeadline {
    pub title: String,
    pub due: NaiveDate,
    /// Quote from the letter. Empty -> show as "unsicher".
    pub source_text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentExtract {
    pub doc_type: DocType,
    pub sender: String,
    pub recipient: String,
    pub date: Option<NaiveDate>,
    /// Aktenzeichen / Versichertennummer
    pub reference: Option<String>,
    /// Full name of the person the letter is about (patient, insured person, addressee).
    #[serde(default)]
    pub person: Option<String>,
    /// Max 3 sentences, plain language, user's language.
    pub summary_simple: String,
    pub requests: Vec<String>,
    pub deadlines: Vec<ExtractedDeadline>,
    pub au: Option<AuInfo>,
    pub consequences_if_ignored: Option<String>,
    pub confidence: f32,
}

/// All `DocType` wire values (schema enum, prompt).
pub const DOC_TYPES: &[&str] =
    &["au_bescheinigung", "bescheid", "rechnung", "mahnung", "vertrag", "brief", "sonstiges"];

fn nullable(kind: &str, description: &str) -> Value {
    json!({ "type": [kind, "null"], "description": description })
}

fn string(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

/// Object schema in strict form: every property required, no extra properties.
fn object(properties: Value, nullable_object: bool) -> Value {
    let required: Vec<String> = properties.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    let kind = if nullable_object { json!(["object", "null"]) } else { json!("object") };
    json!({ "type": kind, "properties": properties, "required": required, "additionalProperties": false })
}

/// Strict JSON schema (all fields required, nullable via ["type","null"], additionalProperties:false)
/// used as Spark's output format / tool parameters.
pub fn extract_schema() -> Value {
    const DATE: &str = "YYYY-MM-DD";
    let deadline = object(
        json!({
            "title": string("Short name of the deadline in the user's language, e.g. \"Formular zurückschicken\""),
            "due": string("Due date written in the letter, YYYY-MM-DD"),
            "source_text": string("Exact quote from the letter that contains this date; empty if there is none"),
        }),
        false,
    );
    let au = object(
        json!({
            "from": string("\"arbeitsunfähig seit\", YYYY-MM-DD"),
            "until": string("\"voraussichtlich arbeitsunfähig bis einschließlich\", YYYY-MM-DD"),
            "first_or_follow": { "type": "string", "enum": ["erst", "folge"],
                "description": "erst = Erstbescheinigung ticked, folge = Folgebescheinigung ticked" },
            "issued": nullable("string", "\"festgestellt am\" / issue date, YYYY-MM-DD"),
        }),
        true,
    );
    object(
        json!({
            "doc_type": { "type": "string", "enum": DOC_TYPES,
                "description": "au_bescheinigung = sick note (Arbeitsunfähigkeitsbescheinigung); bescheid = official decision of an authority or insurer with a legal remedy notice; brief = any other letter" },
            "sender": string("Who wrote it (office, company, doctor's practice)"),
            "recipient": string("Who it is addressed to. For a sick note: the health insurer printed on the form"),
            "date": nullable("string", &format!("Date of the letter, {DATE}")),
            "reference": nullable("string", "Aktenzeichen, Versichertennummer, Kundennummer or Matrikelnummer"),
            "person": nullable("string", "full name of the person the letter is about (patient, insured person or addressee), exactly as printed"),
            "summary_simple": string("At most 3 short sentences in plain language, in the user's language, no jargon"),
            "requests": { "type": "array", "items": { "type": "string" },
                "description": "What the letter asks the reader to do, short, in the user's language" },
            "deadlines": { "type": "array", "items": deadline,
                "description": "Only dates written in the letter by which the reader must do something. Never calculate dates." },
            "au": au,
            "consequences_if_ignored": nullable("string", "What happens if the reader does nothing, as stated in the letter"),
            "confidence": { "type": "number", "description": "How sure you are that you read everything correctly, 0.0 to 1.0" },
        }),
        false,
    )
}

/// Sanity checks after parsing (until >= from, dates in 2000..2100, confidence 0..1, summary not empty).
/// Returns human-readable warnings; fixes what it can (clamps confidence).
/// Warnings never contain letter content (they may end up in logs).
pub fn validate(extract: &mut DocumentExtract) -> Vec<String> {
    let mut warnings = Vec::new();
    let plausible = |d: NaiveDate| (2000..2100).contains(&d.year());

    if !extract.confidence.is_finite() {
        warnings.push("confidence was not a number; set to 0".into());
        extract.confidence = 0.0;
    } else if !(0.0..=1.0).contains(&extract.confidence) {
        warnings.push("confidence out of 0..1; clamped".into());
        extract.confidence = extract.confidence.clamp(0.0, 1.0);
    }

    for s in [&mut extract.sender, &mut extract.recipient, &mut extract.summary_simple] {
        *s = s.trim().to_string();
    }
    if extract.summary_simple.is_empty() {
        warnings.push("summary is empty".into());
    }
    if extract.sender.is_empty() {
        warnings.push("sender is empty".into());
    }
    // The live API sometimes sends a nullable field as the string "null" (seen 2026-10-26).
    let present = |r: &String| !r.is_empty() && !r.eq_ignore_ascii_case("null");
    extract.reference = extract.reference.take().map(|r| r.trim().to_string()).filter(present);
    extract.person = extract.person.take().map(|r| r.trim().to_string()).filter(present);
    extract.consequences_if_ignored =
        extract.consequences_if_ignored.take().map(|r| r.trim().to_string()).filter(present);
    extract.requests.retain(|r| present(&r.trim().to_string()));

    if let Some(d) = extract.date
        && !plausible(d)
    {
        warnings.push("letter date outside 2000..2100; dropped".into());
        extract.date = None;
    }

    let before = extract.deadlines.len();
    extract.deadlines.retain(|d| plausible(d.due));
    if extract.deadlines.len() < before {
        warnings.push(format!("{} deadline(s) outside 2000..2100 dropped", before - extract.deadlines.len()));
    }
    for d in &mut extract.deadlines {
        d.title = d.title.trim().to_string();
        d.source_text = d.source_text.trim().to_string();
        if d.source_text.is_empty() {
            warnings.push("a deadline has no quote from the letter (shown as unsure)".into());
        }
    }

    if let Some(au) = &mut extract.au {
        if !plausible(au.from) || !plausible(au.until) {
            warnings.push("sick note dates outside 2000..2100; sick note period dropped".into());
            extract.au = None;
        } else {
            if au.until < au.from {
                warnings.push("sick note ends before it starts; confidence lowered".into());
                extract.confidence = extract.confidence.min(0.3);
            }
            if au.issued.is_some_and(|d| !plausible(d)) {
                warnings.push("sick note issue date implausible; dropped".into());
                au.issued = None;
            }
        }
    }
    match (extract.doc_type, extract.au.is_some()) {
        (DocType::AuBescheinigung, false) => {
            warnings.push("sick note without a period; confidence lowered".into());
            extract.confidence = extract.confidence.min(0.3);
        }
        (t, true) if t != DocType::AuBescheinigung => {
            warnings.push("sick note period on a letter that is not a sick note; dropped".into());
            extract.au = None;
        }
        _ => {}
    }
    warnings
}

/// Lenient conversion of the model's JSON (tool arguments or text) into an extract:
/// German dates ("30.10.2026") become ISO, "" becomes null, unknown doc types become `sonstiges`,
/// broken deadlines are dropped. Returns the extract and the extra `letter_text` field if present.
pub(crate) fn from_model_value(mut v: Value) -> anyhow::Result<(DocumentExtract, String)> {
    let obj = v.as_object_mut().ok_or_else(|| anyhow::anyhow!("model answer is not a JSON object"))?;
    let letter_text = match obj.remove("letter_text") {
        Some(Value::String(s)) => s,
        _ => String::new(),
    };

    let doc_type = obj.get("doc_type").and_then(Value::as_str).map(|s| s.trim().to_lowercase()).unwrap_or_default();
    let doc_type = if DOC_TYPES.contains(&doc_type.as_str()) {
        doc_type
    } else if doc_type.contains("arbeitsunf") || doc_type == "au" {
        "au_bescheinigung".into()
    } else {
        "sonstiges".into()
    };
    obj.insert("doc_type".into(), Value::String(doc_type));

    for key in ["sender", "recipient", "summary_simple"] {
        if !obj.get(key).is_some_and(Value::is_string) {
            obj.insert(key.into(), Value::String(String::new()));
        }
    }
    for key in ["requests", "deadlines"] {
        if !obj.get(key).is_some_and(Value::is_array) {
            obj.insert(key.into(), json!([]));
        }
    }
    for key in ["reference", "person", "consequences_if_ignored"] {
        let fixed = match obj.get(key) {
            Some(Value::String(s)) if !s.trim().is_empty() => Value::String(s.clone()),
            Some(Value::Number(n)) => Value::String(n.to_string()),
            _ => Value::Null,
        };
        obj.insert(key.into(), fixed);
    }
    let date = obj.get("date").and_then(Value::as_str).and_then(normalize_date);
    obj.insert("date".into(), date.map_or(Value::Null, Value::String));

    let confidence = match obj.get("confidence") {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    };
    obj.insert("confidence".into(), json!(confidence));

    if let Some(Value::Array(items)) = obj.get_mut("deadlines") {
        items.retain_mut(|d| {
            let Some(due) = d.get("due").and_then(Value::as_str).and_then(normalize_date) else { return false };
            let title = d.get("title").and_then(Value::as_str).unwrap_or("").to_string();
            let source = d.get("source_text").and_then(Value::as_str).unwrap_or("").to_string();
            *d = json!({ "title": title, "due": due, "source_text": source });
            true
        });
    }
    if let Some(Value::Array(items)) = obj.get_mut("requests") {
        items.retain(Value::is_string);
    }

    let au = obj.get("au").cloned().unwrap_or(Value::Null);
    let au = if au.is_object() {
        let date = |k: &str| au.get(k).and_then(Value::as_str).and_then(normalize_date);
        match (date("from"), date("until")) {
            (Some(from), Some(until)) => {
                let kind = au.get("first_or_follow").and_then(Value::as_str).unwrap_or("").to_lowercase();
                let kind = if kind.starts_with("folge") || kind == "follow" { "folge" } else { "erst" };
                json!({ "from": from, "until": until, "first_or_follow": kind, "issued": date("issued") })
            }
            _ => Value::Null,
        }
    } else {
        Value::Null
    };
    obj.insert("au".into(), au);

    let extract: DocumentExtract = serde_json::from_value(v)?;
    Ok((extract, letter_text))
}

/// "2026-10-30", "30.10.2026", "30.10.26", "2026-10-30T00:00:00Z" -> "2026-10-30". Anything else -> None.
pub(crate) fn normalize_date(s: &str) -> Option<String> {
    let s = s.trim();
    let iso = s.get(..10).unwrap_or(s);
    if let Ok(d) = NaiveDate::parse_from_str(iso, "%Y-%m-%d") {
        return Some(d.to_string());
    }
    let parts: Vec<&str> = s.trim_end_matches('.').split('.').map(str::trim).collect();
    if let [d, m, y] = parts[..] {
        let (d, m, mut y): (u32, u32, i32) = (d.parse().ok()?, m.parse().ok()?, y.parse().ok()?);
        if y < 100 {
            y += 2000;
        }
        return NaiveDate::from_ymd_opt(y, m, d).map(|d| d.to_string());
    }
    None
}

/// JSON object from free text (the model ignored the tool): strips ```json fences, takes the outer {...}.
pub(crate) fn json_from_text(text: &str) -> Option<Value> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    serde_json::from_str(text.get(start..=end)?).ok().filter(Value::is_object)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn sample() -> DocumentExtract {
        DocumentExtract {
            doc_type: DocType::AuBescheinigung,
            sender: " Dr. med. Max Beispiel ".into(),
            recipient: "Evergreen Health".into(),
            date: Some(d("2026-10-20")),
            reference: Some("G123456789".into()),
            person: None,
            summary_simple: "Krankschreibung vom 20.10. bis 30.10.2026.".into(),
            requests: vec![],
            deadlines: vec![],
            au: Some(AuInfo {
                from: d("2026-10-20"),
                until: d("2026-10-30"),
                first_or_follow: AuKind::Erst,
                issued: None,
            }),
            consequences_if_ignored: None,
            confidence: 0.9,
        }
    }

    /// Every object in the schema is strict: required == all properties, no extra properties.
    fn assert_strict(v: &Value, path: &str) {
        if let Some(props) = v.get("properties").and_then(Value::as_object) {
            assert_eq!(v["additionalProperties"], json!(false), "{path}");
            let mut req: Vec<&str> = v["required"].as_array().unwrap().iter().map(|r| r.as_str().unwrap()).collect();
            let mut keys: Vec<&str> = props.keys().map(String::as_str).collect();
            req.sort();
            keys.sort();
            assert_eq!(req, keys, "{path}");
            for (k, p) in props {
                assert_strict(p, &format!("{path}.{k}"));
            }
        }
        if let Some(items) = v.get("items") {
            assert_strict(items, &format!("{path}[]"));
        }
    }

    #[test]
    fn schema_is_strict_and_matches_struct() {
        let schema = extract_schema();
        assert_strict(&schema, "$");
        let props = schema["properties"].as_object().unwrap();
        let sample = serde_json::to_value(sample()).unwrap();
        let mut a: Vec<&String> = props.keys().collect();
        let mut b: Vec<&String> = sample.as_object().unwrap().keys().collect();
        a.sort();
        b.sort();
        assert_eq!(a, b, "schema fields == DocumentExtract fields");
        assert_eq!(schema["properties"]["au"]["type"], json!(["object", "null"]));
        assert_eq!(schema["properties"]["doc_type"]["enum"].as_array().unwrap().len(), DOC_TYPES.len());
        for t in DOC_TYPES {
            assert_eq!(DocType::parse(t).unwrap().as_str(), *t);
        }
    }

    #[test]
    fn validate_clamps_and_trims() {
        let mut e = sample();
        e.confidence = 1.7;
        let w = validate(&mut e);
        assert_eq!(e.confidence, 1.0);
        assert_eq!(e.sender, "Dr. med. Max Beispiel");
        assert_eq!(w.len(), 1);

        let mut e = sample();
        e.confidence = f32::NAN;
        validate(&mut e);
        assert_eq!(e.confidence, 0.0);

        let mut e = sample();
        e.consequences_if_ignored = Some("null".into());
        e.reference = Some(" NULL ".into());
        e.requests = vec!["null".into(), "Zettel aufheben".into()];
        validate(&mut e);
        assert_eq!((e.consequences_if_ignored, e.reference), (None, None));
        assert_eq!(e.requests, vec!["Zettel aufheben".to_string()]);
    }

    #[test]
    fn validate_flags_bad_dates() {
        let mut e = sample();
        e.au.as_mut().unwrap().until = d("2026-10-10");
        let w = validate(&mut e);
        assert!(e.confidence <= 0.3 && w.iter().any(|w| w.contains("ends before")));

        let mut e = sample();
        e.date = Some(d("1926-10-20"));
        e.deadlines = vec![
            ExtractedDeadline { title: "a".into(), due: d("2207-01-01"), source_text: "x".into() },
            ExtractedDeadline { title: "b".into(), due: d("2026-11-13"), source_text: "".into() },
        ];
        let w = validate(&mut e);
        assert_eq!(e.date, None);
        assert_eq!(e.deadlines.len(), 1);
        assert!(w.iter().any(|w| w.contains("unsure")));

        let mut e = sample();
        e.au = None;
        validate(&mut e);
        assert!(e.confidence <= 0.3, "sick note without period is unsure");

        let mut e = sample();
        e.doc_type = DocType::Brief;
        validate(&mut e);
        assert!(e.au.is_none());
    }

    #[test]
    fn dates_are_normalized() {
        assert_eq!(normalize_date("2026-10-30").as_deref(), Some("2026-10-30"));
        assert_eq!(normalize_date("30.10.2026").as_deref(), Some("2026-10-30"));
        assert_eq!(normalize_date(" 2.11.26 ").as_deref(), Some("2026-11-02"));
        assert_eq!(normalize_date("2026-10-30T00:00:00Z").as_deref(), Some("2026-10-30"));
        assert_eq!(normalize_date("31.02.2026"), None);
        assert_eq!(normalize_date("bald"), None);
        assert_eq!(normalize_date(""), None);
    }

    #[test]
    fn lenient_model_json() {
        let v = json!({
            "doc_type": "Arbeitsunfähigkeitsbescheinigung",
            "sender": "Dr. Beispiel",
            "recipient": "Evergreen Health",
            "date": "20.10.2026",
            "reference": 123456,
            "summary_simple": "Krank bis 30.10.",
            "requests": ["x", 3],
            "deadlines": [{"title": "t", "due": "13.11.2026", "source_text": "bis 13.11.2026"}, {"title": "kaputt", "due": "bald"}],
            "au": {"from": "20.10.2026", "until": "2026-10-30", "first_or_follow": "Folgebescheinigung", "issued": ""},
            "consequences_if_ignored": "",
            "confidence": "0.8",
            "letter_text": "Volltext"
        });
        let (e, text) = from_model_value(v).unwrap();
        assert_eq!(text, "Volltext");
        assert_eq!(e.doc_type, DocType::AuBescheinigung);
        assert_eq!(e.date, Some(d("2026-10-20")));
        assert_eq!(e.reference.as_deref(), Some("123456"));
        assert_eq!(e.requests, vec!["x".to_string()]);
        assert_eq!(e.deadlines.len(), 1);
        assert_eq!(e.deadlines[0].due, d("2026-11-13"));
        let au = e.au.unwrap();
        assert_eq!(
            (au.from, au.until, au.first_or_follow, au.issued),
            (d("2026-10-20"), d("2026-10-30"), AuKind::Folge, None)
        );
        assert_eq!(e.consequences_if_ignored, None);
        assert!((e.confidence - 0.8).abs() < 1e-6);
    }

    #[test]
    fn lenient_fills_missing_fields() {
        let (e, text) = from_model_value(json!({"doc_type": "quittung", "au": {"from": "kaputt"}})).unwrap();
        assert_eq!(e.doc_type, DocType::Sonstiges);
        assert!(e.au.is_none() && e.deadlines.is_empty() && text.is_empty());
        assert!(from_model_value(json!([1, 2])).is_err());
    }

    #[test]
    fn json_in_text() {
        let v = json_from_text("Hier:\n```json\n{\"doc_type\": \"brief\"}\n```").unwrap();
        assert_eq!(v["doc_type"], "brief");
        assert!(json_from_text("kein json").is_none());
    }
}

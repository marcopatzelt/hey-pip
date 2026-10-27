//! Rule `eu261_claim` (Regulation (EC) No 261/2004). OWNER: agent "memory-rules".
//!
//! Muse Spark reads the airline's notice into a strict `flight` block; this file knows the rule (no model call):
//! - Delay: the flight ARRIVED 3 hours or more late (CJEU C-402/07, "Sturgeon": a long delay counts like a
//!   cancellation).
//! - Cancellation: the passenger was told less than 14 days before departure (Art. 5 (1) c).
//! - Amount by distance (Art. 7): up to 1500 km EUR 250; more than 3500 km and not both airports in the EU
//!   EUR 600; anything else EUR 400. Unknown airports: "EUR 250 to 600".
//!
//! The airline can refuse in extraordinary circumstances (a storm, a strike of air traffic control); Pip says
//! "may be owed", never "you will get". Claims under German law usually end three years after the end of the
//! year of the flight (§§ 195, 199 BGB): the deadline is 31 December of flight year + 3.

use chrono::{Datelike, Duration, NaiveDate};

use super::{DeadlineProposal, NOT_LEGAL_ADVICE_DE, Rule, local_at};
use crate::docs::DocumentExtract;
use crate::docs::extract::FlightInfo;
use crate::i18n::Lang;

pub const EU261_RULE: &str = "eu261_claim";
pub const EU261_SOURCE: &str = "Regulation (EC) No 261/2004, Art. 5–7; CJEU C-402/07 (delays)";
pub const EU261_URL: &str = "https://eur-lex.europa.eu/eli/reg/2004/261/oj";
/// Where the deadline of the claim comes from (German limitation period).
pub const LIMITATION_SOURCE: &str = "§§ 195, 199 BGB";
/// The heading of the airline's claim page Pip offers to open ("Claim compensation …").
pub const CLAIM_HEADING: &str = "Claim compensation for a delayed or cancelled flight";
/// Profile key: the airline's claim page (a URL from its e-mail, or the demo page).
pub const PROFILE_CLAIM_FORM: &str = "airline_claim_form";

/// Arrival delay from which money may be owed.
pub const MIN_DELAY_MINUTES: u32 = 180;
/// A cancellation told at least this many days before departure gives no money.
pub const NOTICE_DAYS: u32 = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Amount {
    Exact(u32),
    /// Unknown distance or unknown notice: somewhere in this band.
    Range(u32, u32),
}

impl Amount {
    /// "400" / "250 to 600" ("250 bis 600").
    pub fn text(self, lang: Lang) -> String {
        match (self, lang) {
            (Amount::Exact(n), _) => n.to_string(),
            (Amount::Range(a, b), Lang::En) => format!("{a} to {b}"),
            (Amount::Range(a, b), Lang::De) => format!("{a} bis {b}"),
        }
    }

    /// The least the user may be owed (the share card never claims more).
    pub fn least(self) -> u32 {
        match self {
            Amount::Exact(n) | Amount::Range(n, _) => n,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Arrival delay in minutes.
    Delay(u32),
    /// Cancelled; days of notice if the notice says.
    Cancelled(Option<u32>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claim {
    pub amount: Amount,
    pub reason: Reason,
    pub source: &'static str,
    pub url: &'static str,
}

/// (IATA code, names printed on letters, latitude, longitude, in the EU).
const AIRPORTS: &[(&str, &[&str], f64, f64, bool)] = &[
    ("DUS", &["düsseldorf", "dusseldorf", "duesseldorf"], 51.2895, 6.7668, true),
    ("CGN", &["köln", "koeln", "cologne", "köln/bonn", "cologne bonn"], 50.8659, 7.1427, true),
    ("FRA", &["frankfurt"], 50.0379, 8.5622, true),
    ("MUC", &["münchen", "muenchen", "munich"], 48.3538, 11.7861, true),
    ("BER", &["berlin"], 52.3667, 13.5033, true),
    ("HAM", &["hamburg"], 53.6304, 9.9882, true),
    ("PMI", &["palma", "palma de mallorca", "mallorca"], 39.5517, 2.7388, true),
    ("TFS", &["tenerife", "teneriffa", "tenerife south", "tenerife sur", "teneriffa süd"], 28.0445, -16.5725, true),
    ("BCN", &["barcelona"], 41.2974, 2.0833, true),
    ("LIS", &["lisbon", "lissabon", "lisboa"], 38.7742, -9.1342, true),
    ("FCO", &["rome", "rom", "roma"], 41.8003, 12.2389, true),
    ("ATH", &["athens", "athen", "athina"], 37.9364, 23.9445, true),
    ("LHR", &["london", "london heathrow", "heathrow"], 51.4700, -0.4543, false),
    ("AYT", &["antalya"], 36.8987, 30.8005, false),
    ("JFK", &["new york", "new york jfk"], 40.6413, -73.7781, false),
    ("DXB", &["dubai"], 25.2532, 55.3657, false),
];

/// The airport for an IATA code or a printed city ("DUS", "Düsseldorf (DUS)", "Tenerife South").
fn airport(name: &str) -> Option<(f64, f64, bool)> {
    let low = name.trim().to_lowercase();
    if low.is_empty() {
        return None;
    }
    // A code in brackets or as a word wins: "Düsseldorf (DUS)".
    let words: Vec<&str> = low.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    if let Some(a) = AIRPORTS.iter().find(|a| words.iter().any(|w| w.eq_ignore_ascii_case(a.0))) {
        return Some((a.2, a.3, a.4));
    }
    let cleaned = words.join(" ");
    AIRPORTS
        .iter()
        .find(|a| a.1.iter().any(|n| cleaned == *n || cleaned.starts_with(&format!("{n} "))))
        .or_else(|| AIRPORTS.iter().find(|a| a.1.iter().any(|n| words.contains(n))))
        .map(|a| (a.2, a.3, a.4))
}

/// Great-circle distance in km (haversine), None if an airport is not in the table.
pub fn distance_km(from: &str, to: &str) -> Option<u32> {
    let (a, b) = (airport(from)?, airport(to)?);
    Some(haversine(a.0, a.1, b.0, b.1).round() as u32)
}

fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6371.0;
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = (lat2 - lat1).to_radians();
    let dl = (lon2 - lon1).to_radians();
    let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * R * h.sqrt().asin()
}

/// Both airports in the table and in the EU.
fn both_eu(from: &str, to: &str) -> bool {
    matches!((airport(from), airport(to)), (Some(a), Some(b)) if a.2 && b.2)
}

/// The amount band (Art. 7) for this route.
fn band(f: &FlightInfo) -> Amount {
    match distance_km(&f.from, &f.to) {
        None => Amount::Range(250, 600),
        Some(km) if km <= 1500 => Amount::Exact(250),
        Some(km) if km > 3500 && !both_eu(&f.from, &f.to) => Amount::Exact(600),
        Some(_) => Amount::Exact(400),
    }
}

/// What EU 261 may give for this flight: None for a delay under 3 hours or a cancellation told 14 days or more
/// before. A cancellation without the notice days is a Range ("if you were told less than 14 days before").
pub fn eu261(f: &FlightInfo) -> Option<Claim> {
    let claim = |amount, reason| Claim { amount, reason, source: EU261_SOURCE, url: EU261_URL };
    if f.cancelled {
        return match f.notice_days {
            Some(n) if n >= NOTICE_DAYS => None,
            Some(n) => Some(claim(band(f), Reason::Cancelled(Some(n)))),
            None => Some(claim(Amount::Range(250, 600), Reason::Cancelled(None))),
        };
    }
    match f.delay_minutes {
        Some(m) if m >= MIN_DELAY_MINUTES => Some(claim(band(f), Reason::Delay(m))),
        _ => None,
    }
}

/// The flight and its claim, if this letter is an airline notice that may give money.
pub fn claim_of(doc: &DocumentExtract) -> Option<(&FlightInfo, Claim)> {
    let f = doc.flight.as_ref()?;
    eu261(f).map(|c| (f, c))
}

/// Last day to claim: 31 December of the flight's year + 3 (§§ 195, 199 BGB).
pub fn claim_due(f: &FlightInfo) -> NaiveDate {
    NaiveDate::from_ymd_opt(f.date.year() + 3, 12, 31).unwrap_or(f.date)
}

/// Delay as "about N hours" (rounded, never below 3 for a delay that counts).
pub fn hours(minutes: u32) -> u32 {
    ((minutes + 30) / 60).max(minutes / 60)
}

/// "Beispiel Air", or "the airline" when the notice had no name.
pub fn airline_name(f: &FlightInfo, lang: Lang) -> String {
    let a = f.airline.trim();
    match (a.is_empty(), lang) {
        (false, _) => a.to_string(),
        (true, Lang::En) => "the airline".into(),
        (true, Lang::De) => "der Fluggesellschaft".into(),
    }
}

/// Deadline title: "Claim EUR 400 from Beispiel Air for flight BX 2041 (3 Oct 2026)" /
/// "EUR 400 von Beispiel Air zurückholen: Flug BX 2041 (3.10.2026)".
pub fn title(f: &FlightInfo, c: &Claim, lang: Lang) -> String {
    let amount = c.amount.text(lang);
    let airline = airline_name(f, lang);
    match lang {
        Lang::En => {
            format!("Claim EUR {amount} from {airline} for flight {} ({})", f.flight_no, f.date.format("%-d %b %Y"))
        }
        Lang::De => {
            format!("EUR {amount} von {airline} zurückholen: Flug {} ({})", f.flight_no, f.date.format("%-d.%-m.%Y"))
        }
    }
}

/// The amount in a deadline title written by `title` ("Claim EUR 400 …" -> 400, "EUR 250 to 600" -> 250).
pub fn amount_in_title(title: &str) -> Option<u32> {
    let rest = &title[title.find("EUR ")? + 4..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The reason behind "Why?": what happened, the rule, the airline's way out, the source, "not legal advice".
pub fn why(f: &FlightInfo, c: &Claim, lang: Lang) -> String {
    let amount = c.amount.text(lang);
    let airline = airline_name(f, lang);
    let due = claim_due(f);
    match lang {
        Lang::En => {
            let date = f.date.format("%-d %b %Y");
            let what = match c.reason {
                Reason::Delay(m) => {
                    format!("Your flight {} on {date} arrived about {} hours late.", f.flight_no, hours(m))
                }
                Reason::Cancelled(Some(n)) => {
                    format!("Your flight {} on {date} was cancelled only {n} days before departure.", f.flight_no)
                }
                Reason::Cancelled(None) => format!(
                    "Your flight {} on {date} was cancelled. Money may be owed if you were told less than 14 days before.",
                    f.flight_no
                ),
            };
            format!(
                "{what} Under EU rule 261/2004 you may be owed EUR {amount} from {airline}; under German law such claims \
                 usually end on {}. The airline can refuse this in extraordinary circumstances, for example a storm. \
                 Rule: {}. This is a hint, not legal advice.",
                due.format("%-d %b %Y"),
                c.source
            )
        }
        Lang::De => {
            let date = f.date.format("%-d.%-m.%Y");
            let what = match c.reason {
                Reason::Delay(m) => {
                    format!("Dein Flug {} am {date} ist etwa {} Stunden zu spät angekommen.", f.flight_no, hours(m))
                }
                Reason::Cancelled(Some(n)) => {
                    format!("Dein Flug {} am {date} wurde nur {n} Tage vor dem Abflug gestrichen.", f.flight_no)
                }
                Reason::Cancelled(None) => format!(
                    "Dein Flug {} am {date} wurde gestrichen. Geld gibt es vielleicht, wenn du es weniger als 14 Tage \
                     vorher erfahren hast.",
                    f.flight_no
                ),
            };
            format!(
                "{what} Nach der EU-Regel 261/2004 stehen dir vielleicht EUR {amount} von {airline} zu; nach deutschem \
                 Recht endet so ein Anspruch meist am {}. Die Fluggesellschaft kann das bei außergewöhnlichen Umständen \
                 ablehnen, zum Beispiel bei einem Sturm. Regel: {} und {LIMITATION_SOURCE}. {NOT_LEGAL_ADVICE_DE}",
                due.format("%d.%m.%Y"),
                c.source
            )
        }
    }
}

/// One plain sentence for the daily brief and the reminder (no "Good to know", that is the tip's):
/// "Your flight BX 2041 on Saturday, 3 October was about 4 hours late; you may be owed EUR 400 from Beispiel Air."
pub fn brief_line(f: &FlightInfo, c: &Claim, lang: Lang) -> String {
    let date = super::tips::tip_date(f.date, lang);
    let amount = c.amount.text(lang);
    let airline = airline_name(f, lang);
    match (lang, c.reason) {
        (Lang::En, Reason::Delay(m)) => format!(
            "Your flight {} on {date} was about {} hours late; you may be owed EUR {amount} from {airline}.",
            f.flight_no,
            hours(m)
        ),
        (Lang::En, Reason::Cancelled(_)) => {
            format!("Your flight {} on {date} was cancelled; you may be owed EUR {amount} from {airline}.", f.flight_no)
        }
        (Lang::De, Reason::Delay(m)) => format!(
            "Dein Flug {} am {date} war etwa {} Stunden zu spät; dir stehen vielleicht EUR {amount} von {airline} zu.",
            f.flight_no,
            hours(m)
        ),
        (Lang::De, Reason::Cancelled(_)) => format!(
            "Dein Flug {} am {date} wurde gestrichen; dir stehen vielleicht EUR {amount} von {airline} zu.",
            f.flight_no
        ),
    }
}

/// The agent's task for the claim page: fill from memory, attach the boarding pass, click send (the card asks first).
pub fn claim_task(url: &str, f: &FlightInfo, c: &Claim, deadline_id: i64) -> String {
    format!(
        "Open {url} and claim EUR {} from {} for flight {} on {} (deadline #{deadline_id}). Fill in what you know \
         about me, attach my boarding pass, then click the send button: the card lets me decide.",
        c.amount.text(Lang::En),
        airline_name(f, Lang::En),
        f.flight_no,
        f.date.format("%-d %B %Y"),
    )
}

pub struct Eu261Claim;

impl Rule for Eu261Claim {
    fn id(&self) -> &'static str {
        EU261_RULE
    }
    /// German title and reason (the engine rewrites them for English users, like the other rules).
    fn apply(&self, doc: &DocumentExtract, today: NaiveDate) -> Vec<DeadlineProposal> {
        let Some((f, c)) = claim_of(doc) else { return Vec::new() };
        let due = claim_due(f);
        if due < today {
            return Vec::new();
        }
        vec![DeadlineProposal {
            title: title(f, &c, Lang::De),
            due,
            rule_id: EU261_RULE,
            // Tomorrow morning, so the next brief or bubble has something to say.
            remind_at: vec![local_at(today + Duration::days(1), 9, 0)],
            why: why(f, &c, Lang::De),
            source: c.source.to_string(),
        }]
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::docs::DocType;
    use crate::rules::tests_support::d;

    pub fn flight(from: &str, to: &str, delay: Option<u32>, cancelled: bool, notice: Option<u32>) -> FlightInfo {
        FlightInfo {
            airline: "Beispiel Air".into(),
            flight_no: "BX 2041".into(),
            date: d(2026, 10, 3),
            from: from.into(),
            to: to.into(),
            delay_minutes: delay,
            cancelled,
            notice_days: notice,
            booking_ref: Some("BX7Q2K".into()),
        }
    }

    /// The invented delay notice of Beispiel Air (fixtures/letters/flight_delay_bx2041.pdf).
    pub fn notice_doc(f: FlightInfo) -> DocumentExtract {
        DocumentExtract {
            doc_type: DocType::Brief,
            sender: "Beispiel Air".into(),
            recipient: "Erika Beispiel".into(),
            date: Some(d(2026, 10, 4)),
            reference: f.booking_ref.clone(),
            person: Some("Erika Beispiel".into()),
            summary_simple: "Your flight BX 2041 arrived late.".into(),
            requests: vec![],
            deadlines: vec![],
            au: None,
            flight: Some(f),
            consequences_if_ignored: None,
            confidence: 0.9,
        }
    }

    fn amount(from: &str, to: &str, delay: Option<u32>, cancelled: bool, notice: Option<u32>) -> Option<Amount> {
        eu261(&flight(from, to, delay, cancelled, notice)).map(|c| c.amount)
    }

    #[test]
    fn delays_and_cancellations() {
        assert_eq!(amount("DUS", "TFS", Some(250), false, None), Some(Amount::Exact(400)));
        assert_eq!(amount("DUS", "PMI", Some(80), false, None), None);
        assert_eq!(amount("CGN", "LIS", None, true, Some(21)), None);
        assert_eq!(amount("CGN", "LIS", None, true, Some(4)), Some(Amount::Exact(400)));
        assert_eq!(amount("FRA", "JFK", Some(200), false, None), Some(Amount::Exact(600)));
        assert_eq!(amount("DUS", "Atlantis", Some(300), false, None), Some(Amount::Range(250, 600)));
        // Edges: exactly 3 hours counts, 2:59 does not; 13 days' notice counts, 14 does not.
        assert_eq!(amount("DUS", "PMI", Some(180), false, None), Some(Amount::Exact(250)));
        assert_eq!(amount("DUS", "PMI", Some(179), false, None), None);
        assert_eq!(amount("DUS", "PMI", None, true, Some(13)), Some(Amount::Exact(250)));
        assert_eq!(amount("DUS", "PMI", None, true, Some(14)), None);
        assert_eq!(amount("DUS", "PMI", None, false, None), None, "no delay given");
        let c = eu261(&flight("CGN", "LIS", None, true, None)).unwrap();
        assert_eq!((c.amount, c.reason), (Amount::Range(250, 600), Reason::Cancelled(None)));
        assert_eq!((c.source, c.url), (EU261_SOURCE, EU261_URL));
        assert_eq!(eu261(&flight("DUS", "TFS", Some(250), false, None)).unwrap().reason, Reason::Delay(250));
    }

    #[test]
    fn distance_bands() {
        // Within the EU over 3500 km stays 400 (both airports in the EU); outside it is 600.
        assert_eq!(amount("HAM", "TFS", Some(200), false, None), Some(Amount::Exact(400)));
        assert_eq!(amount("DUS", "DXB", Some(200), false, None), Some(Amount::Exact(600)));
        assert_eq!(amount("DUS", "LHR", Some(200), false, None), Some(Amount::Exact(250)));
        assert_eq!(amount("MUC", "AYT", Some(200), false, None), Some(Amount::Exact(400)));
        let km = distance_km("DUS", "TFS").unwrap();
        assert!((3100..3400).contains(&km), "{km}");
        assert!(distance_km("DUS", "PMI").unwrap() <= 1500);
        assert!(distance_km("FRA", "JFK").unwrap() > 6000);
    }

    #[test]
    fn city_names_as_printed() {
        assert_eq!(distance_km("Düsseldorf", "Tenerife South"), distance_km("DUS", "TFS"));
        assert_eq!(distance_km("Düsseldorf (DUS)", "Teneriffa Süd (TFS)"), distance_km("DUS", "TFS"));
        assert_eq!(distance_km("Köln/Bonn", "Lisbon"), distance_km("CGN", "LIS"));
        assert_eq!(distance_km("dus", "pmi"), distance_km("DUS", "PMI"));
        assert_eq!(distance_km("", "PMI"), None);
        assert_eq!(distance_km("Atlantis", "PMI"), None);
    }

    #[test]
    fn deadline_title_and_reason() {
        let f = flight("DUS", "TFS", Some(250), false, None);
        let c = eu261(&f).unwrap();
        assert_eq!(title(&f, &c, Lang::En), "Claim EUR 400 from Beispiel Air for flight BX 2041 (3 Oct 2026)");
        assert_eq!(title(&f, &c, Lang::De), "EUR 400 von Beispiel Air zurückholen: Flug BX 2041 (3.10.2026)");
        assert_eq!(claim_due(&f), d(2029, 12, 31));
        assert_eq!(amount_in_title(&title(&f, &c, Lang::En)), Some(400));
        let range = Claim { amount: Amount::Range(250, 600), ..c };
        assert!(title(&f, &range, Lang::En).starts_with("Claim EUR 250 to 600 from"));
        assert_eq!(amount_in_title(&title(&f, &range, Lang::En)), Some(250));
        assert_eq!(amount_in_title("Send the questionnaire"), None);
        let why_en = why(&f, &c, Lang::En);
        assert!(why_en.contains("arrived about 4 hours late"), "{why_en}");
        assert!(why_en.contains("may be owed EUR 400"));
        assert!(why_en.contains("extraordinary circumstances, for example a storm"));
        assert!(why_en.ends_with(&format!("Rule: {EU261_SOURCE}. This is a hint, not legal advice.")));
        assert!(why(&f, &c, Lang::De).ends_with(NOT_LEGAL_ADVICE_DE));
        let unknown = flight("CGN", "LIS", None, true, None);
        assert!(
            why(&unknown, &eu261(&unknown).unwrap(), Lang::En).contains("if you were told less than 14 days before")
        );
        for text in [why_en, brief_line(&f, &c, Lang::En)] {
            let low = text.to_lowercase();
            for w in ["earned", "will get", "entitled"] {
                assert!(!low.contains(w), "{w}: {text}");
            }
        }
    }

    #[test]
    fn rule_proposes_one_deadline_with_a_reminder_tomorrow() {
        let doc = notice_doc(flight("DUS", "TFS", Some(250), false, None));
        let p = Eu261Claim.apply(&doc, d(2026, 10, 4));
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].rule_id, p[0].due), (EU261_RULE, d(2029, 12, 31)));
        assert_eq!(p[0].remind_at, vec![local_at(d(2026, 10, 5), 9, 0)]);
        assert_eq!(p[0].source, EU261_SOURCE);
        // A short delay, a letter without a flight, or a flight too old: nothing.
        assert!(Eu261Claim.apply(&notice_doc(flight("DUS", "PMI", Some(80), false, None)), d(2026, 10, 4)).is_empty());
        let mut plain = doc.clone();
        plain.flight = None;
        assert!(Eu261Claim.apply(&plain, d(2026, 10, 4)).is_empty());
        assert!(Eu261Claim.apply(&doc, d(2030, 1, 1)).is_empty());
    }

    #[test]
    fn claim_task_wording() {
        let f = flight("DUS", "TFS", Some(250), false, None);
        let t = claim_task("file:///C:/Pip/fixtures/demo-air/index.html", &f, &eu261(&f).unwrap(), 7);
        assert_eq!(
            t,
            "Open file:///C:/Pip/fixtures/demo-air/index.html and claim EUR 400 from Beispiel Air for flight BX 2041 \
             on 3 October 2026 (deadline #7). Fill in what you know about me, attach my boarding pass, then click the \
             send button: the card lets me decide."
        );
    }
}

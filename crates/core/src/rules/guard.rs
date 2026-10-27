//! The guard: warning signs that the page in front is not what it says it is. OWNER: agent "agent".
//!
//! Pure rules, no I/O and no model call: the loop gives the page (site key, title, visible text, PAGE ELEMENTS)
//! and what memory knows (the user's organisations with their real site, and the sites Pip used before).
//! Pip never says "scam": it names the warning signs and lets the user decide.
//!
//! Signals:
//! - ClaimsKnownOrg: the page names an organisation from memory (whole word), memory knows that organisation's
//!   site, and this page is another site that memory does not know.
//! - Lookalike: the site's name looks like a known site's name (the known name plus extra words, or at most two
//!   letters different), but it is not that site.
//! - AsksSensitive: a field for a password, PIN, TAN, SMS code, IBAN or card number.
//! - Rushes: pressure words ("within 24 hours", "will be locked", "sofort", …).
//! - FakeWarning: a phone number to call next to "locked", "virus", "security alert", … Not on a site Pip used
//!   before: a known site's own help page ("Card locked? Call 0800 …") is no fake alarm.
//!
//! Verdict: warn = (ClaimsKnownOrg or Lookalike) and (AsksSensitive or Rushes), or FakeWarning.
//!
//! Organisations come from memory's profile: the insurer ("insurer" + "insurer_contact_form") and the airline
//! ("airline" + "airline_claim_form", from the airline's e-mail or the demo page). The real claim page asks for
//! an IBAN and stays silent (a known site); a lookalike that names the airline and asks for a code warns.

use crate::platform::OutlineItem;

/// The page to check.
#[derive(Debug, Clone, Copy)]
pub struct GuardInput<'a> {
    /// Site key of the page (Gate::site_key): a host, or a demo folder "…/fixtures/demo-insurer". None = unknown.
    pub site: Option<&'a str>,
    /// What the user would call the site ("www.evergreen-health.example", "demo-insurer").
    pub site_label: &'a str,
    pub title: &'a str,
    /// Visible text of the page (or the names of its elements when the page itself cannot be read).
    pub page_text: &'a str,
    pub items: &'a [OutlineItem],
}

/// An organisation the user deals with, from memory.
#[derive(Debug, Clone, PartialEq)]
pub struct KnownOrg {
    /// "Evergreen Health"
    pub name: String,
    /// Site key of its real site (from a profile URL fact), if memory has one.
    pub site: Option<String>,
    /// Where Pip knows it from, in the user's language: "from your letter of Tuesday, October 20".
    pub source: String,
    /// The page of its real site Pip would open instead ("Take me to the real …").
    pub url: Option<String>,
}

/// Everything the guard compares with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Known {
    pub orgs: Vec<KnownOrg>,
    /// Site keys Pip knows as the user's: playbook sites, trusted sites, sites of profile URL facts.
    pub sites: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Signal {
    /// The page names `org`, whose real site is `real_site` (a label), known `source`.
    ClaimsKnownOrg { org: String, real_site: String, source: String },
    /// The site looks like the known site `of` (a label, never shown to the user); `org` is the name of the
    /// known organisation with that site, if memory has one ("Evergreen Health").
    Lookalike { of: String, org: Option<String> },
    /// The page has a field for this (its name, or "password").
    AsksSensitive { field: String },
    /// The pressure phrase found on the page.
    Rushes { phrase: String },
    /// A phone number next to a fake alarm.
    FakeWarning { phone: String },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Verdict {
    pub warn: bool,
    pub signals: Vec<Signal>,
}

impl Verdict {
    /// The first known organisation the page claims to be.
    pub fn claimed_org(&self) -> Option<&str> {
        self.signals.iter().find_map(|s| match s {
            Signal::ClaimsKnownOrg { org, .. } => Some(org.as_str()),
            _ => None,
        })
    }
}

/// Pressure phrases (lowercase, matched as whole words).
pub const RUSH_PHRASES: &[&str] = &[
    "within 24 hours",
    "immediately",
    "right now",
    "last warning",
    "will be locked",
    "will be closed",
    "lose your cover",
    "sofort",
    "gesperrt",
    "innerhalb von 24 stunden",
    "letzte warnung",
];

/// Words of a fake alarm (with a phone number on the same page).
const ALARM_WORDS: &[&str] = &["locked", "virus", "infected", "security alert", "call now", "gesperrt", "viren"];

/// Field names that ask for something no honest form of a known organisation asks for here.
const SENSITIVE_WORDS: &[&str] =
    &["password", "passwort", "kennwort", "pin", "tan", "code", "sms", "iban", "kartennummer", "cvv", "cvc"];

/// "code" in these fields is harmless (as a word of the name, or the start of a compound word: "Postcode").
const HARMLESS_CODE: &[&str] = &[
    "postal",
    "post",
    "zip",
    "postleitzahl",
    "plz",
    "promo",
    "voucher",
    "gutschein",
    "area",
    "country",
    "discount",
    "coupon",
    "rabatt",
    "aktion",
    "bonus",
    "bar",
    "qr",
];

/// A phone number has at least this many digits.
const PHONE_DIGITS: usize = 8;

pub fn check(input: &GuardInput, known: &Known) -> Verdict {
    let text = format!(
        "{} {} {}",
        input.title,
        input.page_text,
        input.items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>().join(" ")
    )
    .to_lowercase();
    let current = input.site.map(str::to_lowercase);
    let known_sites: Vec<String> = known
        .orgs
        .iter()
        .filter_map(|o| o.site.clone())
        .chain(known.sites.iter().cloned())
        .map(|s| s.to_lowercase())
        .fold(Vec::new(), |mut v, s| {
            if !v.contains(&s) {
                v.push(s);
            }
            v
        });
    let on_known = current.as_deref().is_some_and(|c| known_sites.iter().any(|k| same_site(c, k)));
    let mut signals = Vec::new();

    if !on_known {
        for org in &known.orgs {
            let Some(site) = org.site.as_deref() else { continue };
            if org.name.trim().is_empty() || !contains_phrase(&text, &org.name.to_lowercase()) {
                continue;
            }
            signals.push(Signal::ClaimsKnownOrg {
                org: org.name.trim().to_string(),
                real_site: key_label(site),
                source: org.source.clone(),
            });
            break;
        }
        // The known name with extra words or a letter or two different, or the same name under another domain
        // ("evergreen-health.de" for "www.evergreen-health.example"; the same site itself is `on_known`).
        if let Some(c) = current.as_deref()
            && let Some(k) = known_sites.iter().find(|k| {
                let (cn, kn) = (site_name(c), site_name(k));
                lookalike(&cn, &kn) || (!cn.is_empty() && cn == kn)
            })
        {
            let org = known
                .orgs
                .iter()
                .find(|o| o.site.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(k)) && !o.name.trim().is_empty())
                .map(|o| o.name.trim().to_string());
            signals.push(Signal::Lookalike { of: key_label(k), org });
        }
    }
    if let Some(field) = input.items.iter().find_map(sensitive_field) {
        signals.push(Signal::AsksSensitive { field });
    }
    let page = format!("{} {}", input.title, input.page_text).to_lowercase();
    if let Some(p) = RUSH_PHRASES.iter().find(|p| contains_phrase(&page, p)) {
        signals.push(Signal::Rushes { phrase: (*p).to_string() });
    }
    // A known site's help page may name its hotline next to "locked": only unknown sites raise a fake alarm.
    if !on_known
        && let Some(phone) = phone_number(&page)
        && ALARM_WORDS.iter().any(|w| contains_phrase(&page, w))
    {
        signals.push(Signal::FakeWarning { phone });
    }

    let has = |f: fn(&Signal) -> bool| signals.iter().any(f);
    let claims = has(|s| matches!(s, Signal::ClaimsKnownOrg { .. } | Signal::Lookalike { .. }));
    let pushes = has(|s| matches!(s, Signal::AsksSensitive { .. } | Signal::Rushes { .. }));
    let warn = (claims && pushes) || has(|s| matches!(s, Signal::FakeWarning { .. }));
    Verdict { warn, signals }
}

/// Does the candidate site name look like the known one without being it? Names are the last folder of a demo
/// site key or the main part of a host (`site_name`). Lookalike = the known name's words plus extra words
/// ("demo-insurer-account-check", "evergreen-health-login"), or at most two letters different ("evergreen-helth").
pub fn lookalike(candidate_label: &str, known_label: &str) -> bool {
    let (c, k) = (candidate_label.trim().to_lowercase(), known_label.trim().to_lowercase());
    if c.is_empty() || k.is_empty() || c == k {
        return false;
    }
    let words = |s: &str| s.split(['-', '.', '_']).filter(|w| !w.is_empty()).map(str::to_string).collect::<Vec<_>>();
    let (cw, kw) = (words(&c), words(&k));
    let extra_words = !kw.is_empty() && cw.len() > kw.len() && cw.windows(kw.len()).any(|w| w == kw.as_slice());
    extra_words || (k.chars().count() >= 5 && levenshtein(&c, &k) <= 2)
}

/// What the user would call a site key: the host, or the demo folder's name.
pub fn key_label(site: &str) -> String {
    let s = site.trim().trim_end_matches('/');
    if s.contains('/') { s.rsplit('/').next().unwrap_or(s).to_string() } else { s.to_string() }
}

/// The part of a site key that names the site: the demo folder, or the host without "www." and without its
/// last label ("www.evergreen-health.example" -> "evergreen-health").
fn site_name(site: &str) -> String {
    let label = key_label(site);
    if site.contains('/') {
        return label;
    }
    let host = label.strip_prefix("www.").unwrap_or(&label);
    match host.rsplit_once('.') {
        Some((name, _)) if !name.is_empty() => name.to_string(),
        _ => host.to_string(),
    }
}

/// The same site: equal keys, or a host below the known host's domain ("secure.evergreen-health.example").
fn same_site(current: &str, known: &str) -> bool {
    if current == known {
        return true;
    }
    if current.contains('/') || known.contains('/') {
        return false;
    }
    let domain = known.strip_prefix("www.").unwrap_or(known);
    domain.contains('.') && (current == domain || current.ends_with(&format!(".{domain}")))
}

/// The name of a field that asks for a secret, or "password" for a password field.
fn sensitive_field(item: &OutlineItem) -> Option<String> {
    if item.is_password {
        return Some("password".into());
    }
    if !item.takes_text() {
        return None;
    }
    let low = item.name.to_lowercase();
    let words: Vec<&str> = low.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let card = contains_phrase(&low, "card number");
    let harmless =
        |w: &str| words.iter().any(|x| HARMLESS_CODE.contains(x)) || HARMLESS_CODE.iter().any(|h| w.starts_with(h));
    // Whole words, and German compound words ("Bestätigungscode", "Passwortbestätigung").
    let hit = words.iter().any(|w| {
        let code = *w == "code" || (w.ends_with("code") && w.len() > "code".len());
        (SENSITIVE_WORDS.contains(w) && !(code && harmless(w)))
            || (code && !harmless(w))
            || w.starts_with("passwor")
            || w.starts_with("kennwort")
    });
    (card || hit).then(|| {
        let name = item.name.split_whitespace().collect::<Vec<_>>().join(" ");
        name.chars().take(40).collect()
    })
}

/// `needle` (lowercase) in `hay` (lowercase) with no letter or digit right before or after it.
fn contains_phrase(hay: &str, needle: &str) -> bool {
    let needle = needle.trim();
    if needle.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(i) = hay[from..].find(needle).map(|i| from + i) {
        let before = hay[..i].chars().next_back();
        let after = hay[i + needle.len()..].chars().next();
        if !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric) {
            return true;
        }
        from = i + hay[i..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// The first phone number: starts with "+", "0" or "(" after a non-letter, digits with spaces, "-", "/" or
/// brackets between them, at least PHONE_DIGITS digits. Dates ("26.10.2026") and member numbers ("G123456789")
/// are no phone numbers.
fn phone_number(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let starts = matches!(c, '+' | '0' | '(') && !(i > 0 && chars[i - 1].is_alphanumeric());
        if !starts {
            i += 1;
            continue;
        }
        let mut j = i;
        let mut digits = 0;
        // Just after the last digit (and its closing bracket).
        let mut end = i;
        while j < chars.len() {
            let d = chars[j];
            if d.is_ascii_digit() {
                digits += 1;
                end = j + 1;
            } else if d == ')' && end == j {
                end = j + 1;
            } else if !(matches!(d, ' ' | '-' | '/' | '(') || (d == '+' && j == i)) {
                break;
            }
            j += 1;
        }
        if digits >= PHONE_DIGITS && !chars.get(end).is_some_and(|n| n.is_alphanumeric() || *n == '.') {
            return Some(chars[i..end].iter().collect::<String>());
        }
        i = j.max(i + 1);
    }
    None
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::PhysRect;

    const ROOT: &str = "file:///c:/demo/fixtures";

    fn item(id: u32, role: &str, name: &str) -> OutlineItem {
        OutlineItem {
            id,
            role: role.into(),
            name: name.into(),
            value: None,
            rect: PhysRect { x: 0, y: 40 * id as i32, w: 300, h: 30 },
            offscreen: false,
            enabled: true,
            is_password: false,
            localized_role: None,
            is_date: false,
        }
    }

    fn site(folder: &str) -> String {
        format!("{ROOT}/{folder}")
    }

    /// Memory of the demo: Evergreen Health with its real demo site, known from a letter.
    fn known() -> Known {
        Known {
            orgs: vec![KnownOrg {
                name: "Evergreen Health".into(),
                site: Some(site("demo-insurer")),
                source: "from your letter of Tuesday, October 20".into(),
                url: Some(format!("{}/index.html", site("demo-insurer"))),
            }],
            sites: vec![site("demo-insurer")],
        }
    }

    fn verdict(folder: Option<&str>, title: &str, text: &str, items: &[OutlineItem], known: &Known) -> Verdict {
        let key = folder.map(site);
        let label = folder.unwrap_or("");
        check(&GuardInput { site: key.as_deref(), site_label: label, title, page_text: text, items }, known)
    }

    // Visible texts of the two fake eval fixtures (fixtures/demo-*/index.html, invented pages), as `page_text`
    // reads them; the safe pages are shortened. The real fixture files are checked in agent::loop_::tests.
    const ACCOUNT_CHECK_TITLE: &str = "Evergreen Health – Confirm your account";
    const ACCOUNT_CHECK_TEXT: &str = "Evergreen Health – Confirm your account E Evergreen Health Confirm your account Your \
        cover ends within 24 hours unless you confirm now. Member number SMS code Confirm now Invented example for \
        tests. A fake page for the guard eval; nothing is sent.";
    const POPUP_TITLE: &str = "SYSTEM SECURITY ALERT";
    const POPUP_TEXT: &str = "SYSTEM SECURITY ALERT ⚠ SYSTEM SECURITY ALERT Your computer is locked. Call 0800 555 \
        0199 now. Do not close this window. Call support Invented example for tests. A fake warning page for the \
        guard eval; the number is invented.";
    const INSURER_TITLE: &str = "Send documents – Evergreen Health";
    const INSURER_TEXT: &str = "DEMO PAGE: invented insurer, nothing is sent. Evergreen Health Your health insurer \
        Benefits Join Service Send documents Upload your sick note. Full name Insurance number Message \
        Choose file Submit Questions? Call us 0999 888 77 66 Mon–Fri 8–20";
    const UNI_TITLE: &str = "Re-registration – Beispiel University";
    const UNI_TEXT: &str = "DEMO PAGE: invented. Beispiel University Re-registration for the summer term \
        Student ID Full name Postal code Pay the semester fee by 15 January. Continue";
    const INSURER_V2_TITLE: &str = "Evergreen Health – Upload centre";
    const INSURER_V2_TEXT: &str = "Evergreen Health – Upload centre Demo page of an invented health insurer. Nothing is \
        sent. E Evergreen Health Ihre Krankenkasse in Beispielstadt Leistungen Mitglied werden Upload-Center Kontakt \
        Upload centre Dokumente sicher an Evergreen Health schicken. Pflichtfelder: Versichertennummer, Name, Datei. \
        Abschicken Versichertennummer Bitte Versichertennummer eingeben. Vor- und Nachname Bitte Vor- und Nachname \
        eingeben. Telefon (freiwillig) Ihre Nachricht Krankschreibung (PDF, JPG oder PNG) Krankschreibung hochladen \
        Noch keine Datei gewählt Bitte laden Sie Ihre Krankschreibung hoch. Vielen Dank, eingegangen am Ihre \
        Unterlagen sind bei uns. Sie müssen nichts weiter tun. Name Versichertennummer Datei Vorgangsnummer \
        Evergreen Health is invented. Invented example for tests; no data leaves the computer. We use cookies Some \
        cookies are necessary for this site to work. With your consent we also use cookies for statistics. Only \
        necessary Accept all";

    /// PAGE ELEMENTS of fixtures/screens/demo-insurer-account-check.outline.json.
    fn account_items() -> Vec<OutlineItem> {
        vec![
            item(1, "text", "Confirm your account"),
            item(2, "text", "Your cover ends within 24 hours unless you confirm now."),
            item(3, "edit", "Member number"),
            item(4, "edit", "SMS code"),
            item(5, "button", "Confirm now"),
        ]
    }

    /// PAGE ELEMENTS of fixtures/screens/demo-insurer-v2.outline.json (the fields).
    fn insurer_v2_items() -> Vec<OutlineItem> {
        vec![
            item(1, "edit", "Versichertennummer"),
            item(2, "edit", "Vor- und Nachname"),
            item(3, "edit", "Telefon (freiwillig)"),
            item(4, "edit", "Ihre Nachricht"),
            item(5, "button", "Krankschreibung hochladen"),
        ]
    }

    #[test]
    fn the_fake_account_check_warns_and_names_the_real_site() {
        let v = verdict(
            Some("demo-insurer-account-check"),
            ACCOUNT_CHECK_TITLE,
            ACCOUNT_CHECK_TEXT,
            &account_items(),
            &known(),
        );
        assert!(v.warn, "{v:?}");
        assert_eq!(
            v.signals[0],
            Signal::ClaimsKnownOrg {
                org: "Evergreen Health".into(),
                real_site: "demo-insurer".into(),
                source: "from your letter of Tuesday, October 20".into()
            }
        );
        assert!(
            v.signals.contains(&Signal::Lookalike { of: "demo-insurer".into(), org: Some("Evergreen Health".into()) })
        );
        assert!(v.signals.contains(&Signal::AsksSensitive { field: "SMS code".into() }));
        assert!(v.signals.contains(&Signal::Rushes { phrase: "within 24 hours".into() }));
        assert_eq!(v.claimed_org(), Some("Evergreen Health"));
    }

    #[test]
    fn the_fake_virus_warning_warns_even_with_empty_memory() {
        let v = verdict(Some("demo-alert-locked"), POPUP_TITLE, POPUP_TEXT, &[], &Known::default());
        assert!(v.warn, "{v:?}");
        assert!(v.signals.contains(&Signal::FakeWarning { phone: "0800 555 0199".into() }), "{v:?}");
    }

    #[test]
    fn the_real_insurer_page_is_safe() {
        let items = vec![item(1, "edit", "Full name"), item(2, "edit", "Insurance number")];
        let v = verdict(Some("demo-insurer"), INSURER_TITLE, INSURER_TEXT, &items, &known());
        assert!(!v.warn, "{v:?}");
        assert!(v.signals.is_empty(), "{v:?}");
    }

    #[test]
    fn a_known_sites_help_page_with_a_phone_number_is_safe() {
        let text = "Help – Evergreen Health Card locked? Call 0800 555 0100 Mon–Fri 8–20";
        let v = verdict(Some("demo-insurer"), "Help – Evergreen Health", text, &[], &known());
        assert!(!v.warn, "{v:?}");
        assert!(v.signals.is_empty(), "{v:?}");
        // The same text on a site Pip does not know still warns (cold and warm memory).
        for k in [Known::default(), known()] {
            let v = verdict(Some("demo-help-desk"), "Help", text, &[], &k);
            assert!(v.warn, "{v:?}");
            assert!(v.signals.contains(&Signal::FakeWarning { phone: "0800 555 0100".into() }), "{v:?}");
        }
        // The fake virus popup on an unknown site still warns with what memory knows.
        let v = verdict(Some("demo-alert-locked"), POPUP_TITLE, POPUP_TEXT, &[], &known());
        assert!(v.warn, "{v:?}");
    }

    #[test]
    fn a_new_harmless_site_is_safe_cold_and_warm() {
        let items = vec![item(1, "edit", "Student ID"), item(2, "edit", "Postal code")];
        for k in [Known::default(), known()] {
            let v = verdict(Some("demo-uni"), UNI_TITLE, UNI_TEXT, &items, &k);
            assert!(!v.warn, "{v:?}");
            assert!(v.signals.is_empty(), "postal code is no secret: {v:?}");
        }
    }

    #[test]
    fn a_sister_site_that_asks_nothing_secret_is_safe() {
        let v = verdict(Some("demo-insurer-v2"), INSURER_V2_TITLE, INSURER_V2_TEXT, &insurer_v2_items(), &known());
        assert!(!v.warn, "{v:?}");
        // The signs are there, but nothing pushes: no warning.
        assert!(v.signals.iter().any(|s| matches!(s, Signal::ClaimsKnownOrg { .. })), "{v:?}");
        assert!(
            v.signals.contains(&Signal::Lookalike { of: "demo-insurer".into(), org: Some("Evergreen Health".into()) })
        );
    }

    #[test]
    fn a_site_pip_used_before_is_never_a_claim() {
        let mut k = known();
        k.sites.push(site("demo-insurer-v2"));
        let v = verdict(Some("demo-insurer-v2"), ACCOUNT_CHECK_TITLE, ACCOUNT_CHECK_TEXT, &account_items(), &k);
        assert!(!v.warn, "{v:?}");
    }

    #[test]
    fn hosts_lookalikes_and_subdomains() {
        let k = Known {
            orgs: vec![KnownOrg {
                name: "Evergreen Health".into(),
                site: Some("www.evergreen-health.example".into()),
                source: "you told me".into(),
                url: None,
            }],
            sites: vec![],
        };
        let check_host = |host: &str| {
            let items = account_items();
            check(
                &GuardInput {
                    site: Some(host),
                    site_label: host,
                    title: ACCOUNT_CHECK_TITLE,
                    page_text: ACCOUNT_CHECK_TEXT,
                    items: &items,
                },
                &k,
            )
        };
        assert!(check_host("evergreen-health-login.example").warn);
        assert!(check_host("www.evergreen-helth.example").warn);
        assert!(check_host("account-check.example").warn, "claims the org on a strange site");
        assert!(!check_host("secure.evergreen-health.example").warn, "a page of the real domain");
        assert!(!check_host("www.evergreen-health.example").warn);
        let of = Signal::Lookalike { of: "www.evergreen-health.example".into(), org: Some("Evergreen Health".into()) };
        let v = check_host("evergreen-health-login.example");
        assert!(v.signals.contains(&of), "{v:?}");
        let v = check_host("evergreen-health.de");
        assert!(v.warn && v.signals.contains(&of), "the same name under another domain: {v:?}");
        let v = check_host("account-check.example");
        assert!(!v.signals.contains(&of), "another name is no lookalike: {v:?}");
    }

    #[test]
    fn lookalike_rules() {
        assert!(lookalike("demo-insurer-account-check", "demo-insurer"));
        assert!(lookalike("demo-insurer-v2", "demo-insurer"));
        assert!(lookalike("evergreen-helth", "evergreen-health"));
        assert!(lookalike("evergreen-health-login", "evergreen-health"));
        assert!(!lookalike("demo-insurer", "demo-insurer"));
        assert!(!lookalike("demo-uni", "demo-insurer"));
        assert!(!lookalike("insurer", "demo-insurer"), "fewer words is no lookalike");
        assert!(!lookalike("ab", "ac"), "short names never by spelling");
        assert_eq!(key_label("file:///c:/demo/fixtures/demo-insurer"), "demo-insurer");
        assert_eq!(key_label("www.evergreen-health.example"), "www.evergreen-health.example");
    }

    #[test]
    fn sensitive_fields_and_rush_words() {
        let mut pw = item(1, "edit", "");
        pw.is_password = true;
        let field = |name: &str| sensitive_field(&item(1, "edit", name));
        assert_eq!(sensitive_field(&pw).as_deref(), Some("password"));
        assert_eq!(field("SMS code").as_deref(), Some("SMS code"));
        assert_eq!(field("Your PIN").as_deref(), Some("Your PIN"));
        assert_eq!(field("IBAN").as_deref(), Some("IBAN"));
        assert_eq!(field("Card number").as_deref(), Some("Card number"));
        assert!(field("Postal code").is_none());
        assert!(field("Postcode").is_none());
        assert!(field("Discount code").is_none());
        assert!(field("Pinboard note").is_none());
        assert_eq!(field("Bestätigungscode").as_deref(), Some("Bestätigungscode"));
        assert_eq!(field("Passwortbestätigung").as_deref(), Some("Passwortbestätigung"));
        assert!(field("Full name").is_none());
        // A text that mentions a code is no field.
        assert!(sensitive_field(&item(1, "text", "We sent you a code")).is_none());
        assert!(contains_phrase("bitte sofort bestätigen", "sofort"));
        assert!(!contains_phrase("sofortige hilfe", "sofort"));
    }

    /// Memory of the demo with the airline: Beispiel Air's real claim page (demo-air), known from its e-mail.
    fn known_with_airline() -> Known {
        let mut k = known();
        k.orgs.push(KnownOrg {
            name: "Beispiel Air".into(),
            site: Some(site("demo-air")),
            source: "from your e-mail of Sunday, October 4".into(),
            url: Some(format!("{}/index.html", site("demo-air"))),
        });
        k.sites.push(site("demo-air"));
        k
    }

    const AIR_TITLE: &str = "Claim compensation – Beispiel Air";
    const AIR_TEXT: &str = "DEMO PAGE: invented airline, nothing is sent. Beispiel Air Claim compensation for a delayed \
        or cancelled flight Booking reference Flight number Date of flight Full name E-mail IBAN Boarding pass Choose \
        file Send claim";
    const REFUND_CHECK_TITLE: &str = "Beispiel Air – Refund check";
    const REFUND_CHECK_TEXT: &str = "Beispiel Air Refund check Your refund of EUR 400 is ready. Confirm your card within \
        24 hours or the refund will be closed. Card number Expiry date CVC SMS code Get my refund Invented example \
        for tests; nothing is sent.";

    #[test]
    fn the_airlines_real_claim_page_with_its_iban_field_is_silent() {
        let items = vec![
            item(1, "edit", "Booking reference"),
            item(2, "edit", "Full name"),
            item(3, "edit", "IBAN"),
            item(4, "button", "Send claim"),
        ];
        let v = verdict(Some("demo-air"), AIR_TITLE, AIR_TEXT, &items, &known_with_airline());
        assert!(!v.warn, "{v:?}");
        assert_eq!(v.signals, vec![Signal::AsksSensitive { field: "IBAN".into() }], "only the field, no claim");
    }

    #[test]
    fn the_refund_check_lookalike_names_the_real_airline_page() {
        let items =
            vec![item(1, "edit", "Card number"), item(2, "edit", "SMS code"), item(3, "button", "Get my refund")];
        let v = verdict(
            Some("demo-air-refund-check"),
            REFUND_CHECK_TITLE,
            REFUND_CHECK_TEXT,
            &items,
            &known_with_airline(),
        );
        assert!(v.warn, "{v:?}");
        assert_eq!(
            v.signals[0],
            Signal::ClaimsKnownOrg {
                org: "Beispiel Air".into(),
                real_site: "demo-air".into(),
                source: "from your e-mail of Sunday, October 4".into()
            }
        );
        assert!(v.signals.contains(&Signal::Lookalike { of: "demo-air".into(), org: Some("Beispiel Air".into()) }));
        assert!(v.signals.contains(&Signal::Rushes { phrase: "within 24 hours".into() }), "{v:?}");
        assert_eq!(v.claimed_org(), Some("Beispiel Air"));
        // Without the airline in memory the page still asks for a card and rushes, but claims nobody known.
        let cold = verdict(Some("demo-air-refund-check"), REFUND_CHECK_TITLE, REFUND_CHECK_TEXT, &items, &known());
        assert!(cold.claimed_org().is_none(), "{cold:?}");
    }

    #[test]
    fn phone_numbers_not_dates_or_member_numbers() {
        assert_eq!(phone_number("call now: 0800 123 45 67 (free)").as_deref(), Some("0800 123 45 67"));
        assert_eq!(phone_number("ruf an: +49 30 1234567").as_deref(), Some("+49 30 1234567"));
        assert_eq!(phone_number("tel. (0800) 123-4567").as_deref(), Some("(0800) 123-4567"));
        assert!(phone_number("letter of 26.10.2026, member g123456789").is_none());
        assert!(phone_number("amount 1 000 000 eur").is_none());
        // The real insurer's hotline is a phone number, but there is no alarm: safe (see the_real_insurer_page_is_safe).
        assert!(phone_number(&INSURER_TEXT.to_lowercase()).is_some());
    }
}

//! The guard: warning signs that the page in front is not what it says it is. OWNER: agent "agent".
//!
//! Pure rules, no I/O and no model call: the loop gives the page (site key, title, visible text, PAGE ELEMENTS)
//! and what memory knows (the user's organisations with their real site, and the sites Pip used before).
//! Pip never says "scam": it names the warning signs and lets the user decide.
//! Rules of wording (every `guard_` text in i18n; a test there checks them): name the organisation and the concrete
//! risk ("card number", "code", "rush"); never a verdict ("scam", "safe", "Betrug", "guaranteed"); end with "I can't
//! be sure, but I would not type anything here." The user decides.
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
//! - PaymentUnderPressure (needs no memory): fields for at least two of {card number, CVV/CVC/security code,
//!   SMS/one-time code, PIN} AND pressure (a pressure phrase, a countdown like "09:59 remaining", or a small fee of
//!   at most EUR 5 next to "parcel"/"package"/"Paket"/"Zoll"/"customs"). A shop's checkout with card fields and no
//!   pressure stays silent; so does a site Pip used before.
//!
//! Verdict: warn = (ClaimsKnownOrg or Lookalike) and (AsksSensitive or Rushes), or FakeWarning, or
//! PaymentUnderPressure.
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
    /// Fields for a card and a code (their names) under pressure (`phrase`: the pressure phrase, the countdown or
    /// the small fee); `org` is the name the page gives itself (from its title), if any. Needs no memory.
    PaymentUnderPressure { fields: Vec<String>, phrase: String, org: Option<String> },
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

/// The kinds of payment secrets (a field name with one of these words; lowercase). Two kinds or more on one page
/// with pressure = PaymentUnderPressure.
const PAY_KINDS: &[(&str, &[&str])] = &[
    ("card", &["card number", "kartennummer", "credit card", "kreditkarte", "cc-number"]),
    ("cvv", &["cvv", "cvc", "cvv2", "security code", "sicherheitscode", "prüfziffer", "kartenprüfnummer"]),
    ("otp", &["sms", "one-time", "one time", "einmalcode", "tan", "verification code", "bestätigungscode", "otp"]),
    ("pin", &["pin"]),
];

/// Words next to a countdown ("09:59 remaining").
const COUNTDOWN_WORDS: &[&str] = &["remaining", "left", "verbleibend", "übrig", "noch"];

/// Words of a parcel that is "held" for a small fee.
const PARCEL_WORDS: &[&str] =
    &["parcel", "package", "paket", "pakets", "sendung", "zoll", "customs", "redelivery", "zustellung"];

/// A fee next to these words is a shop's shipping cost, not a parcel held for a fee.
const SHOP_WORDS: &[&str] =
    &["shipping", "versandkosten", "subtotal", "zwischensumme", "order total", "warenkorb", "cart"];

/// A held-parcel fee is at most this much (EUR, in cents).
const SMALL_FEE_CENTS: u32 = 500;
/// A fee counts as "next to" a parcel word within this many characters.
const FEE_NEAR: usize = 80;

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

    // Card fields and a code under pressure: a warning sign without any memory (a known site's own page is exempt).
    if !on_known
        && let Some(fields) = payment_fields(input.items)
        && let Some(phrase) = RUSH_PHRASES
            .iter()
            .find(|p| contains_phrase(&page, p))
            .map(|p| (*p).to_string())
            .or_else(|| countdown(&page))
            .or_else(|| small_parcel_fee(&page))
    {
        // One line names both risks, so the field and pressure lines go; the page's own name only when no line
        // names a known organisation already.
        signals.retain(|s| !matches!(s, Signal::AsksSensitive { .. } | Signal::Rushes { .. }));
        let named =
            signals.iter().any(|s| matches!(s, Signal::ClaimsKnownOrg { .. } | Signal::Lookalike { org: Some(_), .. }));
        let org = if named { None } else { org_of_title(input.title) };
        signals.push(Signal::PaymentUnderPressure { fields, phrase, org });
    }

    let has = |f: fn(&Signal) -> bool| signals.iter().any(f);
    let claims = has(|s| matches!(s, Signal::ClaimsKnownOrg { .. } | Signal::Lookalike { .. }));
    let pushes = has(|s| matches!(s, Signal::AsksSensitive { .. } | Signal::Rushes { .. }));
    let warn =
        (claims && pushes) || has(|s| matches!(s, Signal::FakeWarning { .. } | Signal::PaymentUnderPressure { .. }));
    Verdict { warn, signals }
}

/// The bubble line of a PaymentUnderPressure signal: "This page asks for your card number and a code and tries to
/// rush you." plus "It uses the name {org}, but I can't check that it really belongs to {org}." when the page
/// names itself.
pub fn payment_pressure_line(org: Option<&str>, lang: crate::i18n::Lang) -> String {
    use crate::i18n::tf;
    let org_line = org.map(|o| tf("guard_payment_org", lang, &[("org", o)])).unwrap_or_default();
    tf("guard_payment_pressure", lang, &[("org_line", &org_line)]).trim().to_string()
}

/// The names of the payment fields when the page has fields of at least two kinds (`PAY_KINDS`), else None.
fn payment_fields(items: &[OutlineItem]) -> Option<Vec<String>> {
    let mut kinds: Vec<&str> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for item in items.iter().filter(|i| i.takes_text() || i.is_password) {
        let low = item.name.to_lowercase();
        let words: Vec<&str> =
            low.split(|c: char| !c.is_alphanumeric() && c != '-').filter(|w| !w.is_empty()).collect();
        let kind = PAY_KINDS.iter().find_map(|(kind, needles)| {
            needles
                .iter()
                .any(
                    |n| if n.contains(' ') || n.contains('-') { low.contains(n) } else { words.iter().any(|w| w == n) },
                )
                .then_some(*kind)
        });
        let Some(kind) = kind else { continue };
        let name = item.name.split_whitespace().collect::<Vec<_>>().join(" ");
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
        if !name.is_empty() && !names.contains(&name) {
            names.push(name.chars().take(40).collect());
        }
    }
    (kinds.len() >= 2).then_some(names)
}

/// A countdown "9:59" / "09:59" / "0:04:59" within 40 characters of "remaining", "left", "verbleibend", …
fn countdown(page: &str) -> Option<String> {
    let chars: Vec<char> = page.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() || (i > 0 && (chars[i - 1].is_ascii_digit() || chars[i - 1] == ':')) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == ':') {
            j += 1;
        }
        let token: String = chars[i..j].iter().collect();
        let parts: Vec<&str> = token.split(':').collect();
        let clock = parts.len() >= 2
            && parts.iter().all(|p| !p.is_empty())
            && parts[1..].iter().all(|p| p.len() == 2)
            && parts[0].len() <= 2;
        if clock {
            let from = i.saturating_sub(40);
            let to = (j + 40).min(chars.len());
            let near: String = chars[from..to].iter().collect();
            if COUNTDOWN_WORDS.iter().any(|w| contains_phrase(&near, w)) {
                return Some(token);
            }
        }
        i = j;
    }
    None
}

/// A fee of at most EUR 5 ("EUR 1.99", "€ 2,50", "1,99 €", "3.00 EUR") within FEE_NEAR characters of a parcel
/// word, and no shop word (shipping, subtotal, cart) near it. Returns the amount as written.
fn small_parcel_fee(page: &str) -> Option<String> {
    let chars: Vec<char> = page.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() || (i > 0 && (chars[i - 1].is_ascii_digit() || chars[i - 1] == '.')) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && (chars[j].is_ascii_digit() || ((chars[j] == '.' || chars[j] == ',') && j > i)) {
            j += 1;
        }
        let number: String = chars[i..j].iter().collect::<String>().trim_end_matches(['.', ',']).to_string();
        let before: String = chars[i.saturating_sub(5)..i].iter().collect();
        let after: String = chars[j..(j + 5).min(chars.len())].iter().collect();
        let euro = |s: &str| s.contains('€') || s.contains("eur");
        if (euro(&before) || euro(&after))
            && let Some(cents) = cents_of(&number)
            && cents > 0
            && cents <= SMALL_FEE_CENTS
        {
            let from = i.saturating_sub(FEE_NEAR);
            let to = (j + FEE_NEAR).min(chars.len());
            let near: String = chars[from..to].iter().collect();
            let parcel = PARCEL_WORDS.iter().any(|w| contains_phrase(&near, w));
            let shop = SHOP_WORDS.iter().any(|w| contains_phrase(&near, w));
            if parcel && !shop {
                return Some(format!("EUR {number}"));
            }
        }
        i = j.max(i + 1);
    }
    None
}

/// "1.99" / "1,99" / "2" -> cents (a thousands separator is no small fee).
fn cents_of(number: &str) -> Option<u32> {
    let n = number.replace(',', ".");
    let (whole, frac) = n.split_once('.').unwrap_or((&n, ""));
    if frac.len() > 2 || whole.is_empty() || frac.contains('.') {
        return None;
    }
    let whole: u32 = whole.parse().ok()?;
    let frac: u32 = if frac.is_empty() { 0 } else { format!("{frac:0<2}").parse().ok()? };
    whole.checked_mul(100)?.checked_add(frac)
}

/// A title part that is only one of these words names the page, not the organisation ("Checkout – Beispiel
/// Garden" -> "Beispiel Garden", never "It uses the name Checkout").
const PAGE_WORDS: &[&str] = &[
    "checkout",
    "payment",
    "pay",
    "login",
    "log in",
    "sign in",
    "home",
    "welcome",
    "start",
    "error",
    "zahlung",
    "bezahlen",
    "kasse",
    "anmeldung",
    "anmelden",
    "willkommen",
    "startseite",
    "fehler",
    "warenkorb",
    "basket",
    "cart",
];

/// The name a page gives itself in its title: the part around " – ", " - ", " | " or ": " whose words all start
/// with a capital letter ("Beispiel Air – refund center" -> "Beispiel Air"), at most four words, and not just a
/// page word like "Checkout".
fn org_of_title(title: &str) -> Option<String> {
    title
        .split([' ', '\u{a0}'])
        .collect::<Vec<_>>()
        .join(" ")
        .split(" – ")
        .flat_map(|p| p.split(" - "))
        .flat_map(|p| p.split(" | "))
        .flat_map(|p| p.split(": "))
        .map(str::trim)
        .find(|p| {
            let words: Vec<&str> = p.split_whitespace().collect();
            !words.is_empty()
                && words.len() <= 4
                && words.iter().all(|w| w.chars().next().is_some_and(|c| c.is_uppercase() || c.is_ascii_digit()))
                && !words.iter().all(|w| w.chars().all(|c| !c.is_lowercase()) && w.chars().count() > 3)
                && !PAGE_WORDS.contains(&p.to_lowercase().as_str())
        })
        .map(str::to_string)
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
pub(crate) fn contains_phrase(hay: &str, needle: &str) -> bool {
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
        // Card + code under pressure: one line for both risks, no second name (the claim line names the airline).
        assert!(
            v.signals.contains(&Signal::PaymentUnderPressure {
                fields: vec!["Card number".into(), "SMS code".into()],
                phrase: "within 24 hours".into(),
                org: None,
            }),
            "{v:?}"
        );
        assert!(!v.signals.iter().any(|s| matches!(s, Signal::Rushes { .. } | Signal::AsksSensitive { .. })));
        assert_eq!(v.claimed_org(), Some("Beispiel Air"));
        // Without the airline in memory the page still asks for a card and rushes, but claims nobody known.
        let cold = verdict(Some("demo-air-refund-check"), REFUND_CHECK_TITLE, REFUND_CHECK_TEXT, &items, &known());
        assert!(cold.claimed_org().is_none(), "{cold:?}");
    }

    /// fixtures/demo-air-refund-check/index.html as the loop reads it (title, visible text, its four fields).
    const REFUND_CENTER_TITLE: &str = "Beispiel Air – refund center";
    const REFUND_CENTER_TEXT: &str = "BX Beispiel Air – refund center Receive your refund Confirm within 24 hours or \
        your EUR 400 refund expires. Card number Name on card CVV SMS code Get my refund now Invented example for \
        tests. A fake page for the guard eval; nothing is sent.";

    fn refund_center_items() -> Vec<OutlineItem> {
        vec![
            item(1, "edit", "Card number"),
            item(2, "edit", "Name on card"),
            item(3, "edit", "CVV"),
            item(4, "edit", "SMS code"),
            item(5, "button", "Get my refund now"),
        ]
    }

    /// An invented "parcel fee" page (the eval's demo-parcel-fee family): a small customs fee, card + code.
    const PARCEL_TITLE: &str = "Parcel Service – Delivery on hold";
    const PARCEL_TEXT: &str = "Parcel Service Your parcel is on hold. A customs fee of EUR 1.99 is still open. Pay \
        the fee so we can deliver your package. Card number Expiry date CVC Pay EUR 1.99 Invented example for tests.";
    const PAKET_TITLE: &str = "Paketdienst | Zustellung angehalten";
    const PAKET_TEXT: &str = "Paketdienst Ihre Sendung wartet im Lager. Noch 09:59 verbleibend. Kartennummer \
        Prüfziffer Einmalcode Weiter";
    /// An invented shop's checkout: card fields, shipping cost, no pressure (the eval's demo-shop-safe).
    const SHOP_TITLE: &str = "Checkout – Green Leaf Shop";
    const SHOP_TEXT: &str = "Green Leaf Shop Checkout Order summary Tea set EUR 34.95 Shipping EUR 4.95 Order total \
        EUR 39.90 Your package arrives in 2–3 days. Card number Expiry date CVC Name on card Pay now";

    fn card_items(code: &str) -> Vec<OutlineItem> {
        vec![
            item(1, "edit", "Card number"),
            item(2, "edit", "Expiry date"),
            item(3, "edit", code),
            item(4, "button", "Pay"),
        ]
    }

    #[test]
    fn the_refund_check_warns_without_any_memory() {
        let v = verdict(
            Some("demo-air-refund-check"),
            REFUND_CENTER_TITLE,
            REFUND_CENTER_TEXT,
            &refund_center_items(),
            &Known::default(),
        );
        assert!(v.warn, "{v:?}");
        assert!(v.claimed_org().is_none());
        let Some(Signal::PaymentUnderPressure { fields, phrase, org }) =
            v.signals.iter().find(|s| matches!(s, Signal::PaymentUnderPressure { .. }))
        else {
            panic!("{v:?}")
        };
        assert_eq!(fields, &["Card number", "CVV", "SMS code"]);
        assert_eq!(phrase, "within 24 hours");
        assert_eq!(org.as_deref(), Some("Beispiel Air"));
        let line = payment_pressure_line(org.as_deref(), crate::i18n::Lang::En);
        assert_eq!(
            line,
            "This page asks for your card number and a code and tries to rush you. It uses the name Beispiel Air, \
             but I can't check that it really belongs to Beispiel Air."
        );
        // With the airline in memory it still warns (and names the real page first).
        let warm = verdict(
            Some("demo-air-refund-check"),
            REFUND_CENTER_TITLE,
            REFUND_CENTER_TEXT,
            &refund_center_items(),
            &known_with_airline(),
        );
        assert!(warm.warn && warm.claimed_org() == Some("Beispiel Air"), "{warm:?}");
    }

    #[test]
    fn a_parcel_fee_with_card_and_code_warns_cold_and_warm() {
        for k in [Known::default(), known_with_airline()] {
            let v = verdict(Some("demo-parcel-fee"), PARCEL_TITLE, PARCEL_TEXT, &card_items("CVC"), &k);
            assert!(v.warn, "{v:?}");
            assert!(
                v.signals.iter().any(|s| matches!(s, Signal::PaymentUnderPressure { phrase, org, .. }
                    if phrase == "EUR 1.99" && org.as_deref() == Some("Parcel Service"))),
                "{v:?}"
            );
            let items = vec![
                item(1, "edit", "Kartennummer"),
                item(2, "edit", "Prüfziffer"),
                item(3, "edit", "Einmalcode"),
                item(4, "button", "Weiter"),
            ];
            let v = verdict(Some("demo-paket"), PAKET_TITLE, PAKET_TEXT, &items, &k);
            assert!(
                v.warn
                    && v.signals
                        .iter()
                        .any(|s| matches!(s, Signal::PaymentUnderPressure { phrase, .. } if phrase == "09:59")),
                "a countdown: {v:?}"
            );
        }
    }

    #[test]
    fn card_fields_without_pressure_stay_silent() {
        for k in [Known::default(), known_with_airline()] {
            let v = verdict(Some("demo-shop-safe"), SHOP_TITLE, SHOP_TEXT, &card_items("CVC"), &k);
            assert!(!v.warn, "a shop's checkout: {v:?}");
            assert!(!v.signals.iter().any(|s| matches!(s, Signal::PaymentUnderPressure { .. })), "{v:?}");
            // One kind only (card number + expiry): no payment signal even with a rush word.
            let one = vec![item(1, "edit", "Card number"), item(2, "edit", "Expiry date")];
            let v = verdict(Some("demo-shop-safe"), SHOP_TITLE, "Order immediately", &one, &k);
            assert!(!v.warn, "{v:?}");
            let items = vec![item(1, "edit", "Student ID"), item(2, "edit", "Postal code")];
            assert!(!verdict(Some("demo-uni"), UNI_TITLE, UNI_TEXT, &items, &k).warn);
        }
        // The airline's own IBAN page (step 2 of demo-air) and a known shop's own pressure banner stay silent.
        let items = vec![item(1, "edit", "E-mail"), item(2, "edit", "IBAN"), item(3, "button", "Send claim")];
        assert!(!verdict(Some("demo-air"), AIR_TITLE, AIR_TEXT, &items, &known_with_airline()).warn);
        let mut k = Known::default();
        k.sites.push(site("demo-parcel-fee"));
        let v = verdict(Some("demo-parcel-fee"), PARCEL_TITLE, PARCEL_TEXT, &card_items("CVC"), &k);
        assert!(!v.warn, "a site Pip used before: {v:?}");
    }

    // fixtures/demo-parcel-fee and fixtures/demo-shop-safe (the eval's pages) as the loop reads them.
    const POST_TITLE: &str = "Beispiel Post – Your parcel is on hold";
    const POST_TEXT: &str = "BP Beispiel Post Parcel tracking Your parcel BP-4471-0932 is on hold: the customs fee of \
        EUR 1.99 has not been paid. Pay EUR 1.99 within 24 hours or your parcel will be returned Time left: 23:41:09 \
        Card number Expiry date (MM/YY) CVV SMS code We have sent a code to your phone. Enter it to confirm the \
        payment. Pay EUR 1.99 and release my parcel Invented page for tests. Beispiel Post does not exist; nothing \
        is sent.";
    const GARDEN_TITLE: &str = "Beispiel Garden – Checkout";
    const GARDEN_TEXT: &str = "Beispiel Garden Plants and tools Plants Tools Basket (2) Checkout Payment by card Name on \
        the card Card number Expiry date (MM/YY) Security code Pay EUR 23.80 You can change or cancel your order until \
        it is shipped. Your order Garden hose, 15 m EUR 17.90 Seed mix \"Bee meadow\" EUR 1.95 Delivery EUR 3.95 \
        Total EUR 23.80 Invented shop for tests. Beispiel Garden does not exist; nothing is sent.";

    #[test]
    fn the_eval_pages_parcel_fee_warns_and_the_shop_stays_silent() {
        let post = vec![
            item(1, "edit", "Card number"),
            item(2, "edit", "Expiry date (MM/YY)"),
            item(3, "edit", "CVV"),
            item(4, "edit", "SMS code"),
            item(5, "button", "Pay EUR 1.99 and release my parcel"),
        ];
        let garden = vec![
            item(1, "edit", "Name on the card"),
            item(2, "edit", "Card number"),
            item(3, "edit", "Expiry date (MM/YY)"),
            item(4, "edit", "Security code"),
            item(5, "button", "Pay EUR 23.80"),
        ];
        for k in [Known::default(), known_with_airline()] {
            let v = verdict(Some("demo-parcel-fee"), POST_TITLE, POST_TEXT, &post, &k);
            assert!(v.warn, "{v:?}");
            assert!(
                v.signals.contains(&Signal::PaymentUnderPressure {
                    fields: vec!["Card number".into(), "CVV".into(), "SMS code".into()],
                    phrase: "within 24 hours".into(),
                    org: Some("Beispiel Post".into()),
                }),
                "{v:?}"
            );
            let v = verdict(Some("demo-shop-safe"), GARDEN_TITLE, GARDEN_TEXT, &garden, &k);
            assert!(!v.warn, "{v:?}");
            assert!(!v.signals.iter().any(|s| matches!(s, Signal::PaymentUnderPressure { .. })), "{v:?}");
        }
        // The same parcel page without its rush line still warns on the countdown alone.
        let calm = POST_TEXT.replace("within 24 hours", "soon");
        let v = verdict(Some("demo-parcel-fee"), POST_TITLE, &calm, &post, &Known::default());
        assert!(v.warn, "{v:?}");
    }

    #[test]
    fn fees_countdowns_and_page_names() {
        assert_eq!(small_parcel_fee("your parcel: customs fee eur 2.50 open").as_deref(), Some("EUR 2.50"));
        assert_eq!(small_parcel_fee("zollgebühr 1,99 € für ihr paket").as_deref(), Some("EUR 1,99"));
        assert!(small_parcel_fee("your parcel: customs fee eur 25.00").is_none(), "not a small fee");
        assert!(small_parcel_fee("shipping eur 4.95 for your package").is_none(), "a shop's shipping cost");
        assert!(small_parcel_fee("pay eur 1.99 for the app").is_none(), "no parcel");
        assert!(small_parcel_fee("parcel 1.000 eur").is_none());
        assert_eq!(countdown("time left: 4:59").as_deref(), Some("4:59"));
        assert_eq!(countdown("0:09:59 remaining").as_deref(), Some("0:09:59"));
        assert!(countdown("open mon–fri 8:00–20:00").is_none());
        assert!(countdown("4:59 pm").is_none());
        assert_eq!(org_of_title("Beispiel Air – refund center").as_deref(), Some("Beispiel Air"));
        assert_eq!(org_of_title("Claim compensation – Beispiel Air").as_deref(), Some("Beispiel Air"));
        assert_eq!(org_of_title("Paketdienst | Zustellung angehalten").as_deref(), Some("Paketdienst"));
        assert_eq!(org_of_title("Checkout – Beispiel Garden").as_deref(), Some("Beispiel Garden"));
        assert_eq!(org_of_title("Payment | Beispiel Post").as_deref(), Some("Beispiel Post"));
        assert_eq!(org_of_title("Checkout"), None);
        assert_eq!(org_of_title("SYSTEM SECURITY ALERT"), None);
        assert_eq!(org_of_title("confirm your card"), None);
        assert_eq!(
            payment_pressure_line(None, crate::i18n::Lang::En),
            "This page asks for your card number and a code and tries to rush you."
        );
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

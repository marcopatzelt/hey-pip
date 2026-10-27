//! Date fields: recognise a field's date mask ("dd.mm.yyyy", "TT.MM.JJJJ", "mm/dd/yyyy", "yyyy-mm-dd"),
//! read a date the model wrote in any common form, and give the digits to type in the field's own order.
//! Pure helpers for `loop_::type_into_element`. OWNER: agent "agent".

use chrono::NaiveDate;

use crate::platform::OutlineItem;

/// Order of day, month and year in a date field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mask {
    /// dd.mm.yyyy (German, most of Europe)
    Dmy,
    /// mm/dd/yyyy (US)
    Mdy,
    /// yyyy-mm-dd (ISO)
    Ymd,
}

/// A field that takes a date, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateField {
    /// Order of day, month and year.
    pub mask: Mask,
    /// A date input (Chromium shows the mask as its value and moves from day to month to year by itself): the
    /// value takes ISO, the keys digits only. False: a text field with a date name or placeholder, which takes
    /// the written date ("18.10.2026") both ways.
    pub input: bool,
    /// Separator of the mask: '.', '/' or '-'.
    pub sep: char,
}

impl DateField {
    /// What goes in through the UI tree: ISO for a date input (the only form Chromium takes there), the written
    /// date for a text field.
    pub fn value(&self, date: NaiveDate) -> String {
        if self.input { iso(date) } else { self.text(date) }
    }

    /// What to type: digits in the field's order for a date input, the written date for a text field.
    pub fn keys(&self, date: NaiveDate) -> String {
        if self.input { date_keys(self.mask, date) } else { self.text(date) }
    }

    /// The date written in the mask's order with its separator: 18.10.2026, 10/18/2026, 2026-10-18.
    pub fn text(&self, date: NaiveDate) -> String {
        let (a, b, c) = match self.mask {
            Mask::Dmy => (date.format("%d"), date.format("%m"), date.format("%Y")),
            Mask::Mdy => (date.format("%m"), date.format("%d"), date.format("%Y")),
            Mask::Ymd => (date.format("%Y"), date.format("%m"), date.format("%d")),
        };
        format!("{a}{s}{b}{s}{c}", s = self.sep)
    }
}

/// Words in a field name that make an empty text field a date field (then dd.mm.yyyy is assumed). A label
/// names the field ("First day of illness"); its TT.MM.JJJJ placeholder is not in the UI tree. "tag" only in
/// these forms: "Vertrag", "Betrag" and "Antrag" are no dates.
const DATE_WORDS: &[&str] = &[
    "date",
    "datum",
    "since",
    "birth",
    "born",
    "geboren",
    "seit",
    "day",
    "beginn",
    "krankheitstag",
    "erster tag",
    "letzter tag",
];

/// The date field behind an outline item: its value shows a placeholder mask or a date already (a date input),
/// its name carries a mask, or its name says "date" and it is empty (a text field). None for every other field.
pub fn date_field(item: &OutlineItem) -> Option<DateField> {
    if !item.takes_text() || item.is_password {
        return None;
    }
    let value = item.value.as_deref().unwrap_or("").trim();
    if let Some((mask, sep)) = mask_in(value).or_else(|| shape_of(value)) {
        return Some(DateField { mask, input: true, sep });
    }
    if let Some((mask, sep)) = mask_in(&item.name) {
        return Some(DateField { mask, input: false, sep });
    }
    let name = item.name.to_lowercase();
    (value.is_empty() && DATE_WORDS.iter().any(|w| name.contains(w))).then_some(DateField {
        mask: Mask::Dmy,
        input: false,
        sep: '.',
    })
}

/// The date mask of a field (see `date_field`).
pub fn date_mask(item: &OutlineItem) -> Option<Mask> {
    date_field(item).map(|f| f.mask)
}

/// "dd.mm.yyyy" / "TT.MM.JJJJ" / "mm/dd/yyyy" / "yyyy-mm-dd" anywhere in the text, with its separator.
fn mask_in(text: &str) -> Option<(Mask, char)> {
    let lower: Vec<char> = text.to_lowercase().chars().collect();
    let norm: Vec<char> = lower
        .iter()
        .map(|&c| match c {
            '/' | '-' => '.',
            't' => 'd',
            'j' => 'y',
            c => c,
        })
        .collect();
    for (pat, mask, sep_at) in
        [("dd.mm.yyyy", Mask::Dmy, 2), ("mm.dd.yyyy", Mask::Mdy, 2), ("yyyy.mm.dd", Mask::Ymd, 4)]
    {
        let pat: Vec<char> = pat.chars().collect();
        if let Some(i) = norm.windows(pat.len()).position(|w| w == pat.as_slice()) {
            return Some((mask, lower[i + sep_at]));
        }
    }
    None
}

/// A value that is a date already ("20.10.2026", "10/20/2026", "2026-10-20"): its order and separator. A slash
/// means month first (US), a dot or dash day first.
fn shape_of(value: &str) -> Option<(Mask, char)> {
    let tokens: Vec<&str> = value.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty()).collect();
    let [a, _, c] = tokens.as_slice() else { return None };
    if !tokens.iter().all(|s| s.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    let sep = value.trim().chars().nth(a.len())?;
    if !matches!(sep, '.' | '/' | '-') || parse_any_date(value).is_none() {
        return None;
    }
    match (a.len(), c.len()) {
        (4, _) => Some((Mask::Ymd, sep)),
        (_, 4) if sep == '/' => Some((Mask::Mdy, sep)),
        (_, 4) => Some((Mask::Dmy, sep)),
        _ => None,
    }
}

/// A date in any common form: 2026-10-18, 18.10.2026, 18/10/2026 (day first), 18 October 2026,
/// October 18, 2026, 18. Oktober 2026.
pub fn parse_any_date(text: &str) -> Option<NaiveDate> {
    parse_date_in(text, Mask::Dmy)
}

/// Like `parse_any_date`, but a slashed date ("10/18/2026") and bare digits ("10182026") follow the field's
/// mask. A dotted or dashed date is never month first.
pub fn parse_date_in(text: &str, mask: Mask) -> Option<NaiveDate> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let tokens: Vec<&str> = t.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty()).collect();
    match tokens.as_slice() {
        [digits] if digits.len() == 8 && digits.chars().all(|c| c.is_ascii_digit()) => {
            let (a, b, c) = match mask {
                Mask::Ymd => (&digits[0..4], &digits[4..6], &digits[6..8]),
                _ => (&digits[0..2], &digits[2..4], &digits[4..8]),
            };
            let n = |s: &str| s.parse::<u32>().ok();
            match mask {
                Mask::Dmy => ymd(n(c)? as i32, n(b)?, n(a)?),
                Mask::Mdy => ymd(n(c)? as i32, n(a)?, n(b)?),
                Mask::Ymd => ymd(n(a)? as i32, n(b)?, n(c)?),
            }
        }
        [a, b, c] => {
            let num = |s: &str| s.parse::<u32>().ok().filter(|_| s.chars().all(|c| c.is_ascii_digit()));
            match (num(a), num(b), num(c)) {
                (Some(y), Some(m), Some(d)) if a.len() == 4 => ymd(y as i32, m, d),
                (Some(x), Some(y2), Some(year)) if c.len() == 4 => {
                    let slash = t.as_bytes().get(a.len()) == Some(&b'/');
                    let (d, m) = if slash && mask == Mask::Mdy { (y2, x) } else { (x, y2) };
                    // 10/18/2026 cannot be day first: month 18 does not exist.
                    ymd(year as i32, m, d).or_else(|| ymd(year as i32, d, m))
                }
                (Some(d), None, Some(year)) if c.len() == 4 => ymd(year as i32, month_number(b)?, d),
                (None, Some(d), Some(year)) if c.len() == 4 => ymd(year as i32, month_number(a)?, d),
                _ => None,
            }
        }
        _ => None,
    }
}

fn ymd(y: i32, m: u32, d: u32) -> Option<NaiveDate> {
    if !(1900..=2200).contains(&y) {
        return None;
    }
    NaiveDate::from_ymd_opt(y, m, d)
}

/// "October" / "Oct" / "Oktober" / "März" -> 10 / 10 / 10 / 3.
fn month_number(word: &str) -> Option<u32> {
    const MONTHS: [(&str, &str); 12] = [
        ("january", "januar"),
        ("february", "februar"),
        ("march", "märz"),
        ("april", "april"),
        ("may", "mai"),
        ("june", "juni"),
        ("july", "juli"),
        ("august", "august"),
        ("september", "september"),
        ("october", "oktober"),
        ("november", "november"),
        ("december", "dezember"),
    ];
    let w = word.to_lowercase();
    if w.chars().count() < 3 {
        return None;
    }
    MONTHS
        .iter()
        .position(|(en, de)| en.starts_with(&w) || de.starts_with(&w) || (w == "mrz" && *de == "märz"))
        .map(|i| i as u32 + 1)
}

/// The digits to type into a masked date field, in its order: 18102026 (dd.mm.yyyy), 10182026 (mm/dd/yyyy),
/// 20261018 (yyyy-mm-dd). Date inputs move to the next part by themselves.
pub fn date_keys(mask: Mask, date: NaiveDate) -> String {
    let f = match mask {
        Mask::Dmy => "%d%m%Y",
        Mask::Mdy => "%m%d%Y",
        Mask::Ymd => "%Y%m%d",
    };
    date.format(f).to_string()
}

/// The day a field shows, read in its mask's order or as ISO; None for a mask, a half-filled date
/// ("TT.mm.102026") or anything else.
pub fn date_shown(shown: &str, mask: Mask) -> Option<NaiveDate> {
    parse_date_in(shown, mask).or_else(|| parse_date_in(shown, Mask::Ymd))
}

/// A Chromium date input shows its mask while a part is still empty: "TT.MM.JJJJ", "dd.mm.yyyy", also half
/// filled ("TT.mm.102026", "19.mm.jjjj"). Its order, when at least one part is still letters; None for a full
/// date or plain text.
pub fn shows_mask(value: &str) -> Option<Mask> {
    #[derive(PartialEq)]
    enum Part {
        Day,
        Month,
        Year,
        Digits,
    }
    let parts: Vec<Part> = value
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|t| {
            let l = t.to_lowercase();
            if l.len() == 4 && l.chars().all(|c| c.is_ascii_digit()) {
                Some(Part::Year) // a filled year part
            } else if l.chars().all(|c| c.is_ascii_digit()) {
                Some(Part::Digits)
            } else if l == "tt" || l == "dd" {
                Some(Part::Day)
            } else if l == "mm" {
                Some(Part::Month)
            } else if l == "jjjj" || l == "yyyy" || l == "jj" || l == "yy" {
                Some(Part::Year)
            } else {
                None
            }
        })
        .collect::<Option<_>>()?;
    let letters = value.chars().any(|c| c.is_alphabetic());
    if parts.len() != 3 || !letters {
        return None;
    }
    let at = |want: Part| parts.iter().position(|p| *p == want);
    Some(match (at(Part::Day), at(Part::Month), at(Part::Year)) {
        (_, _, Some(0)) => Mask::Ymd,
        (Some(d), Some(m), _) if m < d => Mask::Mdy,
        (_, Some(0), _) => Mask::Mdy,
        (Some(1), _, _) => Mask::Mdy,
        _ => Mask::Dmy,
    })
}

/// yyyy-mm-dd: the only form Chromium date inputs take through their value.
pub fn iso(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::PhysRect;

    fn field(name: &str, value: &str) -> OutlineItem {
        OutlineItem {
            id: 1,
            role: "edit".into(),
            name: name.into(),
            value: Some(value.into()),
            rect: PhysRect { x: 0, y: 0, w: 10, h: 10 },
            offscreen: false,
            enabled: true,
            is_password: false,
        }
    }

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn date_masks() {
        assert_eq!(date_mask(&field("Sick since", "dd.mm.yyyy")), Some(Mask::Dmy));
        assert_eq!(date_mask(&field("Krank seit", "TT.MM.JJJJ")), Some(Mask::Dmy));
        assert_eq!(date_mask(&field("Start", "mm/dd/yyyy")), Some(Mask::Mdy));
        assert_eq!(date_mask(&field("Start", "YYYY-MM-DD")), Some(Mask::Ymd));
        assert_eq!(date_mask(&field("Birth date (dd/mm/yyyy)", "")), Some(Mask::Dmy));
        assert_eq!(date_mask(&field("Date of birth", "")), Some(Mask::Dmy));
        assert_eq!(date_mask(&field("Sick since", "")), Some(Mask::Dmy));
        // A filled field named "date" is not guessed; plain fields are no date fields.
        assert_eq!(date_mask(&field("Date of birth", "Erika")), None);
        assert_eq!(date_mask(&field("Employer", "")), None);
        let mut b = field("Date", "");
        b.role = "button".into();
        assert_eq!(date_mask(&b), None);
    }

    #[test]
    fn date_inputs_and_date_text_fields() {
        let x = d(2026, 10, 18);
        // Chromium date input: the mask is its value. ISO through the value, digits as keys.
        let f = date_field(&field("Sick since", "dd.mm.yyyy")).unwrap();
        assert_eq!(f, DateField { mask: Mask::Dmy, input: true, sep: '.' });
        assert_eq!((f.value(x), f.keys(x)), ("2026-10-18".into(), "18102026".into()));
        let f = date_field(&field("Start", "mm/dd/yyyy")).unwrap();
        assert_eq!((f.value(x), f.keys(x)), ("2026-10-18".into(), "10182026".into()));
        // A date input that already holds a day: its value shows the order.
        let f = date_field(&field("Sick since", "20.10.2026")).unwrap();
        assert_eq!(f, DateField { mask: Mask::Dmy, input: true, sep: '.' });
        assert_eq!(date_field(&field("Start", "10/20/2026")).unwrap().mask, Mask::Mdy);
        assert_eq!(date_field(&field("Start", "2026-10-20")).unwrap().mask, Mask::Ymd);
        assert_eq!(date_field(&field("Street", "Hauptstr. 12")), None);
        assert_eq!(date_field(&field("Number", "12.10.20")), None, "no year");
        // A text field with a date name or placeholder: the written date, both ways.
        let f = date_field(&field("Birth date (dd/mm/yyyy)", "")).unwrap();
        assert_eq!(f, DateField { mask: Mask::Dmy, input: false, sep: '/' });
        assert_eq!((f.value(x), f.keys(x)), ("18/10/2026".into(), "18/10/2026".into()));
        let f = date_field(&field("Date of birth", "")).unwrap();
        assert_eq!(f, DateField { mask: Mask::Dmy, input: false, sep: '.' });
        assert_eq!((f.value(x), f.keys(x)), ("18.10.2026".into(), "18.10.2026".into()));
        let f = date_field(&field("Start (YYYY-MM-DD)", "")).unwrap();
        assert_eq!(f.text(x), "2026-10-18");
    }

    #[test]
    fn text_field_with_a_date_placeholder_gets_the_written_date() {
        // Chromium names an unlabelled text field by its placeholder; a label may carry the pattern too. A
        // labelled field ("First day of illness", demo-insurer questionnaire-v2) hides its placeholder: the label's
        // words have to do.
        for name in [
            "TT.MM.JJJJ",
            "dd.mm.yyyy",
            "Krank seit (TT.MM.JJJJ)",
            "Start date TT.MM.JJJJ",
            "First day of illness",
            "Krankheitsbeginn",
            "Erster Tag der Arbeitsunfähigkeit",
        ] {
            let f = date_field(&field(name, "")).unwrap();
            assert_eq!(f, DateField { mask: Mask::Dmy, input: false, sep: '.' }, "{name}");
            // The model writes ISO or the written date; the field gets 20.10.2026 both ways, never digits only.
            for text in ["2026-10-20", "20.10.2026"] {
                let day = parse_date_in(text, f.mask).unwrap();
                assert_eq!((f.value(day), f.keys(day)), ("20.10.2026".into(), "20.10.2026".into()), "{name} {text}");
            }
        }
        // Number fields with "tag" inside stay plain: eight digits there are a number, never a date.
        for name in ["Vertragsnummer", "Betrag", "Antragsnummer", "Employer"] {
            assert_eq!(date_field(&field(name, "")), None, "{name}");
        }
    }

    #[test]
    fn dates_in_any_form() {
        for t in ["2026-10-18", "18.10.2026", "18/10/2026", "18 October 2026", "October 18, 2026", "18. Oktober 2026"] {
            assert_eq!(parse_any_date(t), Some(d(2026, 10, 18)), "{t}");
        }
        assert_eq!(parse_any_date("Oct 18, 2026"), Some(d(2026, 10, 18)));
        assert_eq!(parse_any_date("3. März 2026"), Some(d(2026, 3, 3)));
        // Day first unless the field is US; a month 18 cannot be meant.
        assert_eq!(parse_any_date("05/10/2026"), Some(d(2026, 10, 5)));
        assert_eq!(parse_date_in("05/10/2026", Mask::Mdy), Some(d(2026, 5, 10)));
        assert_eq!(parse_date_in("05.10.2026", Mask::Mdy), Some(d(2026, 10, 5)), "a dotted date is never US");
        assert_eq!(parse_any_date("10/18/2026"), Some(d(2026, 10, 18)));
        // Digits only, read in the field's order.
        assert_eq!(parse_date_in("18102026", Mask::Dmy), Some(d(2026, 10, 18)));
        assert_eq!(parse_date_in("10182026", Mask::Mdy), Some(d(2026, 10, 18)));
        assert_eq!(parse_date_in("20261018", Mask::Ymd), Some(d(2026, 10, 18)));
        for t in ["", "Erika Beispiel", "G123456789", "dd.mm.yyyy", "31.02.2026", "18.10.26", "Bäckerei 12 GmbH"] {
            assert_eq!(parse_any_date(t), None, "{t}");
        }
    }

    #[test]
    fn masks_shown_by_a_date_input() {
        for v in ["TT.MM.JJJJ", "TT.mm.jjjj", "dd.mm.yyyy", "TT.mm.102026", "19.mm.jjjj", "19.10.jjjj"] {
            assert_eq!(shows_mask(v), Some(Mask::Dmy), "{v}");
        }
        assert_eq!(shows_mask("mm/dd/yyyy"), Some(Mask::Mdy));
        assert_eq!(shows_mask("10/dd/yyyy"), Some(Mask::Mdy));
        assert_eq!(shows_mask("yyyy-mm-dd"), Some(Mask::Ymd));
        assert_eq!(shows_mask("2026-mm-dd"), Some(Mask::Ymd));
        for v in ["19.10.2026", "", "Erika Beispiel", "Hauptstr. 12", "Sick since"] {
            assert_eq!(shows_mask(v), None, "{v}");
        }
        let x = d(2026, 10, 19);
        assert_eq!(date_shown("19.10.2026", Mask::Dmy), Some(x));
        assert_eq!(date_shown("2026-10-19", Mask::Dmy), Some(x));
        assert_eq!(date_shown("TT.mm.102026", Mask::Dmy), None, "half filled is no date");
        assert_eq!(date_shown("19.mm.jjjj", Mask::Dmy), None);
    }

    #[test]
    fn keys_in_the_fields_order() {
        let x = d(2026, 10, 18);
        assert_eq!(date_keys(Mask::Dmy, x), "18102026");
        assert_eq!(date_keys(Mask::Mdy, x), "10182026");
        assert_eq!(date_keys(Mask::Ymd, x), "20261018");
        assert_eq!(iso(x), "2026-10-18");
        assert_eq!(date_keys(Mask::Dmy, d(2026, 3, 5)), "05032026");
    }
}

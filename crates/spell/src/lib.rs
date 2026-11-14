//! Spelling: which words of a paragraph are not in the dictionary, and what to offer instead.
//!
//! The dictionaries are Hunspell files read by `spellbook`. This crate adds what a document editor needs
//! around the lookup: cutting a paragraph into words, leaving alone what is not prose (addresses, file
//! names, abbreviations, identifiers), choosing the dictionary by the script of the word, the user's own
//! words, and offsets in UTF-16 code units (the unit of the editor's text positions).
//!
//! It knows nothing about layout or the document model: the caller hands in the text of one paragraph.

use std::collections::HashSet;

/// A misspelled word: offsets in UTF-16 code units from the start of the checked text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Script {
    Cyrillic,
    Latin,
}

struct Dict {
    lang: String,
    script: Script,
    dict: spellbook::Dictionary,
}

/// Dictionaries, the languages in use and the user's own words.
#[derive(Default)]
pub struct Speller {
    dicts: Vec<Dict>,
    /// `None`: every loaded dictionary is in use.
    active: Option<Vec<String>>,
    /// Lower-cased, with the apostrophe normalised.
    words: HashSet<String>,
}

/// The combining acute accent: a stress mark in Ukrainian and Russian text, not part of the spelling.
const STRESS: char = '\u{301}';

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '\u{2019}' | '\u{2bc}' | '`')
}

fn is_hyphen(c: char) -> bool {
    matches!(c, '-' | '\u{2010}' | '\u{2011}')
}

fn script_of(c: char) -> Option<Script> {
    match c {
        '\u{400}'..='\u{52f}' => Some(Script::Cyrillic),
        'a'..='z' | 'A'..='Z' | '\u{c0}'..='\u{24f}' if c.is_alphabetic() => Some(Script::Latin),
        _ => None,
    }
}

fn script_of_lang(lang: &str) -> Script {
    match lang {
        "uk" | "ru" | "be" | "bg" | "sr" | "mk" | "kk" => Script::Cyrillic,
        _ => Script::Latin,
    }
}

/// The word as dictionaries spell it: no stress marks, a plain apostrophe and hyphen.
fn normalise(word: &str) -> String {
    word.chars()
        .filter(|&c| c != STRESS)
        .map(|c| {
            if is_apostrophe(c) {
                '\''
            } else if is_hyphen(c) {
                '-'
            } else {
                c
            }
        })
        .collect()
}

/// The script of a word, `None` when it mixes scripts or has letters of neither.
fn script_of_word(word: &str) -> Option<Script> {
    let mut found = None;
    for c in word.chars().filter(|c| c.is_alphabetic()) {
        let s = script_of(c)?;
        match found {
            None => found = Some(s),
            Some(f) if f != s => return None,
            Some(_) => {}
        }
    }
    found
}

/// Not prose: an abbreviation (`ДСТУ`, `HTML`) or an identifier in camel case (`JavaScript`, `iPhone`).
fn is_code_like(word: &str) -> bool {
    let letters: Vec<char> = word.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase()) {
        return true;
    }
    letters.windows(2).any(|w| matches!(w, [a, b] if a.is_lowercase() && b.is_uppercase()))
}

/// A stretch between spaces that is an address, a path or a file name rather than words.
fn is_address(chunk: &str) -> bool {
    if chunk.contains('@') || chunk.contains("://") || chunk.contains('\\') || chunk.contains('_') {
        return true;
    }
    let lower = chunk.to_lowercase();
    if lower.starts_with("www.") {
        return true;
    }
    // `report.docx`, `example.com`: a dot between letters with a Latin tail. A missing space after a sentence
    // in Cyrillic text (`кінець.Початок`) is still checked word by word.
    let chars: Vec<char> = chunk.chars().collect();
    chars.windows(3).any(|w| matches!(w, [a, '.', b] if a.is_alphanumeric() && script_of(*b) == Some(Script::Latin)))
}

/// Homoglyphs: Latin letters that look like Cyrillic ones and back. A word typed with one wrong-layout letter
/// looks right on screen and fails every search.
const LOOKALIKES: &[(char, char)] = &[
    ('a', 'а'),
    ('c', 'с'),
    ('e', 'е'),
    ('i', 'і'),
    ('o', 'о'),
    ('p', 'р'),
    ('x', 'х'),
    ('y', 'у'),
    ('A', 'А'),
    ('B', 'В'),
    ('C', 'С'),
    ('E', 'Е'),
    ('H', 'Н'),
    ('I', 'І'),
    ('K', 'К'),
    ('M', 'М'),
    ('O', 'О'),
    ('P', 'Р'),
    ('T', 'Т'),
    ('X', 'Х'),
];

/// The word with every look-alike letter moved to the script most of its letters are in.
fn unmix(word: &str) -> Option<String> {
    let (mut cyr, mut lat) = (0usize, 0usize);
    for c in word.chars() {
        match script_of(c) {
            Some(Script::Cyrillic) => cyr += 1,
            Some(Script::Latin) => lat += 1,
            None => {}
        }
    }
    if cyr == 0 || lat == 0 {
        return None;
    }
    let to_cyrillic = cyr >= lat;
    let fixed: String = word
        .chars()
        .map(|c| {
            LOOKALIKES
                .iter()
                .find_map(|&(l, k)| {
                    if to_cyrillic && c == l {
                        Some(k)
                    } else if !to_cyrillic && c == k {
                        Some(l)
                    } else {
                        None
                    }
                })
                .unwrap_or(c)
        })
        .collect();
    (script_of_word(&fixed).is_some() && fixed != word).then_some(fixed)
}

impl Speller {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads the dictionary of a language (`uk`, `ru`, `en`, …) from the text of its `.aff` and `.dic` files,
    /// replacing an earlier one of the same language.
    pub fn load(&mut self, lang: &str, aff: &str, dic: &str) -> Result<(), String> {
        let dict = spellbook::Dictionary::new(aff, dic).map_err(|e| format!("dictionary {lang}: {e}"))?;
        self.dicts.retain(|d| d.lang != lang);
        self.dicts.push(Dict { lang: lang.to_owned(), script: script_of_lang(lang), dict });
        Ok(())
    }

    pub fn loaded(&self) -> Vec<&str> {
        self.dicts.iter().map(|d| d.lang.as_str()).collect()
    }

    /// Languages to check against. A word is right when any of them (of its script) accepts it; a word in a
    /// script none of them covers is not judged.
    pub fn set_languages<'a>(&mut self, langs: impl IntoIterator<Item = &'a str>) {
        self.active = Some(langs.into_iter().map(str::to_owned).collect());
    }

    /// The user's own words; replaces the previous list. They are matched without regard to case.
    pub fn set_words<'a>(&mut self, words: impl IntoIterator<Item = &'a str>) {
        self.words = words.into_iter().map(|w| normalise(w.trim()).to_lowercase()).filter(|w| !w.is_empty()).collect();
    }

    fn in_use(&self, script: Script) -> impl Iterator<Item = &Dict> {
        self.dicts
            .iter()
            .filter(move |d| d.script == script && self.active.as_ref().is_none_or(|a| a.contains(&d.lang)))
    }

    /// One word without hyphen logic: `None` when no dictionary in use can judge it.
    fn known(&self, word: &str) -> Option<bool> {
        if self.words.contains(&word.to_lowercase()) {
            return Some(true);
        }
        let script = script_of_word(word)?;
        let mut judged = false;
        for d in self.in_use(script) {
            judged = true;
            if d.dict.check(word) {
                return Some(true);
            }
        }
        judged.then_some(false)
    }

    /// Whether a word (as cut by `check_text`, or typed by the caller) is spelled right.
    pub fn check_word(&self, word: &str) -> bool {
        let word = normalise(word);
        let letters = word.chars().filter(|c| c.is_alphabetic()).count();
        if letters < 2 || is_code_like(&word) {
            return true;
        }
        if self.words.contains(&word.to_lowercase()) {
            return true;
        }
        if !word.contains('-') {
            return match script_of_word(&word) {
                Some(_) => self.known(&word).unwrap_or(true),
                // Two scripts in one word is a wrong-layout letter, unless nothing here reads either script.
                None => !self.mixed_is_error(&word),
            };
        }
        // A compound: right as a whole (`будь-який`), or part by part, each in its own script (`web-сторінка`).
        if script_of_word(&word).is_some() && self.known(&word) == Some(true) {
            return true;
        }
        word.split('-').filter(|p| !p.is_empty()).all(|part| {
            let letters = part.chars().filter(|c| c.is_alphabetic()).count();
            if letters < 2 || is_code_like(part) {
                return true;
            }
            match script_of_word(part) {
                Some(_) => self.known(part).unwrap_or(true),
                None => !self.mixed_is_error(part),
            }
        })
    }

    fn mixed_is_error(&self, word: &str) -> bool {
        let has = |s: Script| word.chars().any(|c| script_of(c) == Some(s));
        // Letters of a third script (Greek in a formula written as text) are not ours to judge.
        if word.chars().any(|c| c.is_alphabetic() && script_of(c).is_none()) {
            return false;
        }
        has(Script::Cyrillic)
            && has(Script::Latin)
            && [Script::Cyrillic, Script::Latin].into_iter().any(|s| self.in_use(s).next().is_some())
    }

    /// Misspelled words of one paragraph, in order.
    pub fn check_text(&self, text: &str) -> Vec<Range> {
        let mut out = Vec::new();
        if self.dicts.is_empty() {
            return out;
        }
        let chars: Vec<(u32, char)> = text
            .chars()
            .scan(0u32, |at, c| {
                let start = *at;
                *at += c.len_utf16() as u32;
                Some((start, c))
            })
            .collect();
        let end_of = |i: usize| chars.get(i).map_or(text.encode_utf16().count() as u32, |&(at, _)| at);
        let ch = |i: usize| chars.get(i).map(|&(_, c)| c);
        let mut i = 0;
        while i < chars.len() {
            if ch(i).is_none_or(char::is_whitespace) {
                i += 1;
                continue;
            }
            let chunk_start = i;
            while ch(i).is_some_and(|c| !c.is_whitespace()) {
                i += 1;
            }
            let chunk: String = chars.get(chunk_start..i).unwrap_or_default().iter().map(|&(_, c)| c).collect();
            if is_address(&chunk) {
                continue;
            }
            let mut j = chunk_start;
            while j < i {
                if !ch(j).is_some_and(char::is_alphabetic) {
                    j += 1;
                    continue;
                }
                let start = j;
                // Letters, with an apostrophe, a hyphen or a stress mark only between letters.
                while j < i {
                    let c = ch(j).unwrap_or(' ');
                    let joins =
                        (is_apostrophe(c) || is_hyphen(c)) && ch(j + 1).is_some_and(char::is_alphabetic) && j + 1 < i;
                    if c.is_alphabetic() || c == STRESS || joins {
                        j += 1;
                    } else {
                        break;
                    }
                }
                // `x2`, `3D`, `H2O`: a word glued to a digit is a designation.
                let digit = |k: Option<usize>| k.and_then(ch).is_some_and(|c| c.is_numeric());
                if digit(start.checked_sub(1)) || (j < i && digit(Some(j))) {
                    continue;
                }
                let word: String = chars.get(start..j).unwrap_or_default().iter().map(|&(_, c)| c).collect();
                if !self.check_word(&word) {
                    out.push(Range { start: end_of(start), end: end_of(j) });
                }
            }
        }
        out
    }

    /// Up to `limit` replacements, best first. A word with a wrong-layout letter gets its repaired form first.
    pub fn suggest(&self, word: &str, limit: usize) -> Vec<String> {
        let word = normalise(word);
        let mut out: Vec<String> = Vec::new();
        if let Some(fixed) = unmix(&word)
            && self.check_word(&fixed)
        {
            out.push(fixed);
        }
        let script = script_of_word(&word).or_else(|| unmix(&word).and_then(|w| script_of_word(&w)));
        if let Some(script) = script {
            let mut found = Vec::new();
            for d in self.in_use(script) {
                d.dict.suggest(&word, &mut found);
            }
            for s in found {
                if !out.contains(&s) {
                    out.push(s);
                }
            }
        }
        out.truncate(limit);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AFF: &str = "SET UTF-8\nTRY оаниітврсулдкмпяьзчбегхшжєйїцюфщґ\nSFX A Y 1\nSFX A 0 у .\n";
    const UK: &str = "7\nзвіт/A\nоб'єкт\nтест\nбудь-який\nсторінка\nкінець\nпочаток\n";
    const EN: &str = "3\nhello\nweb\nreport\n";

    fn speller() -> Speller {
        let mut s = Speller::new();
        s.load("uk", AFF, UK).unwrap();
        s.load("en", "SET UTF-8\nTRY esianrtolcdugmphbyfvkwz\n", EN).unwrap();
        s
    }

    fn wrong(s: &Speller, text: &str) -> Vec<String> {
        let units: Vec<u16> = text.encode_utf16().collect();
        s.check_text(text)
            .iter()
            .map(|r| String::from_utf16(&units[r.start as usize..r.end as usize]).unwrap())
            .collect()
    }

    #[test]
    fn flags_only_words_missing_from_the_dictionary() {
        let s = speller();
        assert_eq!(wrong(&s, "Звіт і звітт, звіту тест."), ["звітт"]);
        assert_eq!(wrong(&s, "hello wrold report"), ["wrold"]);
    }

    #[test]
    fn offsets_are_utf16_units() {
        let s = speller();
        // The emoji takes two units, so the word after it starts at 3.
        assert_eq!(s.check_text("😀 звітт"), [Range { start: 3, end: 8 }]);
    }

    #[test]
    fn apostrophes_and_stress_marks_do_not_matter() {
        let s = speller();
        assert!(wrong(&s, "об'єкт об’єкт обʼєкт зві\u{301}т").is_empty());
        // An apostrophe at the edge is a quote mark, not part of the word.
        assert!(wrong(&s, "'тест' ‘звіт’").is_empty());
    }

    #[test]
    fn leaves_what_is_not_prose() {
        let s = speller();
        let text =
            "ДСТУ HTML JavaScript iPhone x2 3D а б user@mail.com https://a.b/c report.docx www.site.ua C:\\dir my_var";
        assert_eq!(wrong(&s, text), Vec::<String>::new());
    }

    #[test]
    fn compounds_are_checked_whole_then_by_parts() {
        let s = speller();
        assert!(wrong(&s, "будь-який web-сторінка звіт-тест IT-тест").is_empty());
        assert_eq!(wrong(&s, "звіт-тесст"), ["звіт-тесст"]);
    }

    #[test]
    fn a_word_in_two_scripts_is_an_error_with_a_repair() {
        let s = speller();
        // Latin "e" inside a Cyrillic word.
        assert_eq!(wrong(&s, "тeст"), ["тeст"]);
        assert_eq!(s.suggest("тeст", 5).first().map(String::as_str), Some("тест"));
    }

    #[test]
    fn languages_and_scripts() {
        let mut s = speller();
        s.set_languages(["uk"]);
        // No dictionary in use reads Latin: Latin words are not judged.
        assert_eq!(wrong(&s, "wrold звітт"), ["звітт"]);
        s.set_languages(["en"]);
        assert_eq!(wrong(&s, "wrold звітт"), ["wrold"]);
        assert!(wrong(&Speller::new(), "будь-що").is_empty());
    }

    #[test]
    fn own_words_are_right_in_any_case() {
        let mut s = speller();
        s.set_words(["Фронтенд", "  ", "коміт"]);
        assert_eq!(wrong(&s, "фронтенд Коміт бекенд"), ["бекенд"]);
        s.set_words([]);
        assert_eq!(wrong(&s, "фронтенд"), ["фронтенд"]);
    }

    #[test]
    fn suggests_from_the_dictionary_of_the_script() {
        let s = speller();
        assert!(s.suggest("звітт", 5).contains(&"звіт".to_owned()));
        assert!(s.suggest("helo", 5).contains(&"hello".to_owned()));
        assert!(s.suggest("звітт", 0).is_empty());
    }

    #[test]
    fn a_missing_space_after_a_sentence_is_still_checked() {
        let s = speller();
        assert_eq!(wrong(&s, "кінець.Початок кінецц.Початок"), ["кінецц"]);
    }
}

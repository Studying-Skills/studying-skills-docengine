//! JavaScript bindings of the speller. Offsets are UTF-16 code units, the unit of a JavaScript string.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
#[derive(Default)]
pub struct Speller(sfdoc_spell::Speller);

#[wasm_bindgen]
impl Speller {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Speller {
        Speller::default()
    }

    /// Loads (or replaces) the dictionary of a language from the text of its Hunspell `.aff` and `.dic`.
    pub fn load(&mut self, lang: &str, aff: &str, dic: &str) -> Result<(), JsValue> {
        self.0.load(lang, aff, dic).map_err(|e| JsValue::from_str(&e))
    }

    /// Languages to check against, comma-separated (`"uk,en"`). A language that is not loaded is skipped.
    #[wasm_bindgen(js_name = setLanguages)]
    pub fn set_languages(&mut self, langs: &str) {
        self.0.set_languages(langs.split(',').map(str::trim).filter(|l| !l.is_empty()));
    }

    /// The user's own words, one per line; replaces the previous list.
    #[wasm_bindgen(js_name = setWords)]
    pub fn set_words(&mut self, words: &str) {
        self.0.set_words(words.lines());
    }

    /// Misspelled words of a paragraph as a flat list `[start, end, start, end, …]`.
    pub fn check(&self, text: &str) -> Vec<u32> {
        self.0.check_text(text).into_iter().flat_map(|r| [r.start, r.end]).collect()
    }

    /// Up to `limit` replacements of a word, one per line, best first.
    pub fn suggest(&self, word: &str, limit: usize) -> String {
        self.0.suggest(word, limit).join("\n")
    }
}

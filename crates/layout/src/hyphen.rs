//! Where a word may be broken at the end of a line: Ukrainian and Russian words, by syllable rules.
//!
//! No dictionaries: a break goes between syllables, never leaves one letter on a line, keeps the soft sign and the
//! apostrophe with the letter before them, and does not tear «дж» and «дз» apart. Words in other alphabets,
//! abbreviations in capitals and words with digits are not broken.

fn is_vowel(c: char) -> bool {
    matches!(
        c.to_lowercase().next().unwrap_or(c),
        'а' | 'е' | 'є' | 'и' | 'і' | 'ї' | 'о' | 'у' | 'ю' | 'я' | 'ы' | 'э' | 'ё'
    )
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '’' | 'ʼ')
}

/// Letters that belong to the letter before them.
fn is_sign(c: char) -> bool {
    matches!(c, 'ь' | 'ъ' | 'Ь' | 'Ъ') || is_apostrophe(c)
}

fn is_cyrillic(c: char) -> bool {
    matches!(c, '\u{400}'..='\u{4FF}')
}

/// A letter of a word this module knows how to break.
pub fn is_word_char(c: char) -> bool {
    is_cyrillic(c) || is_apostrophe(c)
}

/// Byte offsets inside `word` where it may be broken, in ascending order.
pub fn points(word: &str) -> Vec<usize> {
    let chars: Vec<(usize, char)> = word.char_indices().collect();
    let n = chars.len();
    let letter = |i: usize| chars.get(i).map_or(' ', |c| c.1);
    if n < 5 || !chars.iter().all(|c| is_word_char(c.1)) {
        return Vec::new();
    }
    // An abbreviation: every letter a capital.
    if chars.iter().filter(|c| is_cyrillic(c.1)).all(|c| c.1.is_uppercase()) {
        return Vec::new();
    }
    let vowels: Vec<usize> = (0..n).filter(|i| is_vowel(letter(*i))).collect();
    let mut out = Vec::new();
    for pair in vowels.windows(2) {
        let (Some(&i), Some(&j)) = (pair.first(), pair.get(1)) else { continue };
        let between = j - i - 1;
        if between == 0 {
            continue;
        }
        let lower = |k: usize| letter(k).to_lowercase().next().unwrap_or(' ');
        // One consonant goes to the next syllable; of several the first stays, unless they are «дж» or «дз».
        let digraph = between == 2 && lower(i + 1) == 'д' && matches!(lower(i + 2), 'ж' | 'з');
        let mut at = if between == 1 || digraph { i + 1 } else { i + 2 };
        // «й» closes the syllable before it: рай-он.
        if at == i + 1 && lower(i + 1) == 'й' {
            at = i + 2;
        }
        while at < j && is_sign(letter(at)) {
            at += 1;
        }
        if at >= 2
            && n - at >= 2
            && at <= j
            && !is_sign(letter(at))
            && let Some((b, _)) = chars.get(at)
        {
            out.push(*b);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::points;

    fn broken(word: &str) -> String {
        let mut out = String::new();
        let at = points(word);
        for (b, c) in word.char_indices() {
            if at.contains(&b) {
                out.push('-');
            }
            out.push(c);
        }
        out
    }

    #[test]
    fn words_break_between_syllables() {
        assert_eq!(broken("сортування"), "сор-ту-ван-ня");
        assert_eq!(broken("алгоритми"), "ал-го-рит-ми");
        assert_eq!(broken("результати"), "ре-зуль-та-ти");
        assert_eq!(broken("пальці"), "паль-ці");
        assert_eq!(broken("район"), "рай-он");
        assert_eq!(broken("комп'ютер"), "ком-п'ю-тер");
        assert_eq!(broken("об’єднання"), "об’-єд-нан-ня");
        assert_eq!(broken("ходжу"), "хо-джу");
        assert_eq!(broken("Програмування"), "Прог-ра-му-ван-ня");
    }

    #[test]
    fn one_letter_never_stays_alone_and_short_or_foreign_words_are_left_whole() {
        // «о-лі-я» would leave one letter on a line at either end.
        assert_eq!(broken("олія"), "олія");
        assert_eq!(broken("екран"), "ек-ран");
        assert_eq!(broken("який"), "який");
        assert_eq!(broken("ДСТУ"), "ДСТУ");
        assert_eq!(broken("НАВЧАННЯ"), "НАВЧАННЯ");
        assert_eq!(broken("quicksort"), "quicksort");
        assert_eq!(broken("слово2"), "слово2");
        assert_eq!(broken(""), "");
    }
}

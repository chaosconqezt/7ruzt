//! Выделение по маске (Grey + / Grey - в 7-Zip FM).
//!
//! Маска — привычные `*` (любая последовательность) и `?` (один символ),
//! несколько масок через пробел: `*.rs *.toml`. Сравнение регистронезависимое,
//! как в имени колонки «Имя»: `IMAGE*.PNG` выделяет `image01.png`.

/// Совпадает ли имя с маской (или с любой из масок, разделённых пробелами).
pub fn mask_matches(name: &str, mask: &str) -> bool {
    mask.split_whitespace()
        .any(|pattern| wildcard_match(name, pattern))
}

/// Одна маска: жадный проход с возвратом к последней `*` при несовпадении.
fn wildcard_match(name: &str, pattern: &str) -> bool {
    let name: Vec<char> = name.chars().flat_map(|ch| ch.to_lowercase()).collect();
    let pattern: Vec<char> = pattern.chars().flat_map(|ch| ch.to_lowercase()).collect();

    let (mut n, mut p) = (0, 0);
    // Позиция последней `*` в маске и граница имени, которую она уже съела.
    let mut star: Option<usize> = None;
    let mut star_n = 0;
    while n < name.len() {
        match pattern.get(p) {
            Some('?') => {
                n += 1;
                p += 1;
            }
            Some('*') => {
                star = Some(p);
                star_n = n;
                p += 1;
            }
            Some(&ch) if ch == name[n] => {
                n += 1;
                p += 1;
            }
            // Не совпало — последняя `*` съедает ещё один символ имени.
            _ => match star {
                Some(position) => {
                    star_n += 1;
                    n = star_n;
                    p = position + 1;
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&ch| ch == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_matches_any_sequence() {
        assert!(mask_matches("main.rs", "*.rs"));
        assert!(mask_matches("image01.png", "image*.png"));
        assert!(mask_matches("image.png", "image*.png"));
        assert!(mask_matches("anything", "*"));
        assert!(mask_matches("", "*"));
        assert!(!mask_matches("main.toml", "*.rs"));
        // `*` в середине и несколько звёзд.
        assert!(mask_matches("aXbYc", "a*b*c"));
        assert!(mask_matches("ac", "a**c"));
        assert!(!mask_matches("ab", "a*c"));
    }

    #[test]
    fn question_matches_exactly_one_char() {
        assert!(mask_matches("file1.txt", "file?.txt"));
        assert!(!mask_matches("file10.txt", "file?.txt"));
        assert!(!mask_matches("file.txt", "file?.txt"));
        assert!(mask_matches("файл1", "?????"));
    }

    #[test]
    fn match_is_case_insensitive() {
        assert!(mask_matches("IMAGE01.PNG", "image*.png"));
        assert!(mask_matches("main.RS", "*.rs"));
        assert!(mask_matches("Отчёт.PDF", "*.pdf"));
    }

    #[test]
    fn several_masks_are_split_by_whitespace() {
        assert!(mask_matches("main.rs", "*.cpp *.rs"));
        assert!(mask_matches("lib.cpp", "*.cpp\t*.rs"));
        assert!(!mask_matches("main.h", "*.cpp *.rs"));
        // Пробел — разделитель масок, а не часть имени.
        assert!(mask_matches("b.txt", "a b.txt"));
        assert!(!mask_matches("a b.txt", "a b.txt"));
    }

    #[test]
    fn empty_mask_matches_nothing() {
        assert!(!mask_matches("main.rs", ""));
        assert!(!mask_matches("main.rs", "   "));
    }

    #[test]
    fn dot_is_literal_not_dos_wildcard() {
        // `*.*` требует точку: имя без расширения не совпадает.
        assert!(mask_matches("main.rs", "*.*"));
        assert!(!mask_matches("README", "*.*"));
        assert!(mask_matches(".gitignore", "*.*"));
        assert!(mask_matches(".gitignore", ".*"));
    }

    #[test]
    fn trailing_star_backtracks_correctly() {
        assert!(mask_matches("aaaab", "*aab"));
        assert!(mask_matches("ababab", "*ab"));
        assert!(!mask_matches("aaaab", "*ac"));
    }
}

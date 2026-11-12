//! Одна запись листинга: файл в каталоге или элемент внутри архива.
//!
//! Один тип на оба источника намеренно: UI не должен знать, откуда пришли строки,
//! иначе для архива и для папки появятся две таблицы и два набора сортировок.

use std::cmp::Ordering;
use std::time::SystemTime;

use chrono::{DateTime, Local};

/// Запись листинга.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Имя без пути — ровно то, что видно в колонке «Имя».
    pub name: String,
    pub size: u64,
    /// Сжатый размер (только у архивов).
    pub packed: Option<u64>,
    /// Уже отформатировано под колонку: `2026-11-06 14:00`.
    pub modified: Option<String>,
    pub created: Option<String>,
    pub attributes: Option<String>,
    pub crc: Option<String>,
    /// Содержимое зашифровано (`Encrypted = +` в `-slt`). Нужно, чтобы спросить
    /// пароль до проверки архива: без пароля 7z упирается в stdin.
    pub encrypted: bool,
    pub is_dir: bool,
}

impl Entry {
    pub fn dir(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            size: 0,
            packed: None,
            modified: None,
            created: None,
            attributes: Some("D".to_string()),
            crc: None,
            encrypted: false,
            is_dir: true,
        }
    }

    pub fn file(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            size: 0,
            packed: None,
            modified: None,
            created: None,
            attributes: Some("A".to_string()),
            crc: None,
            encrypted: false,
            is_dir: false,
        }
    }

    /// Размер в виде `1 659 704` — с пробелами между разрядами, как в 7-Zip.
    pub fn size_text(&self) -> String {
        group_digits(self.size)
    }

    /// Сжатый размер, если известен.
    pub fn packed_text(&self) -> String {
        self.packed.map(group_digits).unwrap_or_default()
    }
}

/// `1234567` → `1 234 567`. Каталоги размера не показывают, поэтому вызывать по месту.
pub fn group_digits(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);

    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

/// `8284` → `8 KiB` — целые KiB/MiB/GiB/TiB, как в колонке размера 7-Zip.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes >> 10;
    let mut unit = UNITS[0];
    for next in UNITS.iter().skip(1) {
        if value < 1024 {
            break;
        }
        value >>= 10;
        unit = next;
    }
    format!("{value} {unit}")
}

/// Системное время → `2026-11-06 14:00` в локальной зоне.
pub fn format_time(time: SystemTime) -> String {
    let local: DateTime<Local> = time.into();
    local.format("%Y-%m-%d %H:%M").to_string()
}

/// Уже отформатированная строка из вывода 7-Zip → `2026-11-06 14:00`.
///
/// 7-Zip не стесняется отдавать дробные секунды (`2026-11-09 17:20:09.4759984`),
/// а в колонке они не нужны и только ломают ширину.
pub fn trim_seconds(raw: &str) -> String {
    let trimmed = raw.trim();
    let looks_like_date = trimmed.len() >= 16 && trimmed.as_bytes().get(4) == Some(&b'-');
    // Срез байтовый: если 16-й байт попал в середину многобайтового символа,
    // `&s[..16]` паникует. «26 год» гарантирует ASCII только до 5-го байта.
    if looks_like_date && trimmed.is_char_boundary(16) {
        trimmed[..16].to_string()
    } else {
        trimmed.to_string()
    }
}

/// Колонка, по которой сортируем.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    /// Расширение файла, внутри одного расширения — по имени.
    Type,
    Size,
    Modified,
    Created,
    Comment,
}

impl SortKey {
    /// Порядковый номер — нужен UI для стабильных `ElementId` на заголовках колонок.
    pub fn index(self) -> usize {
        match self {
            Self::Name => 0,
            Self::Size => 1,
            Self::Modified => 2,
            Self::Created => 3,
            Self::Comment => 4,
            Self::Type => 5,
        }
    }

    /// Следующая колонка по кругу.
    pub fn next(self) -> Self {
        match self {
            Self::Name => Self::Type,
            Self::Type => Self::Size,
            Self::Size => Self::Modified,
            Self::Modified => Self::Created,
            Self::Created => Self::Comment,
            Self::Comment => Self::Name,
        }
    }
}

/// Расширение имени: часть после последней точки.
///
/// Точка в начале — не расширение (`.gitignore` — имя целиком, как в проводнике),
/// у `README` и `file.` расширения нет. Регистр сохраняется: сравнение — на месте.
pub fn file_extension(name: &str) -> &str {
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[dot + 1..],
        _ => "",
    }
}

/// Сортировка «как в проводнике»: каталоги всегда выше файлов, независимо от колонки.
pub fn sort_entries(entries: &mut [Entry], key: SortKey, ascending: bool) {
    entries.sort_by(|a, b| {
        let by_kind = b.is_dir.cmp(&a.is_dir);
        if by_kind != Ordering::Equal {
            return by_kind;
        }

        let ordering = match key {
            SortKey::Name => natural_cmp(&a.name, &b.name),
            SortKey::Type => {
                let by_ext = file_extension(&a.name)
                    .chars()
                    .flat_map(char::to_lowercase)
                    .cmp(file_extension(&b.name).chars().flat_map(char::to_lowercase));
                by_ext.then_with(|| natural_cmp(&a.name, &b.name))
            }
            SortKey::Size => a.size.cmp(&b.size),
            SortKey::Modified => a.modified.cmp(&b.modified),
            SortKey::Created => a.created.cmp(&b.created),
            SortKey::Comment => Ordering::Equal,
        };

        if ascending {
            ordering
        } else {
            ordering.reverse()
        }
    });
}

/// Сравнение имён с учётом чисел: `file2` раньше `file10`.
/// Регистронезависимо — иначе `ZZZ.txt` уедет выше `aaa.txt`, и это выглядит сломанным.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut left = a.chars().peekable();
    let mut right = b.chars().peekable();

    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(l), Some(r)) if l.is_ascii_digit() && r.is_ascii_digit() => {
                let l_num = take_digits(&mut left);
                let r_num = take_digits(&mut right);
                match l_num.cmp(&r_num) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
            (Some(l), Some(r)) => {
                let lower = l.to_lowercase().cmp(r.to_lowercase());
                if lower != Ordering::Equal {
                    return lower;
                }
                left.next();
                right.next();
            }
        }
    }
}

fn take_digits(iter: &mut std::iter::Peekable<std::str::Chars<'_>>) -> u128 {
    let mut value: u128 = 0;
    while let Some(ch) = iter.peek().copied() {
        if !ch.is_ascii_digit() {
            break;
        }
        value = value
            .saturating_mul(10)
            .saturating_add(u128::from(ch as u8 - b'0'));
        iter.next();
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_digits_by_three() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(7), "7");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(1000), "1 000");
        assert_eq!(group_digits(1_659_704), "1 659 704");
        assert_eq!(group_digits(123_456_789), "123 456 789");
    }

    #[test]
    fn human_size_uses_whole_units_like_7zip() {
        assert_eq!(human_size(0), "0 KiB");
        assert_eq!(human_size(8284), "8 KiB");
        assert_eq!(human_size(1024), "1 KiB");
        assert_eq!(human_size(5 * 1024 * 1024), "5 MiB");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3 GiB");
        assert_eq!(human_size(2 * 1024_u64.pow(4)), "2 TiB");
    }

    #[test]
    fn trims_seconds_from_7zip_time() {
        assert_eq!(trim_seconds("2026-11-06 14:00:00"), "2026-11-06 14:00");
        assert_eq!(trim_seconds("2018-01-28 12:00"), "2018-01-28 12:00");
        // Windows-сборка 7-Zip отдаёт доли секунды.
        assert_eq!(
            trim_seconds("2026-11-09 17:20:09.4759984"),
            "2026-11-09 17:20"
        );
        assert_eq!(trim_seconds(""), "");
    }

    #[test]
    fn does_not_slice_multibyte_characters() {
        // 16-й байт попадает в середину «é» — раньше это был бы паника.
        let raw = "2026-11-06 14:0é";
        assert_eq!(trim_seconds(raw), raw);
        // Совсем не дата — не трогаем.
        assert_eq!(trim_seconds("файл.текст"), "файл.текст");
    }

    #[test]
    fn natural_order_puts_file2_before_file10() {
        let mut items = vec![
            Entry::file("file10.txt"),
            Entry::file("file2.txt"),
            Entry::file("file1.txt"),
        ];
        sort_entries(&mut items, SortKey::Name, true);
        let names: Vec<&str> = items.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["file1.txt", "file2.txt", "file10.txt"]);
    }

    #[test]
    fn dirs_go_first_and_stay_first_when_desc() {
        let mut items = vec![
            Entry::file("a.txt"),
            Entry::dir("zdir"),
            Entry::file("b.txt"),
        ];
        sort_entries(&mut items, SortKey::Name, false);
        assert_eq!(items[0].name, "zdir");
        assert!(items[0].is_dir);
    }

    #[test]
    fn case_is_ignored_in_name_order() {
        let mut items = vec![Entry::file("bbb.txt"), Entry::file("AAA.txt")];
        sort_entries(&mut items, SortKey::Name, true);
        assert_eq!(items[0].name, "AAA.txt");
    }

    #[test]
    fn extension_splits_like_explorer() {
        assert_eq!(file_extension("main.rs"), "rs");
        assert_eq!(file_extension("archive.tar.gz"), "gz");
        assert_eq!(file_extension("README"), "");
        assert_eq!(file_extension(".gitignore"), "");
        assert_eq!(file_extension("file."), "");
        assert_eq!(file_extension(""), "");
    }

    #[test]
    fn type_sort_groups_by_extension_then_name() {
        let mut items = vec![
            Entry::file("b.txt"),
            Entry::file("a.rs"),
            Entry::file("c.txt"),
            Entry::file("a.PNG"),
            Entry::file("b.png"),
        ];
        sort_entries(&mut items, SortKey::Type, true);
        let names: Vec<&str> = items.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a.PNG", "b.png", "a.rs", "b.txt", "c.txt"]);
    }

    #[test]
    fn type_sort_ties_use_natural_name_order() {
        let mut items = vec![
            Entry::file("file10.rs"),
            Entry::file("file2.rs"),
            Entry::file("file1.rs"),
        ];
        sort_entries(&mut items, SortKey::Type, true);
        let names: Vec<&str> = items.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["file1.rs", "file2.rs", "file10.rs"]);
    }

    #[test]
    fn type_sort_keeps_dirs_first_and_extensionless_together() {
        let mut items = vec![
            Entry::file("z.txt"),
            Entry::dir("docs"),
            Entry::file("README"),
            Entry::file("LICENSE"),
            Entry::file("a.txt"),
        ];
        sort_entries(&mut items, SortKey::Type, false);
        let names: Vec<&str> = items.iter().map(|e| e.name.as_str()).collect();
        // Каталоги сверху даже при убывании; без расширения — одна группа.
        assert_eq!(names, ["docs", "z.txt", "a.txt", "README", "LICENSE"]);
    }
}

//! Разбор `7z l -slt` — единственный способ узнать содержимое архива без своего парсера 7z.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::entry::{Entry, trim_seconds};

/// Результат листинга архива.
#[derive(Debug, Clone)]
pub struct Listing {
    pub archive: PathBuf,
    /// `7z`, `zip`, … — то, что 7-Zip написал в `Type`.
    pub kind: Option<String>,
    /// Уже «вид из корня архива»: вложенные пути свёрнуты в каталоги.
    pub entries: Vec<Entry>,
    /// Есть хоть один зашифрованный элемент. Проверка такого архива без пароля
    /// не пройдёт, поэтому UI спрашивает пароль заранее.
    pub encrypted: bool,
}

#[derive(Debug)]
pub enum ListError {
    Spawn {
        exe: PathBuf,
        source: std::io::Error,
    },
    Failed {
        code: i32,
        stderr: String,
    },
    Empty,
}

impl std::fmt::Display for ListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn { exe, source } => {
                write!(f, "не удалось запустить `{}`: {source}", exe.display())
            }
            Self::Failed { code, stderr } => {
                let text = stderr.trim();
                if text.is_empty() {
                    write!(f, "7z завершился с кодом {code}")
                } else {
                    write!(f, "7z завершился с кодом {code}: {text}")
                }
            }
            Self::Empty => write!(f, "7z не вернул ни одной записи"),
        }
    }
}

impl std::error::Error for ListError {}

/// Запрашивает список файлов архива.
///
/// Минимальная версия 7z (21.07, есть `-sccUTF-8`) гарантируется [`Locator`],
/// поэтому флаг передаётся всегда, без запасного пути в системной кодировке.
pub fn list_archive(exe: &Path, archive: &Path) -> Result<Listing, ListError> {
    let output = run(exe, archive)?;
    if !output.status.success() {
        // Код 1 — предупреждения, а не отказ: листинг повреждённого архива
        // может быть частичным, но уже распарсенным — не выбрасываем его.
        if output.status.code() == Some(1)
            && let Ok(listing) = parse_archive_output(archive, &output.stdout)
        {
            return Ok(listing);
        }
        return Err(ListError::Failed {
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    parse_archive_output(archive, &output.stdout)
}

fn run(exe: &Path, archive: &Path) -> Result<std::process::Output, ListError> {
    let mut command = Command::new(exe);
    command.arg("l").arg("-slt").arg("-bd").arg("-sccUTF-8");
    // `--` обязателен: путь может начинаться с дефиса и быть принят за ключ.
    command.arg("--").arg(archive);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    // Пароль не передаём: на шифрованном архиве 7z должен упасть, а не ждать ввод.
    command.stdin(std::process::Stdio::null());

    command.output().map_err(|source| ListError::Spawn {
        exe: exe.to_path_buf(),
        source,
    })
}

fn parse_archive_output(archive: &Path, raw: &[u8]) -> Result<Listing, ListError> {
    let text = String::from_utf8_lossy(raw);
    let (props, entries) = parse_slt(&text);

    if entries.is_empty() && props.is_empty() {
        return Err(ListError::Empty);
    }

    Ok(Listing {
        archive: archive.to_path_buf(),
        kind: props
            .iter()
            .find(|(k, _)| k == "Type")
            .map(|(_, v)| v.clone()),
        encrypted: entries.iter().any(|entry| entry.encrypted),
        // Отдаём плоский перечень как есть: какой уровень показать, решает вызывающий
        // через `archive::view_at`.
        entries,
    })
}

/// Разбирает `-slt` на два куска: свойства архива и записи.
///
/// Формат: блоки `Ключ = Значение` через пустую строку, секция записей отделена
pub fn parse_slt(text: &str) -> (Vec<(String, String)>, Vec<Entry>) {
    let mut blocks: Vec<Vec<(String, String)>> = Vec::new();
    let mut current: Vec<(String, String)> = Vec::new();
    let mut in_entries = false;

    for line in text.lines() {
        // Только \r от CRLF: пробелы по краям могут быть частью имени файла.
        let line = line.trim_end_matches('\r');

        // Маркер секции записей встречается один раз, до записей. Внутри секции
        // файл с именем «----------» — законное имя, а не маркер.
        if !in_entries && line.trim_end().starts_with("----------") {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
            in_entries = true;
            continue;
        }

        if line.is_empty() {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
            continue;
        }

        match split_key(line) {
            Some((key, value)) => current.push((key.to_string(), value.to_string())),
            // Не похоже на пару «Ключ = Значение» — значит это продолжение
            // предыдущего значения: 7-Zip печатает имена с переводами строк как есть.
            None => {
                if let Some((_, value)) = current.last_mut() {
                    value.push('\n');
                    value.push_str(line);
                }
            }
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }

    if !in_entries {
        return (Vec::new(), Vec::new());
    }

    let props = blocks.first().cloned().unwrap_or_default();
    let entries = blocks
        .iter()
        .skip(1)
        .filter_map(|block| entry_from_block(block))
        .collect();
    (props, entries)
}

/// Делит строку на пару «Ключ = Значение» по разделителю `" = "`.
///
/// Значение берётся **как есть**, без `trim`: краевые пробелы могут быть частью
/// имени файла, и обрезав их мы получим путь, которого в архиве нет.
/// Ключ обязан выглядеть как ключ 7-Zip: буквы, цифры, пробелы, подчёркивания.
fn split_key(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(" = ")?;
    let looks_like_key = !key.is_empty()
        && key.len() <= 40
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == ' ' || ch == '_');
    looks_like_key.then_some((key.trim(), value))
}

fn entry_from_block(block: &[(String, String)]) -> Option<Entry> {
    let raw_name = block
        .iter()
        .find(|(key, _)| key == "Path")
        .map(|(_, value)| value.clone())?;

    // Windows-сборка 7-Zip пишет разделители как `docs\img\`. Для свёртки в корень
    // и для единообразия с unix-сборками нормализуем к прямому слэшу.
    let name = raw_name.replace('\\', "/");

    let is_dir = block
        .iter()
        .find(|(key, _)| key == "Folder")
        .map(|(_, value)| value == "+")
        .unwrap_or_else(|| {
            block
                .iter()
                .find(|(key, _)| key == "Attributes")
                // Unix-архивы пишут POSIX-строку (`drwxr-xr-x`), где 'd' строчная.
                .map(|(_, value)| value.trim_start().to_ascii_uppercase().starts_with('D'))
                .unwrap_or(false)
        });

    if is_dir {
        let mut entry = Entry::dir(name);
        entry.attributes = find(block, "Attributes");
        entry.modified = find(block, "Modified").map(|value| trim_seconds(&value));
        entry.created = find(block, "Created").map(|value| trim_seconds(&value));
        entry.encrypted = is_encrypted(block);
        return Some(entry);
    }

    let mut entry = Entry::file(name);
    entry.size = find(block, "Size")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    entry.packed = find(block, "Packed Size").and_then(|value| value.parse().ok());
    entry.modified = find(block, "Modified").map(|value| trim_seconds(&value));
    entry.created = find(block, "Created").map(|value| trim_seconds(&value));
    entry.attributes = find(block, "Attributes");
    entry.crc = find(block, "CRC");
    entry.encrypted = is_encrypted(block);
    Some(entry)
}

/// `Encrypted = +` — 7-Zip зашифровал содержимое элемента.
fn is_encrypted(block: &[(String, String)]) -> bool {
    find(block, "Encrypted").as_deref() == Some("+")
}

fn find(block: &[(String, String)], key: &str) -> Option<String> {
    block
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
7-Zip 26.06 (x64) : Copyright (c) 1999-2026 Igor Pavlov : 2026-11-06

Scanning the drive for archives:
1 file, 1337 bytes (2 KiB)

Listing archive: sample.7z

--
Path = sample.7z
Type = 7z
Physical Size = 1337
Headers Size = 200
Method = LZMA2:24
Solid = -
Blocks = 1

----------
Path = docs
Size = 0
Packed Size = 0
Modified = 2026-11-06 14:00:00
Attributes = D_ drwxr-xr-x
Folder = +

Path = docs/readme.txt
Size = 1711
Packed Size = 700
Modified = 2026-11-06 14:01:22
Created = 2026-11-06 13:00:00
Attributes = A
CRC = 1234ABCD
Folder = -

Path = a.bin
Size = 585
Packed Size = 400
Modified = 2026-11-06 14:02:00
Attributes = A_ -rw-r--r--
CRC = DEADBEEF
Folder = -
";

    #[test]
    fn parses_props_and_entries() {
        let (props, entries) = parse_slt(SAMPLE);
        assert!(props.iter().any(|(k, v)| k == "Type" && v == "7z"));
        assert_eq!(entries.len(), 3);

        let dir = &entries[0];
        assert_eq!(dir.name, "docs");
        assert!(dir.is_dir);
        assert_eq!(dir.modified.as_deref(), Some("2026-11-06 14:00"));

        let readme = &entries[1];
        assert_eq!(readme.name, "docs/readme.txt");
        assert!(!readme.is_dir);
        assert_eq!(readme.size, 1711);
        assert_eq!(readme.packed, Some(700));
        assert_eq!(readme.size_text(), "1 711");
        assert_eq!(readme.crc.as_deref(), Some("1234ABCD"));
        // Секунды обрезаны под колонку.
        assert_eq!(readme.modified.as_deref(), Some("2026-11-06 14:01"));
        assert_eq!(readme.created.as_deref(), Some("2026-11-06 13:00"));
    }

    #[test]
    fn ignores_banner_and_scan_lines() {
        let (props, _) = parse_slt(SAMPLE);
        assert!(!props.iter().any(|(k, _)| k.contains("Scanning")));
        assert_eq!(props[0].0, "Path");
    }

    #[test]
    fn windows_backslash_paths_are_normalised() {
        let text = "\
--
Path = x.7z
Type = 7z

----------
Path = docs\\img\\
Size = 0
Folder = +
";
        let (_, entries) = parse_slt(text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "docs/img/");
        assert!(entries[0].is_dir);
    }

    #[test]
    fn keeps_leading_and_trailing_spaces_in_names() {
        let text = "\
--
Path = x.7z
Type = 7z

----------
Path =  leading.txt
Size = 1

Path = trailing.txt 
Size = 2
";
        let (_, entries) = parse_slt(text);
        assert_eq!(entries[0].name, " leading.txt");
        assert_eq!(entries[1].name, "trailing.txt ");
    }

    #[test]
    fn multiline_names_are_glued_back() {
        let text = "\
--
Path = x.7z
Type = 7z

----------
Path = dir/multi
line name.txt
Size = 10
";
        let (_, entries) = parse_slt(text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "dir/multi\nline name.txt");
        assert_eq!(entries[0].size, 10);
    }

    #[test]
    fn dashes_inside_entries_are_not_a_section_marker() {
        let text = "\
--
Path = x.7z
Type = 7z

----------
Path = ----------
Size = 5
";
        let (_, entries) = parse_slt(text);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "----------");
    }

    #[test]
    fn lowercase_posix_dir_attribute_counts_as_dir() {
        let text = "\
--
Path = x.7z
Type = 7z

----------
Path = d
Size = 0
Attributes = drwxr-xr-x
";
        let (_, entries) = parse_slt(text);
        assert!(entries[0].is_dir);
    }

    #[test]
    fn empty_output_gives_nothing() {
        let (props, entries) = parse_slt("");
        assert!(props.is_empty());
        assert!(entries.is_empty());
    }

    #[test]
    fn missing_folder_key_falls_back_to_attributes() {
        let text = "\
--
Path = x.7z
Type = 7z

----------
Path = only
Size = 10
Attributes = D_ drwxr-xr-x
";
        let (_, entries) = parse_slt(text);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_dir);
    }

    #[test]
    fn encrypted_entries_are_flagged() {
        // Так выглядит архив, созданный с паролем: имена видны, содержимое — нет.
        let text = "\
--
Path = x.7z
Type = 7z
Method = LZMA2:12 7zAES

----------
Path = a.txt
Size = 13
Encrypted = +
Method = LZMA2:12 7zAES:19

Path = b.bin
Size = 8
";
        let listing = parse_archive_output(Path::new("x.7z"), text.as_bytes()).unwrap();
        assert!(listing.encrypted, "хотя бы один элемент зашифрован");
        assert!(listing.entries[0].encrypted);
        assert!(!listing.entries[1].encrypted);
    }

    #[test]
    fn plain_archive_is_not_encrypted() {
        let listing = parse_archive_output(Path::new("sample.7z"), SAMPLE.as_bytes()).unwrap();
        assert!(!listing.encrypted);
        assert!(listing.entries.iter().all(|entry| !entry.encrypted));
    }

    #[test]
    fn partial_listing_with_warnings_is_parseable() {
        // Код 1 от 7z: часть записей прочитана, дальше предупреждение.
        // Такой stdout должен разбираться, а не выбрасываться.
        let text = format!("{SAMPLE}\nWARNING: Cannot open the file as [7z] archive\n");
        let listing = parse_archive_output(Path::new("sample.7z"), text.as_bytes()).unwrap();
        assert_eq!(listing.entries.len(), 3);
    }
}

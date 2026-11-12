//! Листинг обычного каталога файловой системы.

use std::fs;
use std::io;
use std::path::Path;

use crate::entry::{Entry, format_time};

/// Читает каталог и отдаёт записи, отсортированные «каталоги сверху, потом по имени».
///
/// Скрытые файлы не фильтруем: 7-Zip их тоже показывает.
pub fn list_dir(path: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();

    for item in fs::read_dir(path)? {
        let item = item?;
        let metadata = match item.metadata() {
            Ok(metadata) => metadata,
            // Битая ссылка или гонка с удалением: показываем имя, но без размера и дат.
            Err(_) => {
                entries.push(Entry::file(item.file_name().to_string_lossy().into_owned()));
                continue;
            }
        };

        // metadata разыменовывает ссылки: symlink на каталог показываем
        // каталогом, как в 7-Zip FM (битые ссылки отсеяны выше).
        let is_dir = metadata.file_type().is_dir();
        let mut entry = entry_for(&item.file_name().to_string_lossy(), is_dir);

        entry.size = if is_dir { 0 } else { metadata.len() };
        entry.modified = metadata.modified().ok().map(format_time);
        entry.created = metadata.created().ok().map(format_time);
        entries.push(entry);
    }

    crate::entry::sort_entries(&mut entries, crate::entry::SortKey::Name, true);
    Ok(entries)
}

fn entry_for(name: &str, is_dir: bool) -> Entry {
    if is_dir {
        Entry::dir(name)
    } else {
        Entry::file(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Уникальный каталог на каждый запуск. Раньше путь был `archiver-list-<pid>`,
    /// и повторный прогон в том же процессе (или сразу после падения) ловил
    /// DELETE_PENDING на NTFS: `remove_dir_all` → `create_dir_all` по тому же пути
    /// отдаёт ERROR_ACCESS_DENIED, пока каталог держит антивирус или индексер.
    fn unique_temp_dir(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};

        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "archiver-{tag}-{}-{nanos}-{seq}",
            std::process::id()
        ))
    }

    #[test]
    fn lists_directory_without_crashing() {
        let dir = unique_temp_dir("list");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("a.txt"), b"hello").unwrap();
        fs::write(dir.join("B.txt"), b"hi").unwrap();

        let entries = list_dir(&dir).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();

        assert!(names.contains(&"a.txt"));
        assert!(names.contains(&"B.txt"));
        assert!(names.contains(&"sub"));

        // Каталог — первым независимо от алфавита.
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].name, "sub");

        let a = entries.iter().find(|e| e.name == "a.txt").unwrap();
        assert_eq!(a.size, 5);
        assert!(a.modified.is_some());
        assert_eq!(a.size_text(), "5");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_dir_is_an_error() {
        let dir = std::env::temp_dir().join("archiver-definitely-not-here-42");
        assert!(list_dir(&dir).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_dir_is_shown_as_dir() {
        let dir = unique_temp_dir("symlink");
        fs::create_dir_all(dir.join("real")).unwrap();
        std::os::unix::fs::symlink(dir.join("real"), dir.join("link")).unwrap();

        let entries = list_dir(&dir).unwrap();
        let link = entries.iter().find(|e| e.name == "link").unwrap();
        assert!(link.is_dir, "ссылка на каталог должна быть каталогом");

        fs::remove_dir_all(&dir).ok();
    }
}

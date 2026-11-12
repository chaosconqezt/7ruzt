//! Что считать архивом по имени файла.
//!
//! Нужно, чтобы «Извлечь» и «Проверить» оживали, когда в папке выделен файл
//! архива: открывать его двойным щелчком для этого не обязательно, 7-Zip FM
//! так и работает. Формат определяет движок, поэтому список расширений шире,
//! чем то, что мы умеем создавать.

/// Расширения, которые 7-Zip открывает как архивы.
///
/// `exe` намеренно нет: самораспаковывающийся архив не отличить от обычной
/// программы по имени, а «Проверить» на случайном `.exe` — это ошибка на пустом
/// месте.
const EXTENSIONS: [&str; 38] = [
    "7z", "zip", "zipx", "rar", "tar", "gz", "tgz", "tpz", "bz2", "tbz2", "tbz", "xz", "txz",
    "lzma", "z", "zst", "tzst", "lz4", "br", "cab", "arj", "lzh", "lha", "iso", "wim", "swm",
    "esd", "msi", "msp", "udf", "xar", "cpio", "rpm", "deb", "dmg", "vhd", "vhdx", "squashfs",
];

/// Похоже ли имя файла на архив.
///
/// Учитывает тома (`backup.7z.001` — это тоже архив, 7z открывает его с любого
/// тома) и регистр (`PHOTO.JPG` не архив, а `PHOTO.ZIP` — архив).
pub fn is_archive_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();

    // Числовой хвост из трёх цифр — номер тома, а не расширение.
    let stem = match lower.rsplit_once('.') {
        Some((head, tail)) if is_volume_suffix(tail) => head,
        _ => lower.as_str(),
    };

    let Some((_, extension)) = stem.rsplit_once('.') else {
        return false;
    };
    EXTENSIONS.contains(&extension)
}

/// `001` … `999` — номер тома.
fn is_volume_suffix(tail: &str) -> bool {
    tail.len() == 3 && tail.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_common_archives() {
        for name in [
            "debug.7z",
            "photos.ZIP",
            "backup.tar",
            "logs.tar.gz",
            "image.iso",
            "setup.msi",
            "movie.mkv.7z",
            "x.zst",
        ] {
            assert!(is_archive_name(name), "{name} должен считаться архивом");
        }
    }

    #[test]
    fn volumes_are_archives() {
        assert!(is_archive_name("backup.7z.001"));
        assert!(is_archive_name("backup.7z.999"));
        // Хвост не из трёх цифр — уже не номер тома.
        assert!(!is_archive_name("backup.7z.01"));
        assert!(!is_archive_name("numbers.001"));
    }

    #[test]
    fn ordinary_files_are_not_archives() {
        for name in [
            "readme.txt",
            "photo.jpg",
            "archiver-app.exe",
            "archive",
            ".gitignore",
        ] {
            assert!(!is_archive_name(name), "{name} не архив");
        }
    }

    #[test]
    fn name_without_extension_is_not_archive() {
        assert!(!is_archive_name("7z"));
        assert!(is_archive_name(".7z"));
    }
}

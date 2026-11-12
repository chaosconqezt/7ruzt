use crate::prelude::*;
/// Короткое объяснение, почему файл не открылся как архив.
///
/// В статус-бар помещается одна строка, а 7z первым делом печатает эхо пути
/// (`ERROR: <путь> : <путь>`), причина — следующей. Берём последнюю строку:
/// она конкретнее всего, а эхо и так видно из выделенной строки списка.
pub(crate) fn open_failure(name: &str, error: &str) -> String {
    let reason = error
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("ERROR: "))
        .unwrap_or_else(|| error.trim());
    format!("«{name}» не открывается как архив: {reason}")
}

/// Лежит ли файл в этом каталоге.
///
/// Сравниваем канонические пути: перетащенный путь приходит абсолютным, а
/// открытый каталог мог быть передан относительным (`.\target\smoke`).
pub(crate) fn parent_is(path: &std::path::Path, dir: &std::path::Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    match (std::fs::canonicalize(parent), std::fs::canonicalize(dir)) {
        (Ok(parent), Ok(dir)) => parent == dir,
        _ => parent == dir,
    }
}

/// Суммарный размер и число файлов под путями источников.
///
/// Нужно окну прогресса: 7z в CLI сообщает только процент, а «Всего»/«Скорость»
/// без объёма не посчитать. Обход идёт в фоновом потоке — на UI это заморозило бы
/// окно, а 7z всё равно пройдёт по тем же каталогам.
pub(crate) fn scan_totals(paths: &[PathBuf]) -> (u64, u64) {
    let mut bytes = 0;
    let mut files = 0;
    for path in paths {
        scan_path(path, &mut bytes, &mut files);
    }
    (bytes, files)
}

pub(crate) fn scan_path(path: &std::path::Path, bytes: &mut u64, files: &mut u64) {
    // symlink_metadata, а не metadata: в ссылку не заходим, иначе junction
    // в Windows может увести обход по кругу.
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if meta.file_type().is_symlink() {
        *files += 1;
        return;
    }
    if meta.is_file() {
        *bytes += meta.len();
        *files += 1;
        return;
    }
    if meta.is_dir()
        && let Ok(entries) = std::fs::read_dir(path)
    {
        for entry in entries.flatten() {
            scan_path(&entry.path(), bytes, files);
        }
    }
}

/// Находит рабочий 7-Zip. Блокирующая (при первом вызове — `7z i`):
/// звать из фонового потока.
pub(crate) fn resolve_tool() -> Result<archiver_core::ResolvedTool, String> {
    Locator::for_current_exe()
        .map_err(|err| err.to_string())
        .and_then(|locator| {
            locator
                .resolve()
                .map_err(|err| format!("7-Zip недоступен: {err}"))
        })
}

/// Листинг архива через найденный 7-Zip.
///
/// Отдельной функцией, а не методом окна: её зовут и обычная загрузка (`reload`),
/// и вход в архив по Enter, которому нужно узнать об ошибке до переезда.
pub(crate) fn read_archive(path: &std::path::Path) -> Result<Listing, String> {
    let tool = resolve_tool()?;
    list_archive(&tool.info.exe, path).map_err(|err| format!("не удалось открыть архив: {err}"))
}

/// Содержимое панели, прочитанное с диска/из архива.
pub(crate) struct PanelRead {
    pub entries: Vec<Entry>,
    pub kind: Option<String>,
    pub encrypted: bool,
    pub is_archive: bool,
}

/// Блокирующее чтение содержимого панели: каталог или архив (`7z i` + `7z l`).
/// Звать ТОЛЬКО из фонового потока — на UI это заморозило бы окно.
pub(crate) fn read_panel(path: &std::path::Path) -> Result<PanelRead, String> {
    if path.is_dir() {
        let entries =
            list_dir(path).map_err(|err| format!("не удалось прочитать каталог: {err}"))?;
        Ok(PanelRead {
            entries,
            kind: None,
            encrypted: false,
            is_archive: false,
        })
    } else {
        let listing = read_archive(path)?;
        Ok(PanelRead {
            entries: listing.entries,
            kind: listing.kind,
            encrypted: listing.encrypted,
            is_archive: true,
        })
    }
}

/// Разбирает введённый адрес относительно базы панели: каталог или архив
/// с подкаталогом. Блокирующая (метаданные ФС) — звать из фонового потока.
pub(crate) fn resolve_address(
    base: &std::path::Path,
    address: &str,
) -> Result<(PathBuf, String), String> {
    if address.is_empty() {
        return Err("Введите путь к папке или архиву".to_string());
    }
    let requested = base.join(address);
    if requested.is_dir() {
        let path = requested.canonicalize().map_err(|err| fs_error(&err))?;
        return Ok((path, String::new()));
    }
    let archive = requested
        .ancestors()
        .find(|path| path.is_file())
        .ok_or_else(|| "Папка или архив не найдены".to_string())?;
    let prefix = requested
        .strip_prefix(archive)
        .map_err(|err| err.to_string())?
        .to_string_lossy()
        .replace('\\', "/");
    Ok((archive.to_path_buf(), prefix.trim_matches('/').to_string()))
}

/// Есть ли в листинге архива подкаталог `prefix` (для адреса вида `a.7z\docs`).
pub(crate) fn prefix_exists(entries: &[Entry], prefix: &str) -> bool {
    let directory = format!("{prefix}/");
    entries.iter().any(|entry| {
        let name = entry.name.replace('\\', "/");
        name.starts_with(&directory) || (entry.is_dir && name.trim_end_matches('/') == prefix)
    })
}

/// Каталог и префикс для подсказок адреса: введённый путь режем на родитель
/// и хвост. Хвостовой разделитель означает «показать содержимое этого каталога».
/// Абсолютный путь в вводе замещает базу (`Path::join` так умеет).
pub(crate) fn split_address_hint(base: &std::path::Path, value: &str) -> (PathBuf, String) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return (base.to_path_buf(), String::new());
    }
    let requested = base.join(trimmed);
    if trimmed.ends_with('\\') || trimmed.ends_with('/') {
        return (requested, String::new());
    }
    let parent = requested
        .parent()
        .map(|path| path.to_path_buf())
        .unwrap_or_else(|| base.to_path_buf());
    let prefix = requested
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    (parent, prefix)
}

/// Подсказки адреса: дети каталога с префиксом, каталоги первыми, не больше 12.
/// Блокирующая (read_dir) — звать из фонового потока.
pub(crate) fn list_address_suggestions(
    dir: &std::path::Path,
    prefix: &str,
) -> Vec<(PathBuf, bool)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let prefix_lower = prefix.to_lowercase();
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.to_lowercase().starts_with(&prefix_lower) {
            continue;
        }
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        if is_dir {
            dirs.push((entry.path(), true));
        } else {
            files.push((entry.path(), false));
        }
    }
    dirs.sort();
    files.sort();
    dirs.extend(files);
    dirs.truncate(12);
    dirs
}

/// Хвост адреса после последнего разделителя: то, что человек уже набрал.
fn address_tail(value: &str) -> &str {
    value.rsplit(['\\', '/']).next().unwrap_or(value)
}

/// Остаток имени после набранного хвоста, без учёта регистра.
///
/// `completion_rest("Windows", "wi") == Some("ndows")`. `None` — имя не
/// начинается с хвоста; тогда подсказка не подходит.
pub(crate) fn completion_rest(name: &str, typed: &str) -> Option<String> {
    let mut chars = name.char_indices();
    let mut end = 0;
    for typed_char in typed.chars() {
        let (index, name_char) = chars.next()?;
        // `to_lowercase` у `char` понимает Unicode, поэтому «Док» дополнит
        // «Документы», а не только латиницу.
        if !name_char.to_lowercase().eq(typed_char.to_lowercase()) {
            return None;
        }
        end = index + name_char.len_utf8();
    }
    Some(name[end..].to_string())
}

/// Бледный хвост, который дополняет набранный адрес.
///
/// Если совпадений несколько, показываем их общий префикс (как shell: Tab
/// дополнит до него). Когда общего префикса нет, берём первый вариант, который
/// действительно что-то дополняет, — иначе Tab на точном имени файла молчал бы.
pub(crate) fn completion_ghost(value: &str, suggestions: &[(PathBuf, bool)]) -> Option<String> {
    if suggestions.is_empty() {
        return None;
    }
    let typed = address_tail(value);
    let rests: Vec<(String, bool)> = suggestions
        .iter()
        .filter_map(|(path, is_dir)| {
            let name = path.file_name()?.to_string_lossy();
            completion_rest(&name, typed).map(|rest| (rest, *is_dir))
        })
        .collect();
    if rests.is_empty() {
        return None;
    }
    if rests.len() == 1 {
        let (rest, is_dir) = &rests[0];
        // Точное имя файла дополнять нечем — пустой хвост не рисуем.
        return (*is_dir || !rest.is_empty()).then(|| finish_completion(rest, *is_dir));
    }
    let common = common_prefix(rests.iter().map(|(rest, _)| rest.as_str()));
    if !common.is_empty() {
        return Some(common);
    }
    rests
        .iter()
        .find(|(rest, is_dir)| *is_dir || !rest.is_empty())
        .map(|(rest, is_dir)| finish_completion(rest, *is_dir))
}

/// Каталогу добавляем разделитель: Tab сразу открывает следующий уровень.
fn finish_completion(rest: &str, is_dir: bool) -> String {
    if is_dir && !rest.ends_with(std::path::MAIN_SEPARATOR) {
        format!("{rest}{}", std::path::MAIN_SEPARATOR)
    } else {
        rest.to_string()
    }
}

/// Наибольший общий префикс строк (по байтам, с выравниванием по границе UTF-8).
fn common_prefix<'a>(mut items: impl Iterator<Item = &'a str>) -> String {
    let Some(first) = items.next() else {
        return String::new();
    };
    let mut prefix = first.to_string();
    for item in items {
        let mut len = prefix
            .bytes()
            .zip(item.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        while len > 0 && !prefix.is_char_boundary(len) {
            len -= 1;
        }
        prefix.truncate(len);
        if prefix.is_empty() {
            break;
        }
    }
    prefix
}

/// Зашифрован ли архив, который мы ещё не открывали. Блокирующая: звать
/// из фонового потока. Ошибку чтения считаем как «неизвестно» — битый архив
/// проверить всё равно надо, а лишний вопрос о пароле только помешает.
pub(crate) fn probe_encrypted(archive: &std::path::Path) -> bool {
    let Ok(tool) = resolve_tool() else {
        return false;
    };
    list_archive(&tool.info.exe, archive)
        .map(|listing| listing.encrypted)
        .unwrap_or(false)
}

/// Временная папка для просмотра файла из архива: `%TEMP%\7zO…`, как в 7-Zip FM.
///
/// Имя уникально (счётчик процесса + наносекунды): два открытых файла не должны
/// затирать друг друга, а повторное открытие — попадать в старое содержимое.
pub(crate) fn temp_open_dir() -> std::io::Result<PathBuf> {
    use std::sync::atomic::AtomicU64;
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("7zO{stamp:x}-{seq:x}"));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Можно ли безопасно извлечь запись архива с таким именем.
///
/// Имя записи приходит из чужого файла: `../`, абсолютные пути и имена вроде
/// `CON` не должны уводить запись за пределы временной папки. Каждый компонент
/// проверяем как обычное имя файла (`check_name` отвергает разделители, `..`,
/// зарезервированные имена и прочее, что Windows не переживёт).
pub(crate) fn entry_path_is_safe(name: &str) -> bool {
    !name.is_empty() && name.split('/').all(|part| check_name(part).is_ok())
}

/// Имя объекта для сообщений: без пути, но с запасным вариантом.
pub(crate) fn name_of(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Ошибка файловой операции по-русски и коротко.
pub(crate) fn fs_error(err: &std::io::Error) -> String {
    match err.kind() {
        std::io::ErrorKind::AlreadyExists => "такое имя уже занято".to_string(),
        std::io::ErrorKind::PermissionDenied => "нет прав доступа".to_string(),
        std::io::ErrorKind::NotFound => "объект не найден".to_string(),
        std::io::ErrorKind::InvalidInput => err.to_string(),
        _ => err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        completion_ghost, completion_rest, entry_path_is_safe, split_address_hint, temp_open_dir,
    };

    #[test]
    fn completion_adds_rest_of_the_name_and_separator_for_directories() {
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(completion_rest("Windows", "wi").as_deref(), Some("ndows"));
        assert_eq!(completion_rest("Windows", "Wi").as_deref(), Some("ndows"));
        assert_eq!(completion_rest("Windows", "x"), None);
        assert_eq!(
            completion_rest("Документы", "док").as_deref(),
            Some("ументы")
        );
        let dir = std::path::PathBuf::from("C:/Windows");
        assert_eq!(
            completion_ghost("C:\\Wi", &[(dir.clone(), true)]),
            Some(format!("ndows{sep}"))
        );
        assert_eq!(
            completion_ghost("C:\\Windows", &[(dir, true)]),
            Some(sep.to_string())
        );
    }

    #[test]
    fn completion_uses_common_prefix_when_matches_diverge() {
        let sep = std::path::MAIN_SEPARATOR;
        let suggestions = vec![
            (std::path::PathBuf::from("C:/Windows"), true),
            (std::path::PathBuf::from("C:/WinRAR"), true),
        ];
        // Общий хвост «n»: Tab дополнит до C:\Win.
        assert_eq!(
            completion_ghost("C:\\Wi", &suggestions).as_deref(),
            Some("n")
        );
        // Когда общего хвоста нет, берём первый вариант, а не молчим.
        assert_eq!(
            completion_ghost("C:\\Win", &suggestions),
            Some(format!("dows{sep}"))
        );
    }

    #[test]
    fn completion_handles_trailing_separator_and_empty_tail() {
        let sep = std::path::MAIN_SEPARATOR;
        let child = vec![(std::path::PathBuf::from("C:/Windows/System32"), true)];
        assert_eq!(
            completion_ghost("C:\\Windows\\", &child),
            Some(format!("System32{sep}"))
        );
        assert_eq!(completion_ghost("", &child), Some(format!("System32{sep}")));
        assert_eq!(completion_ghost("C:\\x", &[]), None);
    }

    #[test]
    fn entry_paths_reject_traversal_and_unsafe_names() {
        assert!(entry_path_is_safe("readme.txt"));
        assert!(entry_path_is_safe("docs/img/photo.png"));
        assert!(!entry_path_is_safe("../evil.txt"));
        assert!(!entry_path_is_safe("docs/../../evil.txt"));
        assert!(!entry_path_is_safe("C:/evil.txt"));
        assert!(!entry_path_is_safe("/evil.txt"));
        assert!(!entry_path_is_safe("docs\\evil.txt"));
        assert!(!entry_path_is_safe("docs/./evil.txt"));
        assert!(!entry_path_is_safe("CON"));
        assert!(!entry_path_is_safe(""));
    }

    #[test]
    fn temp_open_dirs_are_unique_and_created() {
        let first = temp_open_dir().expect("временная папка создаётся");
        let second = temp_open_dir().expect("временная папка создаётся");
        assert_ne!(first, second);
        assert!(first.is_dir());
        assert!(second.is_dir());
        assert!(first.starts_with(std::env::temp_dir()));
        let _ = std::fs::remove_dir_all(&first);
        let _ = std::fs::remove_dir_all(&second);
    }

    #[test]
    fn address_hint_splits_parent_and_prefix() {
        let base = std::path::Path::new("C:/base");
        // Пустой ввод — содержимое базы.
        assert_eq!(
            split_address_hint(base, ""),
            (base.to_path_buf(), String::new())
        );
        // Хвост — префикс имени в родителе.
        let (dir, prefix) = split_address_hint(base, "sub/na");
        assert_eq!(dir, base.join("sub"));
        assert_eq!(prefix, "na");
        // Хвостовой разделитель — содержимое введённого каталога.
        let (dir, prefix) = split_address_hint(base, "sub\\");
        assert_eq!(dir, base.join("sub"));
        assert!(prefix.is_empty());
        // Абсолютный путь замещает базу.
        let (dir, prefix) = split_address_hint(base, "D:/other/fo");
        assert_eq!(dir, std::path::Path::new("D:/other"));
        assert_eq!(prefix, "fo");
    }
}

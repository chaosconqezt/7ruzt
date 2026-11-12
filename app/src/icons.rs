//! Системные значки файлов и папок.
//!
//! Windows-оболочка отдаёт готовую иконку нужного размера: файлу — по типу,
//! папке — стандартную жёлтую. Внешний `file_icon_provider` прячет `unsafe`
//! Win32/COM, поэтому наш линт `unsafe_code = "forbid"` остаётся в силе.
//!
//! Почему кеш: одна и та же иконка у всех `.txt` (а в каталоге их сотни).
//! Запрашиваем у оболочки один раз на ключ, а не на каждую строку кадра.
//! `.exe`/`.lnk` кешируются по полному пути: у программ своя иконка.
//!
//! Записям внутри архива на диске соответствия нет. Оболочке нужен
//! существующий путь, поэтому для них держим пустой файл-пробник с тем же
//! расширением во временной папке `%TEMP%\7zO-icons`.
//!
//! На Linux/macOS пока возвращаем `None` и рисуем встроенные SVG: их сборка
//! ещё впереди (`doc/vendor.md`, порядок платформ). Интерфейс кеша уже общий,
//! реализацию платформы добавим туда же.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use crate::prelude::*;

/// Логические размеры значков списка (`theme::ICON_DETAILS`/`ICON_LARGE`).
/// У оболочки просим крупнее: так значок остаётся чётким при HiDPI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum IconSize {
    Small,
    Large,
}

impl IconSize {
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    fn shell_px(self) -> u16 {
        match self {
            Self::Small => 32,
            Self::Large => 48,
        }
    }
}

/// Общая часть ключа: у каталогов иконка одна, у файлов — по расширению,
/// кроме программ и ярлыков, где важна конкретная цель.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum IconKey {
    Dir,
    Ext(String),
    File(PathBuf),
}

/// Источник значка строки.
///
/// Windows/macOS отдают готовый растр, Linux — путь к PNG/SVG из темы
/// freedesktop: gpui сам загрузит и закеширует его как image-asset.
#[derive(Clone)]
#[allow(dead_code)] // каждый вариант собирает только своя платформа
pub(crate) enum EntryIcon {
    Image(Arc<RenderImage>),
    File(PathBuf),
}

/// Один запрос к оболочке, уже отмеченный в `pending`.
#[derive(Clone, Debug)]
pub(crate) struct IconRequest {
    key: IconKey,
    size: IconSize,
    /// Поля ниже читают только Windows/macOS-загрузчики; Linux обходится
    /// расширением из ключа, но структуру ради этого не раздваиваем.
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
    probe: PathBuf,
    /// Пробник во временной папке можно создать; настоящий файл — нельзя
    /// (иначе после удаления файла мы воскресили бы пустышку).
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
    create_probe: bool,
}

type IconSlot = (IconKey, IconSize);

/// Кеш значков на всё окно.
#[derive(Default)]
pub(crate) struct IconCache {
    images: RefCell<HashMap<IconSlot, Option<EntryIcon>>>,
    pending: RefCell<HashSet<IconSlot>>,
}

impl IconCache {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Готовый значок для строки списка. `None` — ещё не загружен (или
    /// система его не дала): тогда показываем встроенный SVG.
    pub(crate) fn lookup(
        &self,
        name: &str,
        is_dir: bool,
        in_archive: bool,
        base: &Path,
        size: IconSize,
    ) -> Option<EntryIcon> {
        let key = key_for(name, is_dir, in_archive, base);
        self.images.borrow().get(&(key, size)).cloned().flatten()
    }

    /// Какие значки нужно догрузить для этого уровня. Возвращает только новые:
    /// готовые и уже запрошенные пропускаются, поэтому повторный вызов с тем же
    /// списком ничего не делает.
    pub(crate) fn requests(
        &self,
        entries: &[Entry],
        base: &Path,
        in_archive: bool,
    ) -> Vec<IconRequest> {
        // Сначала схлопываем по ключу: папок и `.txt` в каталоге много,
        // а иконка у них одна.
        let mut unique: HashMap<IconKey, (PathBuf, bool)> = HashMap::new();
        for entry in entries {
            let key = key_for(&entry.name, entry.is_dir, in_archive, base);
            unique
                .entry(key)
                .or_insert_with(|| probe_for(&entry.name, entry.is_dir, in_archive, base));
        }

        let images = self.images.borrow();
        let mut pending = self.pending.borrow_mut();
        let mut requests = Vec::new();
        for (key, (probe, create_probe)) in unique {
            for size in [IconSize::Small, IconSize::Large] {
                let slot = (key.clone(), size);
                if images.contains_key(&slot) || !pending.insert(slot.clone()) {
                    continue;
                }
                requests.push(IconRequest {
                    key: key.clone(),
                    size,
                    probe: probe.clone(),
                    create_probe,
                });
            }
        }
        requests
    }

    /// Результаты фоновой загрузки. `None` тоже запоминаем: незачем снова
    /// спрашивать оболочку при каждом открытии каталога.
    pub(crate) fn complete(&self, results: Vec<(IconRequest, Option<EntryIcon>)>) {
        let mut images = self.images.borrow_mut();
        let mut pending = self.pending.borrow_mut();
        for (request, image) in results {
            let slot = (request.key, request.size);
            pending.remove(&slot);
            images.insert(slot, image);
        }
    }
}

/// Ключ кеша. Его считают и отрисовка, и предзагрузка, поэтому правило одно.
fn key_for(name: &str, is_dir: bool, in_archive: bool, base: &Path) -> IconKey {
    if is_dir {
        IconKey::Dir
    } else if !in_archive && per_file_icon(name) {
        IconKey::File(base.join(name))
    } else {
        IconKey::Ext(file_extension(name).to_ascii_lowercase())
    }
}

fn probe_for(name: &str, is_dir: bool, in_archive: bool, base: &Path) -> (PathBuf, bool) {
    if is_dir {
        // Для папки годится любой существующий каталог: оболочка вернёт
        // стандартную папку. Внутри архива каталога на диске нет — берём temp.
        let probe = if in_archive {
            std::env::temp_dir()
        } else {
            base.to_path_buf()
        };
        return (probe, false);
    }

    // Настоящий файл — самый точный источник (иконка программы, ассоциации).
    // `exists()` здесь не зовём: на сетевом каталоге это лишний синхронный
    // заход в ФС на каждую строку. Если файла уже нет, фоновая загрузка просто
    // ничего не вернёт.
    if !in_archive {
        return (base.join(name), false);
    }

    // Внутри архива: берём пробник с тем же расширением.
    (probe_path(file_extension(name)), true)
}

fn per_file_icon(name: &str) -> bool {
    matches!(
        file_extension(name).to_ascii_lowercase().as_str(),
        "exe" | "lnk"
    )
}

/// Временная папка пробников. Её же регистрирует окно для очистки при закрытии.
pub(crate) fn probe_dir() -> PathBuf {
    std::env::temp_dir().join("7zO-icons")
}

fn probe_path(ext: &str) -> PathBuf {
    let ext = ext.to_ascii_lowercase();
    let name = if ext.is_empty() {
        "probe".to_string()
    } else {
        format!("probe.{ext}")
    };
    probe_dir().join(name)
}

/// Загружает системный значок Windows/macOS. Вызывается только из фонового
/// потока: запрос к оболочке синхронный и не должен попадать в кадр.
#[cfg(any(windows, target_os = "macos"))]
pub(crate) fn load_system_icon(request: &IconRequest) -> Option<EntryIcon> {
    if !ensure_probe(&request.probe, request.create_probe) {
        return None;
    }

    let icon = file_icon_provider::get_file_icon(&request.probe, request.size.shell_px()).ok()?;
    if icon.width == 0 || icon.height == 0 {
        return None;
    }
    let expected = usize::try_from(icon.width).ok()? * usize::try_from(icon.height).ok()? * 4;
    if icon.pixels.len() != expected {
        return None;
    }

    // `file_icon_provider` отдаёт RGBA, а gpui хранит BGRA (его PNG-декодер
    // тоже меняет каналы местами).
    let mut pixels = icon.pixels;
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(icon.width, icon.height, pixels)?;
    Some(EntryIcon::Image(Arc::new(RenderImage::new(vec![
        image::Frame::new(buffer),
    ]))))
}

#[cfg(any(windows, target_os = "macos"))]
fn ensure_probe(path: &Path, create: bool) -> bool {
    if path.exists() {
        return true;
    }
    if !create {
        return false;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .is_ok()
}

/// Linux: ищем PNG/SVG в текущей теме freedesktop (KDE/GNOME/…).
///
/// GTK не подключаем: `freedesktop-icons` читает темы напрямую, поэтому
/// сборке не нужны `libgtk-3-dev` и прочие системные библиотеки.
#[cfg(target_os = "linux")]
pub(crate) fn load_system_icon(request: &IconRequest) -> Option<EntryIcon> {
    let candidates = linux_theme()
        .as_deref()
        .map(theme_candidates)
        .unwrap_or_default();
    for name in icon_names(&request.key) {
        for theme in &candidates {
            let lookup = freedesktop_icons::lookup(&name)
                .with_size(request.size.shell_px())
                .with_theme(theme);
            if let Some(path) = lookup.find()
                && loadable_icon(&path)
            {
                return Some(EntryIcon::File(path));
            }
        }
        if candidates.is_empty()
            && let Some(path) = freedesktop_icons::lookup(&name)
                .with_size(request.size.shell_px())
                .find()
            && loadable_icon(&path)
        {
            return Some(EntryIcon::File(path));
        }
    }
    None
}

/// `kdeglobals` пишет `Theme=Breeze Dark`, а каталог темы — `breeze-dark`;
/// пробуем оба написания и регистр, прежде чем откатиться на hicolor.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn theme_candidates(theme: &str) -> Vec<String> {
    let mut out = Vec::new();
    for candidate in [
        theme.to_string(),
        theme.to_ascii_lowercase(),
        theme.to_ascii_lowercase().replace(' ', "-"),
        theme.replace(' ', "-"),
    ] {
        if !candidate.is_empty() && !out.contains(&candidate) {
            out.push(candidate);
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn linux_theme() -> Option<String> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    if let Some(config) = config {
        // KDE: ~/.config/kdeglobals, секция [Icons], ключ Theme.
        if let Ok(text) = std::fs::read_to_string(config.join("kdeglobals"))
            && let Some(theme) = ini_value(&text, "Icons", "Theme")
        {
            return Some(theme);
        }
        // GNOME/GTK: настройка темы значков в settings.ini.
        for file in ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"] {
            if let Ok(text) = std::fs::read_to_string(config.join(file))
                && let Some(theme) = ini_value(&text, "Settings", "gtk-icon-theme-name")
            {
                return Some(theme);
            }
        }
    }
    // Последний шанс: gsettings. Без него останется hicolor внутри look-up.
    freedesktop_icons::default_theme_gtk()
}

#[cfg(target_os = "linux")]
fn loadable_icon(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "svg" | "jpg" | "jpeg" | "webp" | "bmp" | "ico"
            )
        })
}

/// Имена значков в теме freedesktop: сначала конкретный MIME, затем общий
/// для типа. У KDE/GNOME имена совпадают с MIME через дефис.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn icon_names(key: &IconKey) -> Vec<String> {
    match key {
        IconKey::Dir => vec!["folder".to_string(), "inode-directory".to_string()],
        IconKey::Ext(ext) => icon_names_for_ext(ext),
        IconKey::File(path) => icon_names_for_ext(
            path.extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or_default(),
        ),
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn icon_names_for_ext(ext: &str) -> Vec<String> {
    let ext = ext.to_ascii_lowercase();
    if ext.is_empty() {
        return vec![
            "text-x-generic".to_string(),
            "application-x-generic".to_string(),
            "unknown".to_string(),
        ];
    }
    if matches!(ext.as_str(), "exe" | "msi") {
        return vec![
            "application-x-executable".to_string(),
            "application-x-generic".to_string(),
        ];
    }

    let mime = mime_guess::from_ext(&ext)
        .first_or_octet_stream()
        .to_string();
    let mut names = vec![mime.replace('/', "-")];
    let top = mime.split_once('/').map(|(top, _)| top).unwrap_or_default();
    names.push(
        match top {
            "text" => "text-x-generic",
            "image" => "image-x-generic",
            "audio" => "audio-x-generic",
            "video" => "video-x-generic",
            "font" => "font-x-generic",
            "model" => "model-x-generic",
            _ => "application-x-generic",
        }
        .to_string(),
    );
    names.push("unknown".to_string());
    names.dedup();
    names
}

/// Значение ключа из простого INI. Нужен для `kdeglobals` и `settings.ini`,
/// поэтому без отдельной зависимости.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn ini_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut current = "";
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            current = &line[1..line.len() - 1];
            continue;
        }
        if current != section {
            continue;
        }
        if let Some((name, value)) = line.split_once('=')
            && name.trim() == key
        {
            return Some(value.trim().to_string());
        }
    }
    None
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub(crate) fn load_system_icon(_request: &IconRequest) -> Option<EntryIcon> {
    None
}

#[cfg(test)]
mod tests {
    // Не `super::*`: он тянет макрос `test` из prelude кита, и `#[test]`
    // раскручивается сам в себя до предела рекурсии.
    use super::{IconKey, icon_names, ini_value, key_for, theme_candidates};
    use crate::prelude::PathBuf;

    #[test]
    fn extension_key_folds_case_and_ignores_leading_dot() {
        let dir = PathBuf::from("C:/tmp");
        assert_eq!(
            key_for("PHOTO.JPG", false, false, &dir),
            IconKey::Ext("jpg".to_string())
        );
        assert_eq!(
            key_for(".gitignore", false, false, &dir),
            IconKey::Ext(String::new())
        );
    }

    #[test]
    fn folders_share_one_key_and_programs_keep_their_path() {
        let dir = PathBuf::from("C:/tmp");
        assert_eq!(key_for("docs", true, false, &dir), IconKey::Dir);
        assert_eq!(
            key_for("notepad.exe", false, false, &dir),
            IconKey::File(dir.join("notepad.exe"))
        );
        // В архиве настоящего exe нет — остаётся иконка типа.
        assert_eq!(
            key_for("tool.exe", false, true, &dir),
            IconKey::Ext("exe".to_string())
        );
    }

    #[test]
    fn linux_icon_names_prefer_mime_then_generic() {
        assert_eq!(
            icon_names(&IconKey::Ext("pdf".to_string()))[0],
            "application-pdf"
        );
        assert_eq!(
            icon_names(&IconKey::Ext("zip".to_string()))[0],
            "application-zip"
        );
        assert!(
            icon_names(&IconKey::Ext("png".to_string())).contains(&"image-x-generic".to_string())
        );
        assert_eq!(
            icon_names(&IconKey::Ext(String::new()))[0],
            "text-x-generic"
        );
        assert_eq!(icon_names(&IconKey::Dir)[0], "folder");
    }

    #[test]
    fn kde_theme_is_read_from_kdeglobals() {
        let text = "[General]\nColorScheme=BreezeDark\n\n[Icons]\nTheme=Breeze\n";
        assert_eq!(ini_value(text, "Icons", "Theme").as_deref(), Some("Breeze"));
        assert_eq!(ini_value(text, "Icons", "Missing"), None);
    }

    #[test]
    fn kde_theme_name_tries_directory_spelling() {
        let candidates = theme_candidates("Breeze Dark");
        assert_eq!(candidates[0], "Breeze Dark");
        assert!(candidates.contains(&"breeze-dark".to_string()));
    }
}

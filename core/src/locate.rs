//! Поиск рабочего бинарника 7-Zip.
//!
//! 7-Zip лежит **рядом с исполняемым файлом программы** — туда его кладёт
//! `build.rs` при сборке. `PATH` и переменные окружения не опрашиваются:
//! иначе на машине разработчика легко подхватить чужой или старый 7-Zip.

use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use crate::{
    tool::{self, ProbeError, ToolInfo},
    version::Version,
};

/// Не блочные замечания к найденному 7-Zip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// Версия ниже рекомендуемой. Работать можно, но возможны глюки.
    Outdated {
        found: Version,
        recommended: Version,
        /// Есть ли `-sccUTF-8`. Если нет — имена файлов читаем в системной кодировке.
        utf8_console: bool,
    },
}

impl Warning {
    pub fn message(&self) -> String {
        match self {
            Self::Outdated {
                found,
                recommended,
                utf8_console,
            } => {
                let mut text = format!(
                    "7-Zip {found} устарела: рекомендуется {recommended} или новее, возможны глюки."
                );
                if !utf8_console {
                    text.push_str(
                        " В этой версии нет `-sccUTF-8` — имена файлов будут читаться в системной кодировке.",
                    );
                }
                text
            }
        }
    }
}

/// Найденный и проверенный 7-Zip.
#[derive(Debug, Clone)]
pub struct ResolvedTool {
    pub info: ToolInfo,
    pub warnings: Vec<Warning>,
}

#[derive(Debug)]
pub enum LocateError {
    NotFound { tried: Vec<PathBuf> },
    NotUsable(Vec<String>),
}

impl std::fmt::Display for LocateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { .. } => write!(
                f,
                "7-Zip не найден: положи `{}` рядом с программой",
                binary_name()
            ),
            Self::NotUsable(failures) => {
                write!(f, "7-Zip найден, но не запускается:")?;
                for failure in failures {
                    write!(f, "\n  {failure}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for LocateError {}

/// Ищет `7z.exe`/`7zz` рядом с программой.
#[derive(Debug, Clone)]
pub struct Locator {
    exe_dir: PathBuf,
}

impl Locator {
    /// Ищет относительно папки текущего процесса — рабочий вариант для приложения.
    pub fn for_current_exe() -> std::io::Result<Self> {
        let exe = env::current_exe()?;
        let exe_dir = exe
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(Self::with_exe_dir(exe_dir))
    }

    /// Фиксированная папка (тесты, headless-режимы, `--exe-dir`).
    pub fn with_exe_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            exe_dir: dir.into(),
        }
    }

    /// Все кандидаты: имена бинарника для текущей платформы, по порядку.
    pub fn candidate_paths(&self) -> Vec<PathBuf> {
        binary_names()
            .iter()
            .map(|name| self.exe_dir.join(name))
            .collect()
    }

    /// Первый кандидат, который реально работает. Ошибки неудачных кандидатов
    /// копим, а не глотаем — иначе «не найден» без объяснений.
    ///
    /// Успешный результат кешируется по папке exe: версия и поддержка `-sccUTF-8`
    /// в рантайме не меняются, а `7z i` на каждую операцию — лишний процесс.
    /// Неудачи не кешируем: появившийся рядом с exe 7z должен подхватываться
    /// без перезапуска.
    pub fn resolve(&self) -> Result<ResolvedTool, LocateError> {
        if let Some(tool) = cached_tool(&self.exe_dir) {
            return Ok(tool);
        }
        let candidates = self.candidate_paths();
        let mut failures: Vec<String> = Vec::new();

        for path in &candidates {
            if !path.is_file() {
                continue;
            }
            match tool::probe(path) {
                Ok(info) => {
                    // Ниже 21.07 нет `-sccUTF-8`: имена файлов приходили бы
                    // в системной кодировке и ломались. Отказываем, а не предупреждаем.
                    if let Some(rejection) = version_rejection(info.version) {
                        failures.push(format!("{}: {rejection}", path.display()));
                        continue;
                    }
                    let warnings = outdated_warnings(info.version);
                    let resolved = ResolvedTool { info, warnings };
                    store_cached_tool(&self.exe_dir, resolved.clone());
                    return Ok(resolved);
                }
                Err(ProbeError::Spawn { source, .. })
                    if source.kind() == std::io::ErrorKind::NotFound =>
                {
                    // Гонка или битый симлинк — просто пробуем дальше.
                }
                Err(err) => failures.push(format!("{}: {err}", path.display())),
            }
        }

        if failures.is_empty() {
            Err(LocateError::NotFound { tried: candidates })
        } else {
            Err(LocateError::NotUsable(failures))
        }
    }
}

/// Почему версия не годится. `None` — версия поддерживается.
fn version_rejection(version: Version) -> Option<String> {
    if version.supports_utf8_console() {
        None
    } else {
        Some(format!(
            "версия {version} ниже минимальной {}: нет -sccUTF-8, имена файлов были бы в системной кодировке",
            Version::UTF8_CONSOLE_SINCE
        ))
    }
}

/// Кеш успешных resolve по папке exe. Мьютекс держится только на get/insert.
fn tool_cache() -> &'static Mutex<HashMap<PathBuf, ResolvedTool>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, ResolvedTool>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_tool(exe_dir: &Path) -> Option<ResolvedTool> {
    tool_cache()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .get(exe_dir)
        .cloned()
}

fn store_cached_tool(exe_dir: &Path, tool: ResolvedTool) {
    tool_cache()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .insert(exe_dir.to_path_buf(), tool);
}

/// Предупреждения для найденной версии. Пусто, если версия свежая.
pub fn outdated_warnings(version: Version) -> Vec<Warning> {
    if version.is_outdated() {
        vec![Warning::Outdated {
            found: version,
            recommended: Version::RECOMMENDED,
            utf8_console: version.supports_utf8_console(),
        }]
    } else {
        Vec::new()
    }
}

/// Основное имя бинарника на текущей платформе (для сообщений и раскладки).
pub const fn binary_name() -> &'static str {
    if cfg!(windows) { "7z.exe" } else { "7zz" }
}

/// Как называться бинарнику на текущей платформе. На unix официальный 7-Zip
/// зовётся `7zz`, но дистрибутивный p7zip кладёт `7z` — принимаем оба.
pub const fn binary_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["7z.exe"]
    } else {
        &["7zz", "7z"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_live_next_to_exe() {
        let exe_dir = PathBuf::from("C:/Apps/MyApp");
        let locator = Locator::with_exe_dir(exe_dir.clone());
        let candidates = locator.candidate_paths();
        let names = binary_names();

        assert_eq!(candidates.len(), names.len());
        for (path, name) in candidates.iter().zip(names) {
            assert_eq!(path.parent(), Some(exe_dir.as_path()));
            assert_eq!(path.file_name().unwrap().to_string_lossy(), *name);
        }
    }

    #[test]
    fn binary_names_match_platform() {
        if cfg!(windows) {
            assert_eq!(binary_names(), ["7z.exe"]);
        } else {
            assert!(binary_names().contains(&"7zz"));
            assert!(binary_names().contains(&"7z"));
        }
    }

    #[test]
    fn no_warning_for_fresh_version() {
        assert!(outdated_warnings(Version::new(26, 6, 0)).is_empty());
        assert!(outdated_warnings(Version::new(23, 1, 0)).is_empty());
    }

    #[test]
    fn warns_about_old_but_supported_version() {
        let warnings = outdated_warnings(Version::new(22, 0, 0));
        assert_eq!(warnings.len(), 1);
        let text = warnings[0].message();
        assert!(text.contains("22.00"));
        assert!(text.contains("устарела"));
    }

    #[test]
    fn versions_below_utf8_console_are_rejected() {
        assert!(version_rejection(Version::new(16, 4, 0)).is_some());
        assert!(version_rejection(Version::new(21, 6, 0)).is_some());
        assert!(version_rejection(Version::new(21, 7, 0)).is_none());
        assert!(version_rejection(Version::new(26, 6, 0)).is_none());
        let text = version_rejection(Version::new(16, 4, 0)).unwrap();
        assert!(text.contains("21.07"));
        assert!(text.contains("-sccUTF-8"));
    }

    #[test]
    fn not_found_lists_candidates() {
        let locator = Locator::with_exe_dir(PathBuf::from("C:/nowhere"));
        match locator.resolve() {
            Err(LocateError::NotFound { tried }) => {
                assert_eq!(tried.len(), binary_names().len())
            }
            other => panic!("ожидали NotFound, получили {other:?}"),
        }
    }

    #[test]
    fn not_found_message_names_platform_binary() {
        let err = LocateError::NotFound { tried: Vec::new() };
        assert!(err.to_string().contains(binary_name()));
    }

    #[test]
    fn resolve_returns_cached_tool_without_probing() {
        use crate::version::Arch;

        // Папки не существует: без кеша resolve обязан вернуть NotFound.
        let dir = PathBuf::from("C:/archiver-cache-test/nonexistent");
        let info = ToolInfo {
            exe: dir.join(binary_name()),
            version: Version::new(26, 6, 0),
            arch: Arch::X64,
            loaded_libs: Vec::new(),
        };
        store_cached_tool(
            &dir,
            ResolvedTool {
                info: info.clone(),
                warnings: Vec::new(),
            },
        );

        let resolved = Locator::with_exe_dir(dir.clone()).resolve().unwrap();
        assert_eq!(resolved.info.exe, info.exe);
        assert_eq!(resolved.info.version, info.version);

        tool_cache()
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .remove(&dir);
    }
}

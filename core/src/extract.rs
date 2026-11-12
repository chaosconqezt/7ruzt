//! Распаковка через `7z x` с прогрессом и отменой.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::run::{Progress, RunError, failure_reason, run_7z};

/// Что делать с уже существующими файлами. Интерактивный вопрос «перезаписать?»
/// в CLI-движке невозможен, поэтому режим выбирает вызывающий.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overwrite {
    All,
    Skip,
    Rename,
}

impl Overwrite {
    fn flag(self) -> &'static str {
        match self {
            Self::All => "-aoa",
            Self::Skip => "-aos",
            Self::Rename => "-aou",
        }
    }
}

/// Сохранять ли структуру каталогов архива. `7z x` — с путями, `7z e` — без.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathMode {
    Full,
    Flat,
}

impl PathMode {
    fn command(self) -> &'static str {
        match self {
            Self::Full => "x",
            Self::Flat => "e",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtractRequest {
    pub archive: PathBuf,
    pub destination: PathBuf,
    /// Пусто — распаковать всё; иначе маски/имена внутри архива.
    pub entries: Vec<String>,
    pub password: Option<String>,
    pub overwrite: Overwrite,
    pub path_mode: PathMode,
}

impl ExtractRequest {
    pub fn new(archive: impl Into<PathBuf>, destination: impl Into<PathBuf>) -> Self {
        Self {
            archive: archive.into(),
            destination: destination.into(),
            entries: Vec::new(),
            password: None,
            overwrite: Overwrite::All,
            path_mode: PathMode::Full,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtractSummary {
    /// Код возврата 7z: 0 — успех, 1 — успех с предупреждениями.
    pub code: i32,
    pub warnings: String,
    pub cancelled: bool,
}

#[derive(Debug)]
pub enum ExtractError {
    Spawn(std::io::Error),
    Io(std::io::Error),
    Failed {
        code: i32,
        stderr: String,
        diagnostics: Vec<String>,
    },
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(err) => write!(f, "не удалось запустить 7z: {err}"),
            Self::Io(err) => write!(f, "ошибка ввода-вывода: {err}"),
            Self::Failed {
                code,
                stderr,
                diagnostics,
            } => {
                let reason = failure_reason(stderr, diagnostics);
                if reason.is_empty() {
                    write!(f, "7z завершился с кодом {code}")
                } else {
                    write!(f, "7z завершился с кодом {code}: {reason}")
                }
            }
        }
    }
}

impl std::error::Error for ExtractError {}

/// Аргументы `7z x` для запроса. Вынесено отдельно, чтобы проверять тестами.
///
/// Порядок важен: сначала все переключатели, затем `--`, и только после него пути.
/// Без `--` архив с именем на дефис (`-dash.7z`) воспринимается как переключатель,
/// и 7z падает с «Command Line Error» (код 7).
/// `utf8_console` — есть ли у 7z `-sccUTF-8` (с 21.07).
pub fn build_args(request: &ExtractRequest, utf8_console: bool) -> Vec<String> {
    let mut args = vec![
        request.path_mode.command().to_string(),
        format!("-o{}", request.destination.display()),
        // -y: на все вопросы «да»; -bsp1: прогресс в stdout (7z сам делает
        // fflush); -bb1: имена файлов; -bse1: ошибки в stdout.
        "-y".to_string(),
        "-bsp1".to_string(),
        "-bb1".to_string(),
        "-bse1".to_string(),
        request.overwrite.flag().to_string(),
    ];

    if let Some(password) = &request.password {
        // Пароль в аргументах виден в списке процессов; stdin 7z не читает.
        // Риск принят осознанно — см. doc/gotchas.md.
        args.push(format!("-p{password}"));
    }
    if utf8_console {
        args.push("-sccUTF-8".to_string());
    }

    // Конец переключателей: дальше только имена.
    args.push("--".to_string());
    args.push(request.archive.display().to_string());
    args.extend(request.entries.iter().cloned());
    args
}

/// Распаковывает архив. Блокирующая: вызывающий запускает её в фоновом потоке.
pub fn extract(
    exe: &Path,
    request: &ExtractRequest,
    utf8_console: bool,
    cancelled: &AtomicBool,
    on_progress: impl FnMut(Progress),
) -> Result<ExtractSummary, ExtractError> {
    let args = build_args(request, utf8_console);
    let outcome = run_7z(exe, &args, cancelled, on_progress).map_err(|err| match err {
        RunError::Spawn(err) => ExtractError::Spawn(err),
        RunError::Io(err) => ExtractError::Io(err),
    })?;

    if outcome.cancelled {
        return Ok(ExtractSummary {
            code: outcome.code,
            warnings: outcome.stderr,
            cancelled: true,
        });
    }

    match outcome.code {
        0 => Ok(ExtractSummary {
            code: 0,
            warnings: String::new(),
            cancelled: false,
        }),
        // 1 — «завершено с предупреждениями»: часть файлов не распакована,
        // но это не повод считать операцию проваленной.
        1 => Ok(ExtractSummary {
            code: 1,
            warnings: outcome.warning_text(),
            cancelled: false,
        }),
        code => Err(ExtractError::Failed {
            code,
            stderr: outcome.stderr,
            diagnostics: outcome.diagnostics,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_contain_destination_and_flags() {
        let mut request = ExtractRequest::new("a.7z", "out dir");
        request.overwrite = Overwrite::Skip;
        let args = build_args(&request, true);

        assert_eq!(args[0], "x");
        assert!(args.contains(&"-oout dir".to_string()));
        assert!(args.contains(&"-aos".to_string()));
        assert!(args.contains(&"-bsp1".to_string()));

        // Всё после `--` — имена: сначала архив, потом шаблоны.
        let separator = args.iter().position(|arg| arg == "--").expect("нужен --");
        assert_eq!(args[separator + 1], "a.7z");
        assert_eq!(args.len(), separator + 2, "без шаблонов после архива пусто");
    }

    #[test]
    fn flat_mode_uses_e_command_and_keeps_switches_before_double_dash() {
        let mut request = ExtractRequest::new("a.7z", "out");
        request.path_mode = PathMode::Flat;
        request.password = Some("pw".to_string());
        request.entries = vec!["docs/readme.txt".to_string()];
        let args = build_args(&request, true);

        assert_eq!(args[0], "e");
        let separator = args.iter().position(|arg| arg == "--").expect("нужен --");
        assert!(args.contains(&"-ppw".to_string()));
        assert_eq!(args[separator + 1], "a.7z");
        assert_eq!(args[separator + 2], "docs/readme.txt");
    }

    #[test]
    fn password_and_entries_are_appended() {
        let mut request = ExtractRequest::new("a.7z", "out");
        request.password = Some("секрет".to_string());
        request.entries = vec!["docs/readme.txt".to_string()];
        let args = build_args(&request, true);

        assert!(args.contains(&"-pсекрет".to_string()));
        assert_eq!(args.last().unwrap(), "docs/readme.txt");
    }

    #[test]
    fn utf8_console_flag_follows_version_gate() {
        let request = ExtractRequest::new("a.7z", "out");
        let fresh = build_args(&request, true);
        assert!(fresh.contains(&"-sccUTF-8".to_string()));
        let old = build_args(&request, false);
        assert!(!old.contains(&"-sccUTF-8".to_string()));
        let separator = fresh.iter().position(|arg| arg == "--").unwrap();
        assert!(fresh[..separator].contains(&"-sccUTF-8".to_string()));
    }

    #[test]
    fn switches_come_before_double_dash() {
        let mut request = ExtractRequest::new("-dash.7z", "out");
        request.password = Some("pw".to_string());
        let args = build_args(&request, true);

        let separator = args.iter().position(|arg| arg == "--").expect("нужен --");
        // Все переключатели — строго до `--`, иначе 7z примет их за имена.
        for arg in &args[..separator] {
            assert!(
                arg.starts_with('-') || arg == "x",
                "переключатель до --: {arg}"
            );
        }
        assert_eq!(args[separator + 1], "-dash.7z");
    }
}

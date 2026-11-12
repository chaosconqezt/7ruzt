//! Проверка целостности архива через `7z t`.
//!
//! 7-Zip пересчитывает CRC всех файлов и сам сообщает о повреждённых данных;
//! своего разбора формата у нас нет, поэтому проверка целиком на его стороне.
//!
//! Пароль нужен архиву с зашифрованным содержимым: без `-p` 7z пытается читать
//! его со stdin, а у нас там `nul` — получается «Wrong password?» и код 2.
//! Поэтому вызывающий узнаёт о шифровании заранее из [`crate::Listing::encrypted`]
//! и спрашивает пароль до запуска, как это делает 7-Zip File Manager.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::run::{Progress, RunError, failure_reason, run_7z_captured};

#[derive(Debug, Clone)]
pub struct TestRequest {
    pub archive: PathBuf,
    /// Пусто — проверить весь архив; иначе только перечисленные элементы.
    pub entries: Vec<String>,
    pub password: Option<String>,
}

impl TestRequest {
    pub fn new(archive: impl Into<PathBuf>) -> Self {
        Self {
            archive: archive.into(),
            entries: Vec::new(),
            password: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TestSummary {
    /// Код возврата 7z: 0 — всё цело, 1 — цело, но с замечаниями.
    pub code: i32,
    pub warnings: String,
    pub cancelled: bool,
    /// Итоги из хвоста вывода (`Folders: … Files: … Size: … Compressed: …`).
    pub stats: TestStats,
}

/// Счётчики из хвоста вывода `7z t`. `None` — 7z их не напечатал (формат
/// архива может не отдавать сводку), поэтому в окне такие строки пропускаем.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TestStats {
    pub folders: Option<u64>,
    pub files: Option<u64>,
    /// Несжатый объём данных.
    pub size: Option<u64>,
    /// Сжатый объём (физический размер архива).
    pub compressed: Option<u64>,
}

/// Разбирает сводку из stdout `7z t`.
///
/// Нужны только строки вида `Files: 827`: `Physical Size = …` и
/// `Headers Size = …` идут с `=`, а имена файлов при `-bb1` — с префиксом `T`,
/// поэтому ни те ни другие под счётчики не попадают.
pub fn parse_stats(stdout: &str) -> TestStats {
    let mut stats = TestStats::default();
    for line in stdout.lines() {
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        let Ok(value) = value.trim().parse::<u64>() else {
            continue;
        };
        match key.trim() {
            "Folders" => stats.folders = Some(value),
            "Files" => stats.files = Some(value),
            "Size" => stats.size = Some(value),
            "Compressed" => stats.compressed = Some(value),
            _ => {}
        }
    }
    stats
}

#[derive(Debug)]
pub enum TestError {
    Spawn(std::io::Error),
    Io(std::io::Error),
    /// Код 2 — повреждённые данные или заголовок. Это результат проверки,
    /// а не сбой программы, поэтому рядом с кодом едет текст от 7z.
    Failed {
        code: i32,
        stderr: String,
        diagnostics: Vec<String>,
    },
}

impl std::fmt::Display for TestError {
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

impl std::error::Error for TestError {}

/// Аргументы `7z t`.
///
/// Переключатели строго до `--`, пути после: без этого архив с именем на дефис
/// (`-dash.7z`) 7z принимает за переключатель и падает с «Command Line Error».
/// `utf8_console` — есть ли у 7z `-sccUTF-8` (с 21.07).
pub fn build_args(request: &TestRequest, utf8_console: bool) -> Vec<String> {
    let mut args = vec![
        "t".to_string(),
        // -y: на все вопросы «да»; -bsp1: прогресс в stdout; -bb1: имена
        // файлов; -bse1: ошибки и предупреждения в stdout.
        "-y".to_string(),
        "-bsp1".to_string(),
        "-bb1".to_string(),
        "-bse1".to_string(),
    ];

    if let Some(password) = &request.password {
        // Пароль в аргументах виден в списке процессов; stdin 7z не читает.
        // Риск принят осознанно — см. doc/gotchas.md.
        args.push(format!("-p{password}"));
    }
    if utf8_console {
        args.push("-sccUTF-8".to_string());
    }

    args.push("--".to_string());
    args.push(request.archive.display().to_string());
    args.extend(request.entries.iter().cloned());
    args
}

/// Проверяет архив. Блокирующая: вызывающий запускает её в фоновом потоке.
pub fn test_archive(
    exe: &Path,
    request: &TestRequest,
    utf8_console: bool,
    cancelled: &AtomicBool,
    on_progress: impl FnMut(Progress),
) -> Result<TestSummary, TestError> {
    let args = build_args(request, utf8_console);
    let outcome = run_7z_captured(exe, &args, cancelled, on_progress).map_err(|err| match err {
        RunError::Spawn(err) => TestError::Spawn(err),
        RunError::Io(err) => TestError::Io(err),
    })?;

    if outcome.cancelled {
        return Ok(TestSummary {
            code: outcome.code,
            warnings: outcome.stderr,
            cancelled: true,
            stats: TestStats::default(),
        });
    }

    match outcome.code {
        0 => Ok(TestSummary {
            code: 0,
            warnings: String::new(),
            cancelled: false,
            stats: parse_stats(&outcome.stdout),
        }),
        // 1 — «завершено с предупреждениями»: проверка прошла, но не для всего.
        1 => Ok(TestSummary {
            code: 1,
            warnings: outcome.warning_text(),
            cancelled: false,
            stats: parse_stats(&outcome.stdout),
        }),
        code => Err(TestError::Failed {
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
    fn args_start_with_test_command_and_end_with_archive() {
        let request = TestRequest::new("sample.7z");
        let args = build_args(&request, true);

        assert_eq!(args[0], "t");
        assert!(args.contains(&"-bsp1".to_string()));
        assert!(args.contains(&"-bb1".to_string()));

        let separator = args.iter().position(|arg| arg == "--").expect("нужен --");
        assert_eq!(args[separator + 1], "sample.7z");
        assert_eq!(
            args.len(),
            separator + 2,
            "без выделения после архива пусто"
        );
    }

    #[test]
    fn password_and_entries_are_appended() {
        let mut request = TestRequest::new("a.7z");
        request.password = Some("секрет".to_string());
        request.entries = vec!["docs/readme.txt".to_string()];

        let args = build_args(&request, true);
        assert!(args.contains(&"-pсекрет".to_string()));
        assert_eq!(args.last().unwrap(), "docs/readme.txt");
    }

    #[test]
    fn dash_named_archive_is_protected_by_double_dash() {
        let request = TestRequest::new("-dash.7z");
        let args = build_args(&request, true);

        let separator = args.iter().position(|arg| arg == "--").expect("нужен --");
        // Всё до `--` — переключатели (или сама команда), иначе 7z примет
        // имя архива за ключ.
        for arg in &args[..separator] {
            assert!(
                arg.starts_with('-') || arg == "t",
                "переключатель до --: {arg}"
            );
        }
        assert_eq!(args[separator + 1], "-dash.7z");
    }

    #[test]
    fn utf8_console_flag_follows_version_gate() {
        let request = TestRequest::new("a.7z");
        let fresh = build_args(&request, true);
        assert!(fresh.contains(&"-sccUTF-8".to_string()));
        let old = build_args(&request, false);
        assert!(!old.contains(&"-sccUTF-8".to_string()));
        let separator = fresh.iter().position(|arg| arg == "--").unwrap();
        assert!(fresh[..separator].contains(&"-sccUTF-8".to_string()));
    }

    #[test]
    fn failure_message_explains_stdout_only_errors() {
        // Битый заголовок: stderr пуст, всё объяснение в stdout.
        let error = TestError::Failed {
            code: 2,
            stderr: String::new(),
            diagnostics: vec!["ERRORS: Headers Error".to_string()],
        };
        assert_eq!(
            error.to_string(),
            "7z завершился с кодом 2: ERRORS: Headers Error"
        );
    }

    /// Настоящий хвост `7z t` (vox.7z, 7-Zip 26.06).
    const REAL_OUTPUT: &str = "Scanning the drive for archives:
1 file, 45186209 bytes (44 MiB)

Testing archive: vox.7z
--
Path = vox.7z
Type = 7z
Physical Size = 45186209
Headers Size = 2254
Method = LZMA2:24
Solid = +
Blocks = 1

Everything is Ok

Folders: 142
Files: 827
Size:       64812481
Compressed: 45186209
";

    #[test]
    fn parses_summary_counters() {
        assert_eq!(
            parse_stats(REAL_OUTPUT),
            TestStats {
                folders: Some(142),
                files: Some(827),
                size: Some(64812481),
                compressed: Some(45186209),
            }
        );
    }

    #[test]
    fn headers_and_file_names_are_not_counters() {
        // `Physical Size = …` — с `=`, `T Files: …` — имя файла, `LZMA2:24` — метод.
        let output = "Physical Size = 123\nHeaders Size = 45\nT Files: 999\nMethod = LZMA2:24\n";
        assert_eq!(parse_stats(output), TestStats::default());
    }

    #[test]
    fn missing_summary_stays_none() {
        assert_eq!(parse_stats("Everything is Ok\n"), TestStats::default());
        assert_eq!(parse_stats(""), TestStats::default());
    }
}

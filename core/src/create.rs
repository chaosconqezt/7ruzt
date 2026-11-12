//! Создание архива через `7z a`.
//!
//! Что умеем: уровень сжатия, метод и словарь, число потоков, solid-блок,
//! пароль (с шифрованием имён), разбиение на тома, полные пути и сжатие
//! открытых на запись файлов. Формат выбирается по расширению: ZIP или 7z.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::run::{Progress, RunError, failure_reason, run_7z};

/// Уровень сжатия 7-Zip: 0 (без сжатия) … 9 (максимум). Пользователь видит
/// привычную шкалу, в аргументы уходит `-mx=N`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Store,
    Fastest,
    Fast,
    Normal,
    Maximum,
    Ultra,
}

impl Level {
    pub fn value(self) -> u8 {
        match self {
            Self::Store => 0,
            Self::Fastest => 1,
            Self::Fast => 3,
            Self::Normal => 5,
            Self::Maximum => 7,
            Self::Ultra => 9,
        }
    }

    pub fn label_ru(self) -> &'static str {
        match self {
            Self::Store => "Без сжатия",
            Self::Fastest => "Самый быстрый",
            Self::Fast => "Быстрый",
            Self::Normal => "Обычный",
            Self::Maximum => "Максимальный",
            Self::Ultra => "Ультра",
        }
    }
}

/// Метод сжатия внутри 7z. В аргументы уходит `-m0=<id>`.
///
/// Список — методы самого 7z, а не абстракция: у каждого свои ограничения
/// (словарь есть только у LZMA/LZMA2), и врать интерфейсом про «любой метод»
/// не надо. `Store` — 7-Zip называет его `copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Lzma2,
    Lzma,
    Ppmd,
    BZip2,
    Deflate,
    Store,
}

impl Method {
    /// Значение для `-m0=`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Lzma2 => "lzma2",
            Self::Lzma => "lzma",
            Self::Ppmd => "ppmd",
            Self::BZip2 => "bzip2",
            Self::Deflate => "deflate",
            Self::Store => "copy",
        }
    }

    pub fn label_ru(self) -> &'static str {
        match self {
            Self::Lzma2 => "LZMA2",
            Self::Lzma => "LZMA",
            Self::Ppmd => "PPMd",
            Self::BZip2 => "BZip2",
            Self::Deflate => "Deflate",
            Self::Store => "Copy",
        }
    }

    /// Понимает ли метод размер словаря (`-md`). У PPMd/BZip2/Deflate/Copy
    /// такого параметра нет — передавать его туда значит получить ошибку 7z.
    pub fn has_dictionary(self) -> bool {
        matches!(self, Self::Lzma2 | Self::Lzma)
    }
}

#[derive(Debug, Clone)]
pub struct CreateRequest {
    pub destination: PathBuf,
    /// Что кладём в архив: файлы и каталоги как есть.
    pub sources: Vec<PathBuf>,
    pub level: Level,
    pub method: Method,
    /// Размер словаря для LZMA/LZMA2 (`-md64m`). `None` — как решит 7z.
    pub dictionary: Option<String>,
    /// Число потоков (`-mmt=4`). `None` — все ядра (`-mmt=on`).
    pub threads: Option<u32>,
    pub solid: bool,
    /// `-spf`: хранить полные пути, а не относительные.
    pub full_paths: bool,
    /// `-ssw`: сжимать файлы, открытые на запись.
    pub compress_open: bool,
    /// Пароль для шифрования содержимого.
    pub password: Option<String>,
    /// Шифрование имён файлов (`-mhe=on`): только 7z и с непустым паролем.
    pub encrypt_names: bool,
    /// Размер тома: `-v100m`, `-v1g`. Пусто — один файл.
    pub volume: Option<String>,
}

impl CreateRequest {
    pub fn new(destination: impl Into<PathBuf>, sources: Vec<PathBuf>) -> Self {
        Self {
            destination: destination.into(),
            sources,
            level: Level::Normal,
            method: Method::Lzma2,
            dictionary: None,
            threads: None,
            solid: true,
            full_paths: false,
            compress_open: false,
            password: None,
            encrypt_names: false,
            volume: None,
        }
    }

    pub fn validate(&self) -> Result<(), CreateError> {
        if !self.encrypt_names {
            return Ok(());
        }
        if self.password.as_deref().is_none_or(str::is_empty) {
            return Err(CreateError::InvalidInput(
                "Для шифрования имён файлов требуется непустой пароль.".to_string(),
            ));
        }
        if self.is_zip() {
            return Err(CreateError::InvalidInput(
                "Формат ZIP не поддерживает шифрование имён файлов. Используйте формат 7z."
                    .to_string(),
            ));
        }
        Ok(())
    }

    fn is_zip(&self) -> bool {
        self.destination
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    }
}

#[derive(Debug, Clone)]
pub struct CreateSummary {
    pub code: i32,
    pub warnings: String,
    pub cancelled: bool,
}

#[derive(Debug)]
pub enum CreateError {
    InvalidInput(String),
    Spawn(std::io::Error),
    Io(std::io::Error),
    Failed {
        code: i32,
        stderr: String,
        diagnostics: Vec<String>,
    },
}

impl std::fmt::Display for CreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(message) => f.write_str(message),
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

impl std::error::Error for CreateError {}

/// `utf8_console` — есть ли у 7z `-sccUTF-8` (с 21.07): без него имена файлов
/// в прогрессе приходят в системной кодировке и ломаются.
pub fn build_args(request: &CreateRequest, utf8_console: bool) -> Vec<String> {
    // Переключатели идут первыми, `--` отделяет их от имён. Без `--` архив
    // с именем на дефис (`-dash.7z`) 7z принимает за переключатель и падает
    // с «Command Line Error» (код 7).
    let mut args = vec!["a".to_string()];
    if request.is_zip() {
        args.push("-tzip".to_string());
    } else {
        args.push("-t7z".to_string());
    }

    args.push(format!("-mx={}", request.level.value()));
    args.push(format!("-m0={}", request.method.id()));
    // Словарь понимают только LZMA/LZMA2: остальным `-md` — это другой параметр
    // или ошибка, поэтому молча не передаём.
    if request.method.has_dictionary()
        && let Some(dictionary) = request.dictionary.as_deref()
        && !dictionary.is_empty()
    {
        args.push(format!("-md{dictionary}"));
    }
    args.push(if request.solid {
        "-ms=on".to_string()
    } else {
        "-ms=off".to_string()
    });
    args.push(match request.threads {
        Some(threads) => format!("-mmt={threads}"),
        None => "-mmt=on".to_string(),
    });

    if request.full_paths {
        args.push("-spf".to_string());
    }
    if request.compress_open {
        args.push("-ssw".to_string());
    }

    if let Some(password) = &request.password {
        // Пароль в аргументах виден в списке процессов; stdin 7z не читает.
        // Риск принят осознанно — см. doc/gotchas.md.
        args.push(format!("-p{password}"));
        if request.encrypt_names {
            args.push("-mhe=on".to_string());
        }
    }

    if let Some(volume) = &request.volume {
        args.push(format!("-v{volume}"));
    }

    args.extend([
        "-y".to_string(),
        "-bsp1".to_string(),
        "-bb1".to_string(),
        "-bse1".to_string(),
    ]);
    if utf8_console {
        args.push("-sccUTF-8".to_string());
    }

    // Конец переключателей: дальше только имена.
    args.push("--".to_string());
    args.push(request.destination.display().to_string());
    args.extend(
        request
            .sources
            .iter()
            .map(|path| path.display().to_string()),
    );

    args
}

/// Создаёт архив. Блокирующая: вызывающий запускает её в фоновом потоке.
pub fn create(
    exe: &Path,
    request: &CreateRequest,
    utf8_console: bool,
    cancelled: &AtomicBool,
    on_progress: impl FnMut(Progress),
) -> Result<CreateSummary, CreateError> {
    request.validate()?;
    let args = build_args(request, utf8_console);
    let outcome = run_7z(exe, &args, cancelled, on_progress).map_err(|err| match err {
        RunError::Spawn(err) => CreateError::Spawn(err),
        RunError::Io(err) => CreateError::Io(err),
    })?;

    if outcome.cancelled {
        return Ok(CreateSummary {
            code: outcome.code,
            warnings: outcome.stderr,
            cancelled: true,
        });
    }

    match outcome.code {
        0 => Ok(CreateSummary {
            code: 0,
            warnings: String::new(),
            cancelled: false,
        }),
        1 => Ok(CreateSummary {
            code: 1,
            warnings: outcome.warning_text(),
            cancelled: false,
        }),
        code => Err(CreateError::Failed {
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
    fn header_encryption_requires_nonempty_password() {
        for password in [None, Some(String::new())] {
            let mut request = CreateRequest::new("out.7z", vec![PathBuf::from("a.bin")]);
            request.encrypt_names = true;
            request.password = password;

            let error = request.validate().unwrap_err();
            assert!(matches!(&error, CreateError::InvalidInput(_)));
            assert_eq!(
                error.to_string(),
                "Для шифрования имён файлов требуется непустой пароль."
            );
        }
    }

    #[test]
    fn zip_rejects_header_encryption_case_insensitively() {
        for destination in ["out.zip", "out.ZIP", "out.Zip"] {
            let mut request = CreateRequest::new(destination, vec![PathBuf::from("a.bin")]);
            request.encrypt_names = true;
            request.password = Some("pw".to_string());

            let error = request.validate().unwrap_err();
            assert!(matches!(&error, CreateError::InvalidInput(_)));
            assert_eq!(
                error.to_string(),
                "Формат ZIP не поддерживает шифрование имён файлов. Используйте формат 7z."
            );
        }
    }

    #[test]
    fn header_encryption_accepts_nonempty_password_without_trimming() {
        for password in ["pw", " "] {
            let mut request = CreateRequest::new("out.7z", vec![PathBuf::from("a.bin")]);
            request.encrypt_names = true;
            request.password = Some(password.to_string());

            assert!(request.validate().is_ok());
            assert_eq!(request.password.as_deref(), Some(password));
        }
    }

    #[test]
    fn without_header_encryption_validation_preserves_existing_requests() {
        for destination in ["out.7z", "out.zip"] {
            for password in [None, Some(String::new()), Some("pw".to_string())] {
                let mut request = CreateRequest::new(destination, vec![PathBuf::from("a.bin")]);
                request.password = password;
                assert!(request.validate().is_ok());
            }
        }
    }

    #[test]
    fn args_have_format_sources_and_level() {
        let request = CreateRequest::new(
            "out.7z",
            vec![PathBuf::from("docs"), PathBuf::from("a.bin")],
        );
        let args = build_args(&request, true);

        assert_eq!(args[0], "a");
        assert_eq!(args[1], "-t7z");
        assert!(args.contains(&"-mx=5".to_string()));
        assert!(args.contains(&"-ms=on".to_string()));
        assert!(args.contains(&"-mmt=on".to_string()));

        // Архив и источники — строго после `--`.
        let separator = args.iter().position(|arg| arg == "--").expect("нужен --");
        assert_eq!(args[separator + 1], "out.7z");
        assert!(args[separator + 2..].contains(&"docs".to_string()));
        assert!(args[separator + 2..].contains(&"a.bin".to_string()));
    }

    #[test]
    fn dash_named_archive_is_protected_by_double_dash() {
        let request = CreateRequest::new("-dash.7z", vec![PathBuf::from("a.bin")]);
        let args = build_args(&request, true);

        let separator = args.iter().position(|arg| arg == "--").expect("нужен --");
        for arg in &args[..separator] {
            assert!(
                arg.starts_with('-') || arg == "a",
                "переключатель до --: {arg}"
            );
        }
        assert_eq!(args[separator + 1], "-dash.7z");
    }

    #[test]
    fn utf8_console_flag_follows_version_gate() {
        let request = CreateRequest::new("out.7z", vec![PathBuf::from("a.bin")]);
        let fresh = build_args(&request, true);
        assert!(fresh.contains(&"-sccUTF-8".to_string()));
        let old = build_args(&request, false);
        assert!(!old.contains(&"-sccUTF-8".to_string()));
        let separator = fresh.iter().position(|arg| arg == "--").unwrap();
        assert!(fresh[..separator].contains(&"-sccUTF-8".to_string()));
    }

    #[test]
    fn zip_archive_uses_zip_format() {
        let request = CreateRequest::new("bundle.zip", vec![PathBuf::from("a.txt")]);
        let args = build_args(&request, true);
        assert_eq!(args[0], "a");
        assert_eq!(args[1], "-tzip");
    }

    #[test]
    fn password_adds_header_encryption_only_when_asked() {
        let mut request = CreateRequest::new("out.7z", vec![PathBuf::from("a.bin")]);
        request.password = Some("pw".to_string());

        let plain = build_args(&request, true);
        assert!(plain.contains(&"-ppw".to_string()));
        assert!(!plain.iter().any(|arg| arg == "-mhe=on"));

        request.encrypt_names = true;
        let encrypted = build_args(&request, true);
        assert!(encrypted.contains(&"-mhe=on".to_string()));
    }

    #[test]
    fn volume_and_no_solid_are_passed() {
        let mut request = CreateRequest::new("out.7z", vec![PathBuf::from("a.bin")]);
        request.volume = Some("100m".to_string());
        request.solid = false;
        request.level = Level::Ultra;

        let args = build_args(&request, true);
        assert!(args.contains(&"-v100m".to_string()));
        assert!(args.contains(&"-ms=off".to_string()));
        assert!(args.contains(&"-mx=9".to_string()));
    }

    #[test]
    fn level_and_method_are_passed() {
        let mut request = CreateRequest::new("out.7z", vec![PathBuf::from("a.bin")]);
        request.level = Level::Ultra;
        request.method = Method::Lzma;
        request.dictionary = Some("64m".to_string());
        request.threads = Some(4);

        let args = build_args(&request, true);
        assert!(args.contains(&"-mx=9".to_string()));
        assert!(args.contains(&"-m0=lzma".to_string()));
        assert!(args.contains(&"-md64m".to_string()));
        assert!(args.contains(&"-mmt=4".to_string()));
    }

    #[test]
    fn dictionary_is_not_sent_to_methods_without_one() {
        let mut request = CreateRequest::new("out.7z", vec![PathBuf::from("a.bin")]);
        request.method = Method::BZip2;
        request.dictionary = Some("64m".to_string());

        let args = build_args(&request, true);
        assert!(args.contains(&"-m0=bzip2".to_string()));
        assert!(
            !args.iter().any(|arg| arg.starts_with("-md")),
            "у BZip2 нет размера словаря: {args:?}"
        );
    }

    #[test]
    fn paths_and_shared_files_flags() {
        let mut request = CreateRequest::new("out.7z", vec![PathBuf::from("a.bin")]);
        request.full_paths = true;
        request.compress_open = true;

        let args = build_args(&request, true);
        assert!(args.contains(&"-spf".to_string()));
        assert!(args.contains(&"-ssw".to_string()));

        request.full_paths = false;
        request.compress_open = false;
        let plain = build_args(&request, true);
        assert!(!plain.contains(&"-spf".to_string()));
        assert!(!plain.contains(&"-ssw".to_string()));
    }

    #[test]
    fn levels_map_to_7zip_scale() {
        assert_eq!(Level::Store.value(), 0);
        assert_eq!(Level::Normal.value(), 5);
        assert_eq!(Level::Ultra.value(), 9);
    }
}

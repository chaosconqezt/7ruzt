//! Headless-вход в ядро. Нужен, чтобы проверять окружение и гонять тесты
//! там, где GUI не запустить (CI на macOS/Linux, ssh-сессия, чужой комп).

use std::path::PathBuf;
use std::process::ExitCode;

use archiver_core::{LocateError, Locator, binary_name, list_dir};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);

    match args.next().as_deref() {
        Some("tool-info") => {
            let rest: Vec<String> = args.collect();
            match tool_info(&rest) {
                Ok(code) => code,
                Err(message) => {
                    eprintln!("ошибка: {message}");
                    ExitCode::from(2)
                }
            }
        }
        Some("list") => {
            let rest: Vec<String> = args.collect();
            match list_path(&rest) {
                Ok(code) => code,
                Err(message) => {
                    eprintln!("ошибка: {message}");
                    ExitCode::from(2)
                }
            }
        }
        Some("extract") => {
            let rest: Vec<String> = args.collect();
            match extract_archive(&rest) {
                Ok(code) => code,
                Err(message) => {
                    eprintln!("ошибка: {message}");
                    ExitCode::from(2)
                }
            }
        }
        Some("create") => {
            let rest: Vec<String> = args.collect();
            match create_archive(&rest) {
                Ok(code) => code,
                Err(message) => {
                    eprintln!("ошибка: {message}");
                    ExitCode::from(2)
                }
            }
        }
        Some("test") => {
            let rest: Vec<String> = args.collect();
            match test_archive(&rest) {
                Ok(code) => code,
                Err(message) => {
                    eprintln!("ошибка: {message}");
                    ExitCode::from(2)
                }
            }
        }
        Some("hash") => {
            let rest: Vec<String> = args.collect();
            match hash_paths(&rest) {
                Ok(code) => code,
                Err(message) => {
                    eprintln!("ошибка: {message}");
                    ExitCode::from(2)
                }
            }
        }
        Some("--help" | "-h") | None => {
            usage();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("неизвестная команда: {other}\n");
            usage();
            ExitCode::from(2)
        }
    }
}

/// `tool-info [--exe-dir <папка>]`
fn tool_info(args: &[String]) -> Result<ExitCode, String> {
    let mut exe_dir: Option<PathBuf> = None;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--exe-dir" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "флаг --exe-dir требует путь".to_string())?;
                exe_dir = Some(PathBuf::from(value));
            }
            other => return Err(format!("неизвестный флаг: {other}")),
        }
        index += 1;
    }

    let locator = match exe_dir {
        Some(dir) => Locator::with_exe_dir(dir),
        None => Locator::for_current_exe().map_err(|err| err.to_string())?,
    };

    match locator.resolve() {
        Ok(tool) => {
            println!("бинарь   : {}", tool.info.exe.display());
            println!("версия   : {} ({})", tool.info.version, tool.info.arch);

            if tool.info.has_codec_dll() {
                for lib in &tool.info.loaded_libs {
                    println!("кодеки   : {}", lib.display());
                }
            } else if cfg!(windows) {
                println!("кодеки   : 7z.dll не загружен — доступен только формат 7z");
            } else {
                // На unix 7zz — самодостаточный бинарь, отдельных библиотек нет.
                println!("кодеки   : встроены в {}", binary_name());
            }

            if tool.warnings.is_empty() {
                println!("статус   : ok");
            } else {
                for warning in &tool.warnings {
                    println!("внимание : {}", warning.message());
                }
            }

            Ok(ExitCode::SUCCESS)
        }
        Err(err) => {
            eprintln!("{err}");
            if let LocateError::NotFound { tried } = &err {
                const SHOW: usize = 6;
                eprintln!("искал:");
                for path in tried.iter().take(SHOW) {
                    eprintln!("  {}", path.display());
                }
                if tried.len() > SHOW {
                    eprintln!("  ... и ещё {}", tried.len() - SHOW);
                }
                eprintln!("\nисточник 7-Zip: https://www.7-zip.org/download.html");
            }
            Ok(ExitCode::from(1))
        }
    }
}

/// `list <путь> [--sort name|type|size|modified|created] [--desc]` — каталог или архив, без GUI.
fn list_path(args: &[String]) -> Result<ExitCode, String> {
    let mut target: Option<String> = None;
    let mut sort_key = archiver_core::SortKey::Name;
    let mut descending = false;

    for arg in args {
        match arg.as_str() {
            "--desc" => descending = true,
            other => {
                if let Some(value) = other.strip_prefix("--sort=") {
                    sort_key = parse_sort_key(value)?;
                } else if other.starts_with("--") {
                    return Err(format!("неизвестный флаг: {other}"));
                } else {
                    target = Some(other.to_string());
                }
            }
        }
    }

    let target = target.ok_or_else(|| "укажи путь: файл архива или каталог".to_string())?;
    let path = PathBuf::from(target);

    let mut entries = if path.is_dir() {
        list_dir(&path).map_err(|err| format!("не удалось прочитать {}: {err}", path.display()))?
    } else {
        let locator = Locator::for_current_exe().map_err(|err| err.to_string())?;
        let tool = locator.resolve().map_err(|err| {
            format!(
                "{err}\nподсказка: положи `{}` рядом с программой",
                binary_name()
            )
        })?;
        let listing =
            archiver_core::list_archive(&tool.info.exe, &path).map_err(|err| err.to_string())?;
        println!(
            "архив: {} ({}), записей: {}",
            listing.archive.display(),
            listing.kind.as_deref().unwrap_or("?"),
            listing.entries.len()
        );
        archiver_core::view_at(&listing.entries, "")
    };

    // Тот же `sort_entries`, что и в GUI: каталоги сверху, дальше по выбранной колонке.
    archiver_core::sort_entries(&mut entries, sort_key, !descending);

    for entry in &entries {
        println!(
            "{:>12}  {:<16}  {}{}",
            entry.size_text(),
            entry.modified.as_deref().unwrap_or(""),
            entry.name,
            if entry.is_dir { "/" } else { "" }
        );
    }
    println!("всего: {}", entries.len());
    Ok(ExitCode::SUCCESS)
}

fn parse_sort_key(raw: &str) -> Result<archiver_core::SortKey, String> {
    match raw {
        "name" => Ok(archiver_core::SortKey::Name),
        "type" => Ok(archiver_core::SortKey::Type),
        "size" => Ok(archiver_core::SortKey::Size),
        "modified" => Ok(archiver_core::SortKey::Modified),
        "created" => Ok(archiver_core::SortKey::Created),
        other => Err(format!(
            "неизвестная колонка: {other} (name|type|size|modified|created)"
        )),
    }
}

/// `extract <архив> <куда> [--password=…] [--skip-existing] [--verbose] [--cancel-after-ms=N]`
///
/// Печатает прогресс в stdout: проценты и, с `--verbose`, имена файлов.
/// `--cancel-after-ms` — для проверки отмены: через N мс операция убивается.
fn extract_archive(args: &[String]) -> Result<ExitCode, String> {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    let mut archive: Option<PathBuf> = None;
    let mut destination: Option<PathBuf> = None;
    let mut password: Option<String> = None;
    let mut skip_existing = false;
    let mut verbose = false;
    let mut cancel_after_ms: Option<u64> = None;

    for arg in args {
        if let Some(value) = arg.strip_prefix("--password=") {
            password = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("--cancel-after-ms=") {
            cancel_after_ms = Some(value.parse().map_err(|_| "плохое число мс")?);
        } else if arg == "--skip-existing" {
            skip_existing = true;
        } else if arg == "--verbose" {
            verbose = true;
        } else if arg.starts_with("--") {
            return Err(format!("неизвестный флаг: {arg}"));
        } else if archive.is_none() {
            archive = Some(PathBuf::from(arg));
        } else if destination.is_none() {
            destination = Some(PathBuf::from(arg));
        } else {
            return Err(format!("лишний аргумент: {arg}"));
        }
    }

    let archive = archive.ok_or_else(|| "укажи архив".to_string())?;
    let destination = destination.ok_or_else(|| "укажи, куда распаковывать".to_string())?;

    let locator = Locator::for_current_exe().map_err(|err| err.to_string())?;
    let tool = locator.resolve().map_err(|err| {
        format!(
            "{err}\nподсказка: положи `{}` рядом с программой",
            binary_name()
        )
    })?;

    let mut request = archiver_core::ExtractRequest::new(&archive, &destination);
    request.password = password;
    if skip_existing {
        request.overwrite = archiver_core::Overwrite::Skip;
    }

    let cancelled = Arc::new(AtomicBool::new(false));
    if let Some(ms) = cancel_after_ms {
        let flag = Arc::clone(&cancelled);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(ms));
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        });
    }

    println!(
        "распаковка: {} → {}",
        archive.display(),
        destination.display()
    );
    let mut shown = 0u8;
    let started = std::time::Instant::now();

    let result = archiver_core::extract(
        &tool.info.exe,
        &request,
        tool.info.version.supports_utf8_console(),
        &cancelled,
        |progress| match progress {
            archiver_core::Progress::Percent(value) => {
                if verbose || value >= shown + 10 || value == 100 {
                    println!("  {value}%");
                    shown = value;
                }
            }
            archiver_core::Progress::Item(name) => {
                if verbose {
                    println!("  {name}");
                }
            }
        },
    );

    match result {
        Ok(summary) if summary.cancelled => {
            println!("отменено через {} мс", started.elapsed().as_millis());
            Ok(ExitCode::from(1))
        }
        Ok(summary) => {
            println!(
                "готово: код {}, {} мс",
                summary.code,
                started.elapsed().as_millis()
            );
            if !summary.warnings.trim().is_empty() {
                println!("предупреждения: {}", summary.warnings.trim());
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(err) => {
            eprintln!("не удалось: {err}");
            Ok(ExitCode::from(2))
        }
    }
}

/// `create <куда.7z> <что…> [--level=N] [--no-solid] [--password=…] [--encrypt-names] [--volume=…]`
fn create_archive(args: &[String]) -> Result<ExitCode, String> {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    let mut destination: Option<PathBuf> = None;
    let mut sources: Vec<PathBuf> = Vec::new();
    let mut level = archiver_core::Level::Normal;
    let mut solid = true;
    let mut password: Option<String> = None;
    let mut encrypt_names = false;
    let mut volume: Option<String> = None;
    let mut verbose = false;

    for arg in args {
        if let Some(value) = arg.strip_prefix("--password=") {
            password = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("--volume=") {
            volume = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("--level=") {
            level = match value {
                "0" => archiver_core::Level::Store,
                "1" => archiver_core::Level::Fastest,
                "3" => archiver_core::Level::Fast,
                "5" => archiver_core::Level::Normal,
                "7" => archiver_core::Level::Maximum,
                "9" => archiver_core::Level::Ultra,
                other => return Err(format!("уровень бывает 0|1|3|5|7|9, а не {other}")),
            };
        } else if arg == "--no-solid" {
            solid = false;
        } else if arg == "--encrypt-names" {
            encrypt_names = true;
        } else if arg == "--verbose" {
            verbose = true;
        } else if arg.starts_with("--") {
            return Err(format!("неизвестный флаг: {arg}"));
        } else if destination.is_none() {
            destination = Some(PathBuf::from(arg));
        } else {
            sources.push(PathBuf::from(arg));
        }
    }

    let destination = destination.ok_or_else(|| "укажи, куда создавать архив".to_string())?;
    if sources.is_empty() {
        return Err("укажи хотя бы один файл или каталог".to_string());
    }

    let locator = Locator::for_current_exe().map_err(|err| err.to_string())?;
    let tool = locator.resolve().map_err(|err| {
        format!(
            "{err}\nподсказка: положи `{}` рядом с программой",
            binary_name()
        )
    })?;

    let mut request = archiver_core::CreateRequest::new(&destination, sources);
    request.level = level;
    request.solid = solid;
    request.password = password;
    request.encrypt_names = encrypt_names;
    request.volume = volume;

    let cancelled = Arc::new(AtomicBool::new(false));
    println!(
        "создание: {} (уровень {} — {})",
        destination.display(),
        level.value(),
        level.label_ru()
    );

    let mut shown = 0u8;
    let started = std::time::Instant::now();

    let result = archiver_core::create(
        &tool.info.exe,
        &request,
        tool.info.version.supports_utf8_console(),
        &cancelled,
        |progress| match progress {
            archiver_core::Progress::Percent(value) => {
                if verbose || value >= shown + 10 || value == 100 {
                    println!("  {value}%");
                    shown = value;
                }
            }
            archiver_core::Progress::Item(name) => {
                if verbose {
                    println!("  {name}");
                }
            }
        },
    );

    match result {
        Ok(summary) if summary.cancelled => {
            println!("отменено через {} мс", started.elapsed().as_millis());
            Ok(ExitCode::from(1))
        }
        Ok(summary) => {
            println!(
                "готово: код {}, {} мс",
                summary.code,
                started.elapsed().as_millis()
            );
            if !summary.warnings.trim().is_empty() {
                println!("предупреждения: {}", summary.warnings.trim());
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(err) => {
            eprintln!("не удалось: {err}");
            Ok(ExitCode::from(2))
        }
    }
}
/// `test <архив> [--password=…] [--verbose] [--cancel-after-ms=N]`
///
/// `--cancel-after-ms` — для проверки отмены: через N мс операция убивается.
fn test_archive(args: &[String]) -> Result<ExitCode, String> {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    let mut archive: Option<PathBuf> = None;
    let mut password: Option<String> = None;
    let mut verbose = false;
    let mut cancel_after_ms: Option<u64> = None;

    for arg in args {
        if let Some(value) = arg.strip_prefix("--password=") {
            password = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("--cancel-after-ms=") {
            cancel_after_ms = Some(value.parse().map_err(|_| "плохое число мс")?);
        } else if arg == "--verbose" {
            verbose = true;
        } else if arg.starts_with("--") {
            return Err(format!("неизвестный флаг: {arg}"));
        } else if archive.is_none() {
            archive = Some(PathBuf::from(arg));
        } else {
            return Err(format!("лишний аргумент: {arg}"));
        }
    }

    let archive = archive.ok_or_else(|| "укажи архив".to_string())?;

    let locator = Locator::for_current_exe().map_err(|err| err.to_string())?;
    let tool = locator.resolve().map_err(|err| {
        format!(
            "{err}\nподсказка: положи `{}` рядом с программой",
            binary_name()
        )
    })?;

    let mut request = archiver_core::TestRequest::new(&archive);
    request.password = password;

    let cancelled = Arc::new(AtomicBool::new(false));
    if let Some(ms) = cancel_after_ms {
        let flag = Arc::clone(&cancelled);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(ms));
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        });
    }

    println!("проверка: {}", archive.display());

    let mut shown = 0u8;
    let started = std::time::Instant::now();

    let result = archiver_core::test_archive(
        &tool.info.exe,
        &request,
        tool.info.version.supports_utf8_console(),
        &cancelled,
        |progress| match progress {
            archiver_core::Progress::Percent(value) => {
                if verbose || value >= shown + 10 || value == 100 {
                    println!("  {value}%");
                    shown = value;
                }
            }
            archiver_core::Progress::Item(name) => {
                if verbose {
                    println!("  {name}");
                }
            }
        },
    );

    match result {
        Ok(summary) if summary.cancelled => {
            println!("отменено через {} мс", started.elapsed().as_millis());
            Ok(ExitCode::from(1))
        }
        Ok(summary) if summary.code == 0 => {
            println!(
                "архив цел: код {}, {} мс",
                summary.code,
                started.elapsed().as_millis()
            );
            Ok(ExitCode::SUCCESS)
        }
        Ok(summary) => {
            // Код 1 — «проверено, но не всё»: в скриптах это не успех.
            println!(
                "архив проверен с предупреждениями: код {}, {} мс",
                summary.code,
                started.elapsed().as_millis()
            );
            if !summary.warnings.trim().is_empty() {
                println!("предупреждения: {}", summary.warnings.trim());
            }
            Ok(ExitCode::from(1))
        }
        Err(err) => {
            eprintln!("проверка не пройдена: {err}");
            Ok(ExitCode::from(2))
        }
    }
}

/// `hash <что…> [--algo=sha256]` — контрольные суммы через `7z h`.
/// Алгоритмы: crc32, crc64, xxh64, md5, sha1, sha256, sha384, sha512, sha3-256,
/// blake2sp, `*` (все сразу). По умолчанию sha256.
fn hash_paths(args: &[String]) -> Result<ExitCode, String> {
    use archiver_core::HashAlgorithm;

    let mut paths: Vec<PathBuf> = Vec::new();
    let mut algorithm = HashAlgorithm::Sha256;

    for arg in args {
        if let Some(value) = arg.strip_prefix("--algo=") {
            algorithm = match value.to_ascii_lowercase().as_str() {
                "crc32" => HashAlgorithm::Crc32,
                "crc64" => HashAlgorithm::Crc64,
                "xxh64" => HashAlgorithm::Xxh64,
                "md5" => HashAlgorithm::Md5,
                "sha1" => HashAlgorithm::Sha1,
                "sha256" => HashAlgorithm::Sha256,
                "sha384" => HashAlgorithm::Sha384,
                "sha512" => HashAlgorithm::Sha512,
                "sha3-256" => HashAlgorithm::Sha3_256,
                "blake2sp" => HashAlgorithm::Blake2sp,
                "*" => HashAlgorithm::All,
                _ => return Err(format!("неизвестный алгоритм: {value}")),
            };
        } else if arg.starts_with("--") {
            return Err(format!("неизвестный флаг: {arg}"));
        } else {
            paths.push(PathBuf::from(arg));
        }
    }

    if paths.is_empty() {
        return Err("укажи файлы или каталоги".to_string());
    }

    let locator = Locator::for_current_exe().map_err(|err| err.to_string())?;
    let tool = locator.resolve().map_err(|err| {
        format!(
            "{err}\nподсказка: положи `{}` рядом с программой",
            binary_name()
        )
    })?;

    let request = archiver_core::HashRequest::new(algorithm, paths);
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    let summary = archiver_core::hash_files(
        &tool.info.exe,
        &request,
        tool.info.version.supports_utf8_console(),
        &cancelled,
        |_| {},
    )
    .map_err(|err| err.to_string())?;

    print!("{}", summary.report.to_text());
    if !summary.warnings.trim().is_empty() {
        println!("предупреждения: {}", summary.warnings.trim());
    }
    Ok(ExitCode::SUCCESS)
}

fn usage() {
    println!(
        "archiver-cli — служебная обвязка ядра\n\
         \n\
         Использование:\n\
         \x20 archiver-cli tool-info [--exe-dir <папка>]\n\
         \x20 archiver-cli list <каталог|архив> [--sort=…] [--desc]\n\
         \x20 archiver-cli extract <архив> <куда> [--password=…] [--skip-existing] [--verbose]\n\
         \x20 archiver-cli create <куда.7z> <что…> [--level=5] [--no-solid] [--password=…]\n\
         \x20            [--encrypt-names] [--volume=100m] [--verbose]\n\
         \x20 archiver-cli test <архив> [--password=…] [--verbose]\n\
         \x20 archiver-cli hash <что…> [--algo=sha256|crc32|*|…]\n\
         \n\
         tool-info  находит 7-Zip и печатает версию, источник и предупреждения\n\
         list       печатает содержимое каталога или архива (через 7z l -slt)\n\
         extract    распаковывает архив и печатает прогресс (через 7z x -bsp1)\n\
         create     создаёт архив 7z (через 7z a -t7z)\n\
         test       проверяет целостность архива (через 7z t)\n\
         hash       считает контрольные суммы файлов (через 7z h)\n\
         \n\
         7-Zip берётся только из поставки — бинарник лежит рядом с программой.\n\
         `PATH` не опрашивается."
    );
}

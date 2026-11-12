//! Контрольные суммы и хеширование через `7z h`.
//!
//! Считает сам 7-Zip; отсюда уходит набор аргументов, обратно приходит отчёт:
//! таблица по файлам и строки «ALGO for data: …» по всем данным вместе.
//! Прогресс-индикатор (`-bsp1`) намеренно выключен (`-bd`): его `\r`-перерисовки
//! склеиваются со строками таблицы и ломают разбор.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::run::{Progress, RunError, failure_reason, run_7z_captured};

/// Алгоритм из подменю «CRC» 7-Zip FM. `All` — пункт «*»: все сразу.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgorithm {
    Crc32,
    Crc64,
    Xxh64,
    Md5,
    Sha1,
    Sha256,
    Sha384,
    Sha512,
    Sha3_256,
    Blake2sp,
    All,
}

impl HashAlgorithm {
    /// Имя алгоритма, как его понимает 7z (`-scrc…`) и подписывает в отчёте.
    pub fn name(self) -> &'static str {
        match self {
            Self::Crc32 => "CRC32",
            Self::Crc64 => "CRC64",
            Self::Xxh64 => "XXH64",
            Self::Md5 => "MD5",
            Self::Sha1 => "SHA1",
            Self::Sha256 => "SHA256",
            Self::Sha384 => "SHA384",
            Self::Sha512 => "SHA512",
            Self::Sha3_256 => "SHA3-256",
            Self::Blake2sp => "BLAKE2sp",
            Self::All => "*",
        }
    }

    pub fn flag(self) -> String {
        format!("-scrc{}", self.name())
    }
}

#[derive(Debug, Clone)]
pub struct HashRequest {
    /// Файлы или каталоги; каталог 7z обходит рекурсивно.
    pub paths: Vec<PathBuf>,
    pub algorithm: HashAlgorithm,
}

impl HashRequest {
    pub fn new(algorithm: HashAlgorithm, paths: Vec<PathBuf>) -> Self {
        Self { paths, algorithm }
    }
}

/// Суммы одного файла: колонки в том же порядке, что в заголовке таблицы 7z.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashedFile {
    pub name: String,
    pub size: u64,
    /// Пары (алгоритм, сумма).
    pub hashes: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HashReport {
    pub files: Vec<HashedFile>,
    /// Суммы по всем данным вместе («for data:»). Для одного файла совпадают
    /// с его собственными; для нескольких — CRC конкатенации и т. п.
    pub totals: Vec<(String, String)>,
}

impl HashReport {
    /// Текст для буфера обмена: имя файла и его суммы, затем итог по всем данным.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for file in &self.files {
            out.push_str(&file.name);
            out.push('\n');
            for (algo, hash) in &file.hashes {
                out.push_str(&format!("  {algo}: {hash}\n"));
            }
        }
        if self.files.len() > 1 && !self.totals.is_empty() {
            out.push_str("Все данные\n");
            for (algo, hash) in &self.totals {
                out.push_str(&format!("  {algo}: {hash}\n"));
            }
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct HashSummary {
    pub report: HashReport,
    pub warnings: String,
    pub cancelled: bool,
}

#[derive(Debug)]
pub enum HashError {
    Spawn(std::io::Error),
    Io(std::io::Error),
    Failed {
        code: i32,
        stderr: String,
        diagnostics: Vec<String>,
    },
}

impl std::fmt::Display for HashError {
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

impl std::error::Error for HashError {}

/// Аргументы `7z h`. Переключатели до `--`, пути после — иначе имя на дефис
/// 7z примет за ключ. `utf8_console` — есть ли у 7z `-sccUTF-8` (с 21.07):
/// без него имена в отчёте приходят в системной кодировке и ломаются.
pub fn build_args(request: &HashRequest, utf8_console: bool) -> Vec<String> {
    let mut args = vec![
        "h".to_string(),
        request.algorithm.flag(),
        // -bd: без прогресса — его \r-перерисовки ломают таблицу отчёта.
        "-bd".to_string(),
        "-bse1".to_string(),
    ];
    if utf8_console {
        args.push("-sccUTF-8".to_string());
    }
    args.push("--".to_string());
    args.extend(request.paths.iter().map(|path| path.display().to_string()));
    args
}

/// Считает суммы. Блокирующая: вызывающий запускает её в фоновом потоке.
pub fn hash_files(
    exe: &Path,
    request: &HashRequest,
    utf8_console: bool,
    cancelled: &AtomicBool,
    on_progress: impl FnMut(Progress),
) -> Result<HashSummary, HashError> {
    let args = build_args(request, utf8_console);
    let outcome = run_7z_captured(exe, &args, cancelled, on_progress).map_err(|err| match err {
        RunError::Spawn(err) => HashError::Spawn(err),
        RunError::Io(err) => HashError::Io(err),
    })?;

    if outcome.cancelled {
        return Ok(HashSummary {
            report: HashReport::default(),
            warnings: outcome.stderr,
            cancelled: true,
        });
    }

    match outcome.code {
        // 1 — часть файлов не прочиталась; отчёт о прочитанных всё равно есть.
        0 | 1 => Ok(HashSummary {
            report: parse_report(&outcome.stdout),
            warnings: if outcome.code == 1 {
                outcome.warning_text()
            } else {
                String::new()
            },
            cancelled: false,
        }),
        code => Err(HashError::Failed {
            code,
            stderr: outcome.stderr,
            diagnostics: outcome.diagnostics,
        }),
    }
}

/// Разбирает отчёт `7z h`: заголовок таблицы даёт порядок алгоритмов, строки
/// под ним — суммы файлов, строки «for data:» — итог по всем данным.
pub fn parse_report(stdout: &str) -> HashReport {
    let mut report = HashReport::default();
    let mut columns: Vec<String> = Vec::new();
    let mut in_table = false;

    for line in stdout.lines() {
        let trimmed = line.trim();
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();

        // Заголовок: имена алгоритмов, затем «Size  Name». Проверяем имена по
        // списку известных: файл с именем «Name» не должен стать заголовком.
        if tokens.len() > 2
            && tokens[tokens.len() - 2] == "Size"
            && tokens[tokens.len() - 1] == "Name"
            && tokens[..tokens.len() - 2].iter().all(|t| is_algorithm(t))
        {
            columns = tokens[..tokens.len() - 2]
                .iter()
                .map(|t| t.to_string())
                .collect();
            in_table = true;
            continue;
        }

        if in_table {
            if trimmed.is_empty() {
                in_table = false;
                continue;
            }
            if trimmed.starts_with('-') || tokens.len() < columns.len() + 1 {
                continue;
            }
            let Ok(size) = tokens[columns.len()].parse::<u64>() else {
                continue;
            };
            // Итоговая строка таблицы (сумма по всем данным) идёт без имени.
            let name = remainder_after(trimmed, columns.len() + 1);
            if name.is_empty() {
                continue;
            }
            report.files.push(HashedFile {
                name: name.to_string(),
                size,
                hashes: columns
                    .iter()
                    .zip(&tokens)
                    .map(|(algo, value)| (algo.clone(), value.to_string()))
                    .collect(),
            });
            continue;
        }

        // «CRC32  for data:  …» — итог. «for data and names:» не берём:
        // это сумма вместе с именами, в подменю её нет.
        if let Some((algo, value)) = trimmed.split_once("for data:") {
            report
                .totals
                .push((algo.trim().to_string(), value.trim().to_string()));
        }
    }

    report
}

fn is_algorithm(token: &str) -> bool {
    matches!(
        token,
        "CRC32"
            | "CRC64"
            | "XXH64"
            | "MD5"
            | "SHA1"
            | "SHA256"
            | "SHA384"
            | "SHA512"
            | "SHA3-256"
            | "BLAKE2sp"
    )
}

/// Остаток строки после `count` первых токенов — имя файла с пробелами внутри.
fn remainder_after(line: &str, count: usize) -> &str {
    let mut rest = line;
    for _ in 0..count {
        rest = rest.trim_start();
        match rest.find(char::is_whitespace) {
            Some(pos) => rest = &rest[pos..],
            None => return "",
        }
    }
    rest.trim_start()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_have_command_flag_and_paths_after_separator() {
        let request = HashRequest::new(
            HashAlgorithm::Sha256,
            vec![PathBuf::from("-dash.bin"), PathBuf::from("dir")],
        );
        let args = build_args(&request, true);

        assert_eq!(args[0], "h");
        assert!(args.contains(&"-scrcSHA256".to_string()));
        assert!(args.contains(&"-bd".to_string()));
        let separator = args.iter().position(|arg| arg == "--").expect("нужен --");
        assert_eq!(args[separator + 1], "-dash.bin");
        assert_eq!(args[separator + 2], "dir");
        for arg in &args[..separator] {
            assert!(arg.starts_with('-') || arg == "h");
        }
    }

    #[test]
    fn all_flag_is_star() {
        assert_eq!(HashAlgorithm::All.flag(), "-scrc*");
    }

    #[test]
    fn utf8_console_flag_follows_version_gate() {
        let request = HashRequest::new(HashAlgorithm::Crc32, vec![PathBuf::from("a.txt")]);
        let fresh = build_args(&request, true);
        assert!(fresh.contains(&"-sccUTF-8".to_string()));
        let old = build_args(&request, false);
        assert!(!old.contains(&"-sccUTF-8".to_string()));
        // Флаг — переключатель: строго до `--`.
        let separator = fresh.iter().position(|arg| arg == "--").unwrap();
        assert!(fresh[..separator].contains(&"-sccUTF-8".to_string()));
    }

    const SINGLE: &str = "\
7-Zip 26.06 (x64) : Copyright (c) 1999-2026 Igor Pavlov : 2026-11-06

Scanning
1 file, 10 bytes (1 KiB)

SHA256                                                                    Size  Name
---------------------------------------------------------------- -------------  ------------
fd639c4f698a01e97e017cf701b32785c779872121290dd61836b302b2eb618b            10  target\\hashtest.txt
---------------------------------------------------------------- -------------  ------------
fd639c4f698a01e97e017cf701b32785c779872121290dd61836b302b2eb618b            10

Size: 10

SHA256 for data:              fd639c4f698a01e97e017cf701b32785c779872121290dd61836b302b2eb618b

Everything is Ok
";

    #[test]
    fn parses_single_algorithm_report() {
        let report = parse_report(SINGLE);
        assert_eq!(report.files.len(), 1);
        let file = &report.files[0];
        assert_eq!(file.name, "target\\hashtest.txt");
        assert_eq!(file.size, 10);
        assert_eq!(
            file.hashes,
            vec![(
                "SHA256".to_string(),
                "fd639c4f698a01e97e017cf701b32785c779872121290dd61836b302b2eb618b".to_string()
            )]
        );
        assert_eq!(report.totals.len(), 1);
        assert_eq!(report.totals[0].0, "SHA256");
    }

    const MULTI: &str = "\
CRC32    CRC64            SHA256                                                           Size  Name
-------- ---------------- ---------------------------------------------------------------- -------------  ------------
0A13EA09 04AA1BC2658EBB6F fd639c4f698a01e97e017cf701b32785c779872121290dd61836b302b2eb618b            10  my file.txt
F007732D 0000000000000000 0000000000000000000000000000000000000000000000000000000000000000             3  dir\\a.txt
-------- ---------------- ---------------------------------------------------------------- -------------  ------------
3A812C43 0000000000000000 0000000000000000000000000000000000000000000000000000000000000000            13

Size: 13

CRC32  for data:              3A812C43-00000001
CRC32  for data and names:    41BECA72-00000002
CRC64  for data:              04AA1BC2658EBB6F

Everything is Ok
";

    #[test]
    fn parses_columns_names_with_spaces_and_totals() {
        let report = parse_report(MULTI);
        assert_eq!(report.files.len(), 2);
        assert_eq!(report.files[0].name, "my file.txt");
        assert_eq!(report.files[0].hashes.len(), 3);
        assert_eq!(
            report.files[0].hashes[0],
            ("CRC32".to_string(), "0A13EA09".to_string())
        );
        assert_eq!(report.files[0].hashes[1].0, "CRC64");
        assert_eq!(report.files[1].name, "dir\\a.txt");
        assert_eq!(report.files[1].size, 3);
        // «for data and names» не попадает в итог.
        assert_eq!(report.totals.len(), 2);
        assert_eq!(
            report.totals[0],
            ("CRC32".to_string(), "3A812C43-00000001".to_string())
        );
        assert_eq!(report.totals[1].0, "CRC64");
    }

    #[test]
    fn file_named_size_name_is_not_a_header() {
        // Строка файла «Size Name» заканчивается как заголовок, но имена
        // алгоритмов в ней не настоящие — это данные, а не новый заголовок.
        let report = parse_report(
            "CRC32    Size  Name\n-------- -------------  ------------\n0A13EA09            10  Size Name\n",
        );
        assert_eq!(report.files.len(), 1);
        assert_eq!(report.files[0].name, "Size Name");
    }

    #[test]
    fn empty_output_gives_empty_report() {
        let report = parse_report("");
        assert!(report.files.is_empty());
        assert!(report.totals.is_empty());
    }

    #[test]
    fn to_text_lists_files_then_totals() {
        let report = parse_report(MULTI);
        let text = report.to_text();
        assert!(text.starts_with("my file.txt\n  CRC32: 0A13EA09\n"));
        assert!(text.contains("Все данные\n  CRC32: 3A812C43-00000001\n"));
    }
}

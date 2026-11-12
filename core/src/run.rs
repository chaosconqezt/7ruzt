//! Общий запуск 7z с прогрессом и отменой — для распаковки и для создания архива.
//!
//! Чтение stdout крутится в отдельном потоке: `7z` пишет прогресс через `\r` и
//! блокируется на выводе. Если читать в том же потоке, который опрашивает флаг
//! отмены, отмена будет ждать следующей строки.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

/// События по ходу операции.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    Percent(u8),
    Item(String),
}

#[derive(Debug, Clone)]
pub struct RunOutcome {
    /// Код возврата 7z: 0 — успех, 1 — успех с предупреждениями.
    pub code: i32,
    pub stderr: String,
    /// Строки ошибок и предупреждений, найденные в stdout. 7-Zip раскидывает
    /// их по обоим потокам: при битом заголовке stderr пуст, а объяснение
    /// («Open ERROR: Cannot open the file as [7z] archive») уходит в stdout.
    pub diagnostics: Vec<String>,
    /// Полный stdout — заполняется только [`run_7z_captured`], иначе пуст.
    pub stdout: String,
    pub cancelled: bool,
}

impl RunOutcome {
    // При -bse1 предупреждения приходят в stdout, а не только в stderr.
    pub fn warning_text(&self) -> String {
        let mut lines: Vec<&str> = self
            .stderr
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        for line in self.diagnostics.iter().map(|line| line.trim()) {
            if !line.is_empty() && !lines.contains(&line) {
                lines.push(line);
            }
        }
        if lines.is_empty() {
            "7z завершился с предупреждениями (код 1), но не сообщил подробности.".to_string()
        } else {
            lines.join("\n")
        }
    }
}

#[derive(Debug)]
pub enum RunError {
    Spawn(std::io::Error),
    Io(std::io::Error),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(err) => write!(f, "не удалось запустить 7z: {err}"),
            Self::Io(err) => write!(f, "ошибка ввода-вывода: {err}"),
        }
    }
}

impl std::error::Error for RunError {}

/// Запускает `7z` с готовым набором аргументов.
pub fn run_7z(
    exe: &Path,
    args: &[String],
    cancelled: &AtomicBool,
    on_progress: impl FnMut(Progress),
) -> Result<RunOutcome, RunError> {
    run_7z_inner(exe, args, cancelled, on_progress, false)
}

/// Как [`run_7z`], но сохраняет полный stdout в [`RunOutcome::stdout`].
/// Нужно командам с отчётом (`7z h`); для распаковки/создания вывод слишком
/// велик, чтобы тащить его в память целиком.
pub fn run_7z_captured(
    exe: &Path,
    args: &[String],
    cancelled: &AtomicBool,
    on_progress: impl FnMut(Progress),
) -> Result<RunOutcome, RunError> {
    run_7z_inner(exe, args, cancelled, on_progress, true)
}

fn run_7z_inner(
    exe: &Path,
    args: &[String],
    cancelled: &AtomicBool,
    mut on_progress: impl FnMut(Progress),
    capture: bool,
) -> Result<RunOutcome, RunError> {
    let mut command = Command::new(exe);
    command.args(args);

    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command.spawn().map_err(RunError::Spawn)?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let (sender, receiver) = channel();
    let reader = std::thread::spawn(move || match stdout {
        Some(stdout) => read_progress(stdout, sender, capture),
        None => (Vec::new(), String::new()),
    });

    let error_reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut stderr) = stderr {
            // Ошибки короткие, поэтому читаем целиком и не заботимся о памяти.
            let _ = stderr.read_to_string(&mut text);
        }
        text
    });

    let mut was_cancelled = false;

    loop {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(progress) => on_progress(progress),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if cancelled.load(Ordering::Relaxed) {
            // 7z — один процесс без детей, поэтому kill достаточно.
            let _ = child.kill();
            was_cancelled = true;
            break;
        }
    }

    let status = child.wait().map_err(RunError::Io)?;
    let (diagnostics, stdout_text) = reader.join().unwrap_or_default();
    let stderr_text = error_reader.join().unwrap_or_default();

    Ok(RunOutcome {
        code: status.code().unwrap_or(-1),
        stderr: stderr_text,
        diagnostics,
        stdout: stdout_text,
        cancelled: was_cancelled,
    })
}

/// Сколько строк с ошибками имеет смысл помнить: в отчёте 7z их бывает
/// столько же, сколько битых файлов, а показать мы всё равно сможем одну.
const MAX_DIAGNOSTICS: usize = 20;

/// Читает stdout 7z и возвращает найденные там строки ошибок.
/// При `capture` заодно собирает полный текст stdout — для отчётов.
fn read_progress(
    mut stdout: impl Read,
    sender: std::sync::mpsc::Sender<Progress>,
    capture: bool,
) -> (Vec<String>, String) {
    let mut chunk = [0u8; 4096];
    let mut token: Vec<u8> = Vec::new();
    let mut last_percent: Option<u8> = None;
    let mut diagnostics: Vec<String> = Vec::new();
    let mut output = String::new();

    loop {
        let read = match stdout.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };

        for &byte in &chunk[..read] {
            // 7-Zip обновляет строку не через `\r`, а через `\b` (ClearPrev):
            // серия backspace, пробелы, снова backspace, затем новый текст.
            // Без `\b` весь прогресс копился бы до EOF и шкала стояла на нуле.
            if byte == b'\r' || byte == b'\n' || byte == b'\x08' {
                flush_token(&token, &sender, &mut last_percent, &mut diagnostics);
                if capture && !token.is_empty() {
                    output.push_str(&String::from_utf8_lossy(&token));
                    output.push('\n');
                }
                token.clear();
            } else {
                token.push(byte);
            }
        }
    }

    // Последний токен мог остаться без перевода строки.
    flush_token(&token, &sender, &mut last_percent, &mut diagnostics);
    if capture && !token.is_empty() {
        output.push_str(&String::from_utf8_lossy(&token));
        output.push('\n');
    }
    (diagnostics, output)
}

fn flush_token(
    token: &[u8],
    sender: &std::sync::mpsc::Sender<Progress>,
    last: &mut Option<u8>,
    diagnostics: &mut Vec<String>,
) {
    if token.is_empty() {
        return;
    }
    let text = String::from_utf8_lossy(token);
    let update = parse_progress(&text);

    // Прогресс и имена файлов — не ошибки. Проверяем диагностику только на
    // строках, где не нашлось ни процента, ни имени.
    if update.percent.is_none()
        && update.item.is_none()
        && let Some(line) = diagnostic_line(&text)
        && diagnostics.len() < MAX_DIAGNOSTICS
        && diagnostics.last().map(String::as_str) != Some(line)
    {
        diagnostics.push(line.to_string());
    }

    // Одинаковые проценты подряд не шлём: 7z повторяет их между файлами.
    if let Some(value) = update.percent
        && *last != Some(value)
    {
        *last = Some(value);
        let _ = sender.send(Progress::Percent(value));
    }
    if let Some(item) = update.item {
        let _ = sender.send(Progress::Item(item));
    }
}

/// Разобранная строка прогресса 7-Zip.
#[derive(Debug, Default, PartialEq, Eq)]
struct ProgressUpdate {
    percent: Option<u8>,
    item: Option<String>,
}

/// Разбирает токен 7-Zip: ` 36% 4 + src/file.dat` → процент.
///
/// Имя файла из этой же строки **не** отдаём: 7-Zip с `-bb1` печатает его
/// ещё и отдельной строкой `+ файл`, и если брать оба источника, счётчик
/// файлов удвоится. Имена разбирает [`item_name`].
fn parse_progress(token: &str) -> ProgressUpdate {
    let text = token.trim();
    if text.is_empty() {
        return ProgressUpdate::default();
    }

    // Строка списка файлов (`+ path`), а не прогресс: процент внутри имени
    // файла (`50%.txt`) прогрессом не считается.
    if let Some(item) = item_name(text.trim_start()) {
        return ProgressUpdate {
            percent: None,
            item: Some(item.to_string()),
        };
    }

    let bytes = text.as_bytes();
    let mut search_from = 0;
    while let Some(relative) = text[search_from..].find('%') {
        let percent_at = search_from + relative;
        let mut digits_at = percent_at;
        while digits_at > 0 && bytes[digits_at - 1].is_ascii_digit() {
            digits_at -= 1;
        }
        let digits = &text[digits_at..percent_at];
        let before_ok = digits_at == 0 || bytes[digits_at - 1].is_ascii_whitespace();
        if before_ok
            && !digits.is_empty()
            && digits.len() <= 3
            && let Ok(value) = digits.parse::<u32>()
        {
            return ProgressUpdate {
                percent: Some(value.min(100) as u8),
                item: None,
            };
        }
        search_from = percent_at + 1;
    }

    ProgressUpdate::default()
}

/// Строка вывода 7z, которая объясняет проблему. Имена файлов (`- name`, `T name`)
/// отбрасываем: файл с именем `error.log` — не сообщение об ошибке.
pub fn diagnostic_line(token: &str) -> Option<&str> {
    let line = token.trim();
    if line.is_empty() || item_name(line).is_some() {
        return None;
    }

    let lower = line.to_lowercase();
    const NEEDLES: [&str; 6] = [
        "error",
        "warning",
        "cannot open",
        "can't open",
        "wrong password",
        // 7z спрашивает пароль сам, если его не дали: stdin у нас пуст,
        // и это единственное объяснение кода 255 «Break signaled».
        "enter password",
    ];

    (NEEDLES.iter().any(|needle| lower.contains(needle)) && explains_something(line))
        .then_some(line)
}

/// Несёт ли строка объяснение, а не только имя объекта.
///
/// Перед падением 7-Zip печатает `ERROR: <путь>` — эхо архива, по которому
/// ничего не понять; настоящая причина идёт следующей строкой
/// (`Open ERROR: Cannot open the file as [7z] archive`).
fn explains_something(line: &str) -> bool {
    let rest = ["Open ERROR:", "ERROR:", "ERRORS:", "WARNING:"]
        .iter()
        .find_map(|prefix| line.strip_prefix(prefix))
        .unwrap_or(line)
        .trim();
    rest.split_whitespace().count() > 1
}

/// Имя файла из строки `+ path` (добавление), `- path` (извлечение) или
/// `T path` (проверка).
///
/// 7z сыплет в stdout служебными строками («Scanning the drive», «Extracting
/// archive»), настоящие имена идут с префиксом. Всё остальное в статус-бар
/// тащить незачем. `+` обязательно: при упаковке 7-Zip пишет именно так.
pub fn item_name(token: &str) -> Option<&str> {
    let rest = token.trim_start();
    let rest = rest
        .strip_prefix("+ ")
        .or_else(|| rest.strip_prefix("- "))
        .or_else(|| rest.strip_prefix("U "))
        // При проверке 7-Zip метит имена буквой `T`: `T a.txt`.
        .or_else(|| rest.strip_prefix("T "))?;
    let name = rest.trim();
    if name.is_empty() { None } else { Some(name) }
}

/// Первая непустая строка — для короткого сообщения об ошибке.
fn first_line(text: &str) -> &str {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
}

/// Почему 7z упал. stderr информативнее, но часть ошибок 7-Zip пишет только
/// в stdout — без этого падение выглядело бы как голый код возврата.
pub fn failure_reason(stderr: &str, diagnostics: &[String]) -> String {
    let line = first_line(stderr);
    if !line.is_empty() {
        return line.to_string();
    }
    diagnostics.first().cloned().unwrap_or_default()
}

/// Разбирает строку вывода: ` 34%` → [`Progress::Percent`], всё прочее — имя файла.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_lines_are_recognised() {
        assert_eq!(parse_progress(" 34%").percent, Some(34));
        assert_eq!(parse_progress("  0%").percent, Some(0));
        assert_eq!(parse_progress("100%").percent, Some(100));
        assert_eq!(parse_progress("150%").percent, Some(100));
        // 7-Zip печатает счётчик файлов после процента.
        assert_eq!(parse_progress(" 36% 4").percent, Some(36));
    }

    #[test]
    fn progress_line_does_not_duplicate_the_file_name() {
        // Имя файла уже придёт отдельной строкой `+ file`; брать его ещё и из
        // строки прогресса — значит удвоить счётчик «Файлов» в окне.
        assert_eq!(
            parse_progress(" 36% 4 + src/file_4.dat"),
            ProgressUpdate {
                percent: Some(36),
                item: None,
            }
        );
        assert_eq!(parse_progress(" 72% 7 - src/file_07.dat").percent, Some(72));
        assert_eq!(parse_progress(" 50% 5 T src/file_05.dat").percent, Some(50));
    }

    #[test]
    fn clear_prev_tokens_do_not_confuse_the_parser() {
        // ClearPrev: backspace, пробелы, backspace — всё это пустые токены.
        assert_eq!(parse_progress("        ").percent, None);
        assert_eq!(parse_progress(" 16% 2 + a.bin").percent, Some(16));
        // Процент внутри имени файла не считается прогрессом.
        assert_eq!(
            parse_progress("+ file 50%.txt"),
            ProgressUpdate {
                percent: None,
                item: Some("file 50%.txt".to_string()),
            }
        );
    }

    #[test]
    fn empty_lines_produce_nothing() {
        assert_eq!(parse_progress(""), ProgressUpdate::default());
        assert_eq!(parse_progress("   "), ProgressUpdate::default());
        assert_eq!(parse_progress("abc%"), ProgressUpdate::default());
    }

    #[test]
    fn prefixed_lines_are_file_names() {
        // Упаковка: `+`; извлечение: `-`; проверка: `T`; обновление: `U`.
        assert_eq!(item_name("+ docs/readme.txt"), Some("docs/readme.txt"));
        assert_eq!(item_name("- docs/readme.txt"), Some("docs/readme.txt"));
        assert_eq!(item_name("  - a.bin"), Some("a.bin"));
        assert_eq!(item_name("U a.bin"), Some("a.bin"));
        assert_eq!(item_name("T a.bin"), Some("a.bin"));
        assert_eq!(item_name("T T space.txt"), Some("T space.txt"));
        // Служебные строки 7z отбрасываем.
        assert_eq!(item_name("Scanning the drive for archives:"), None);
        assert_eq!(item_name("Extracting archive: sample.7z"), None);
        assert_eq!(item_name("0M Scan C:\\work\\"), None);
        assert_eq!(item_name("- "), None);
        assert_eq!(item_name("+ "), None);
        assert_eq!(item_name("T "), None);
    }

    #[test]
    fn error_lines_are_picked_out_of_stdout() {
        // При битом заголовке stderr пуст, и без этого сообщение было бы пустым.
        assert_eq!(
            diagnostic_line("Open ERROR: Cannot open the file as [7z] archive"),
            Some("Open ERROR: Cannot open the file as [7z] archive")
        );
        assert_eq!(
            diagnostic_line("ERROR: Data Error in encrypted file. Wrong password? : a.txt"),
            Some("ERROR: Data Error in encrypted file. Wrong password? : a.txt")
        );
        assert_eq!(
            diagnostic_line("WARNING: Some files were skipped"),
            Some("WARNING: Some files were skipped")
        );
        assert_eq!(
            diagnostic_line("Enter password (will not be echoed):"),
            Some("Enter password (will not be echoed):")
        );
    }

    #[test]
    fn bare_object_echo_is_not_an_explanation() {
        // 7z печатает путь архива после `ERROR:` — это не причина падения.
        assert_eq!(diagnostic_line("ERROR: C:\\work\\bad.7z"), None);
        assert_eq!(diagnostic_line("ERRORS:"), None);
    }

    #[test]
    fn file_names_and_progress_are_not_errors() {
        // Файл с «error» в имени — не сообщение об ошибке.
        assert_eq!(diagnostic_line("T error.log"), None);
        assert_eq!(diagnostic_line("- docs/error.txt"), None);
        assert_eq!(diagnostic_line(" 34%"), None);
        assert_eq!(diagnostic_line("Everything is Ok"), None);
        assert_eq!(diagnostic_line(""), None);
    }

    #[test]
    fn clear_prev_backspaces_flush_percent_and_file_while_running() {
        // Так выглядит сырой поток 7z: ClearPrev (`\b`, пробелы, `\b`) перед
        // каждой порцией. Без разбора `\x08` всё это пришло бы одним куском в EOF.
        let raw = b"\x08\x08  0%\x08\x08        \x08\x08 16% 2 + a.bin\n+ a.bin\n\x08\x08        \x08\x08 32% 5 + b.bin\n+ b.bin\n";
        let (sender, receiver) = channel();
        let (diagnostics, _) = read_progress(&raw[..], sender, false);
        assert!(diagnostics.is_empty());
        assert_eq!(
            receiver.try_iter().collect::<Vec<_>>(),
            vec![
                Progress::Percent(0),
                Progress::Percent(16),
                Progress::Item("a.bin".to_string()),
                Progress::Percent(32),
                Progress::Item("b.bin".to_string()),
            ]
        );
    }

    #[test]
    fn warning_text_preserves_all_stderr_lines() {
        let outcome = RunOutcome {
            code: 1,
            stderr: "WARNING: Cannot open file\nAccess is denied.\nlocked.txt\n".to_string(),
            diagnostics: Vec::new(),
            stdout: String::new(),
            cancelled: false,
        };
        assert_eq!(
            outcome.warning_text(),
            "WARNING: Cannot open file\nAccess is denied.\nlocked.txt"
        );
    }

    #[test]
    fn warning_text_preserves_stdout_diagnostics_without_progress_or_file_names() {
        let stdout =
            " 34%\rT warning.log\n- error.txt\nWARNING: Some files were skipped\nWarnings: 1\n";
        for capture in [false, true] {
            let (sender, _receiver) = channel();
            let (diagnostics, stdout) = read_progress(stdout.as_bytes(), sender, capture);
            let outcome = RunOutcome {
                code: 1,
                stderr: String::new(),
                diagnostics,
                stdout,
                cancelled: false,
            };
            assert_eq!(
                outcome.warning_text(),
                "WARNING: Some files were skipped\nWarnings: 1"
            );
        }
    }

    #[test]
    fn warning_text_combines_both_streams_without_repeating_diagnostics() {
        let outcome = RunOutcome {
            code: 1,
            stderr: "WARNING: Cannot open file\nAccess is denied.\n".to_string(),
            diagnostics: vec![
                " WARNING: Cannot open file ".to_string(),
                "".to_string(),
                "WARNING: Some files were skipped".to_string(),
                "WARNING: Some files were skipped".to_string(),
            ],
            stdout: String::new(),
            cancelled: false,
        };
        assert_eq!(
            outcome.warning_text(),
            "WARNING: Cannot open file\nAccess is denied.\nWARNING: Some files were skipped"
        );
    }

    #[test]
    fn warning_text_is_nonempty_even_without_an_explanation() {
        for (stderr, diagnostics) in [("", vec![]), (" \n\t", vec!["  ".to_string()])] {
            let outcome = RunOutcome {
                code: 1,
                stderr: stderr.to_string(),
                diagnostics,
                stdout: "Scanning the drive\n 100%\n".to_string(),
                cancelled: false,
            };
            assert_eq!(
                outcome.warning_text(),
                "7z завершился с предупреждениями (код 1), но не сообщил подробности."
            );
        }
    }

    #[test]
    fn failure_reason_prefers_stderr_then_stdout() {
        assert_eq!(
            failure_reason("ERROR: Wrong password?\nmore", &["ignored".to_string()]),
            "ERROR: Wrong password?"
        );
        assert_eq!(
            failure_reason("", &["ERRORS: Headers Error".to_string()]),
            "ERRORS: Headers Error"
        );
        assert_eq!(failure_reason("  \n", &[]), "");
    }
}

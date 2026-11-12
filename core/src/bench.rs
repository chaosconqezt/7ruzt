//! Бенчмарк производительности через `7z b`.
//!
//! В отличие от распаковки/проверки, здесь нет процентов и имён файлов: 7z
//! печатает в stdout шапку (CPU, частоты, RAM) и таблицу словарей, строка за
//! строкой по мере прохождения. Мы не разбираем её в модель, а отдаём вызывающему
//! сырые строки — окно показывает ровно то, что печатает консоль.
//!
//! Чтение stdout — в отдельном потоке (как в [`crate::run`]): 7z блокируется на
//! выводе, а поток, опрашивающий флаг отмены, должен убить процесс сразу, не
//! дожидаясь следующей строки.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::time::Duration;

use crate::run::RunError;

/// Итог прогона `7z b`.
#[derive(Debug, Clone)]
pub struct BenchOutcome {
    /// Код возврата 7z: 0 — успех.
    pub code: i32,
    pub stderr: String,
    pub cancelled: bool,
}

/// Одна измеренная величина: скорость, загрузка и два рейтинга.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BenchValue {
    /// Скорость в КиБ/с (в строке `Tot` её нет — там 0).
    pub speed: f64,
    /// Нагрузка в процентах (уже с учётом потоков).
    pub usage: f64,
    /// Рейтинг на единицу нагрузки, MIPS.
    pub per_usage: f64,
    /// Итоговый рейтинг, MIPS.
    pub rating: f64,
}

/// Пара «сжатие | распаковка» из строк `Avr` и `Tot`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BenchPair {
    pub compressing: BenchValue,
    pub decompressing: BenchValue,
}

/// Строка таблицы: один размер словаря (степень двойки) и обе величины.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BenchRow {
    /// `Dict` из вывода: степень двойки, т.е. 2^n байт.
    pub dict_log2: u32,
    pub compressing: BenchValue,
    pub decompressing: BenchValue,
}

/// Частоты CPU для заданного числа потоков: `1T CPU Freq (MHz): …`.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchFreq {
    pub threads: u32,
    /// Значения как их напечатал 7z: для 1T — просто МГц, для NT — пары
    /// «загрузка% частота» по проходам.
    pub values: String,
}

/// Разобранный текстовый вывод `7z b` (одного прогона).
///
/// Всё, что 7z печатает, но чего нет в окне 7-Zip FM (например, размер
/// обработанных данных), здесь отсутствует осознанно: цифру не выдумываем.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BenchReport {
    pub version: Option<String>,
    pub os: Option<String>,
    pub cpu: Option<String>,
    pub cpu_details: Option<String>,
    /// Строка параметров, например `mt=32 d=16m`.
    pub params: Option<String>,
    pub ram_size_mb: Option<u64>,
    pub ram_usage_mb: Option<u64>,
    pub hardware_threads: Option<u32>,
    pub bench_threads: Option<u32>,
    pub freqs: Vec<BenchFreq>,
    pub rows: Vec<BenchRow>,
    pub average: Option<BenchPair>,
    pub total: Option<BenchPair>,
}

impl BenchReport {
    /// Максимальный словарь прогона в мебибайтах (последняя строка таблицы).
    pub fn dict_mb(&self) -> Option<u64> {
        let log2 = self.rows.iter().map(|row| row.dict_log2).max()?;
        (log2 < 63).then(|| (1u64 << log2) / (1024 * 1024))
    }

    /// Итог сжатия: скорость берём из `Avr`, нагрузки и рейтинги — из `Tot`.
    pub fn total_compressing(&self) -> Option<BenchValue> {
        let average = self.average?.compressing;
        match self.total {
            Some(total) if total.compressing.rating > 0.0 => Some(BenchValue {
                speed: average.speed,
                ..total.compressing
            }),
            _ => Some(average),
        }
    }

    /// Итог распаковки. В `Tot` распаковки нет — тогда итогом считаем `Avr`.
    pub fn total_decompressing(&self) -> Option<BenchValue> {
        let average = self.average?.decompressing;
        match self.total {
            Some(total) if total.decompressing.rating > 0.0 => Some(BenchValue {
                speed: average.speed,
                ..total.decompressing
            }),
            _ => Some(average),
        }
    }

    /// Последняя измеренная строка — она же «Текущий» после завершения.
    pub fn current(&self) -> Option<BenchRow> {
        self.rows.last().copied()
    }
}

/// Разбирает строки stdout `7z b`. Неизвестные строки пропускаются: вывод 7z
/// меняется от версии к версии, и падать из-за новой строки нельзя.
pub fn parse_output<I, S>(lines: I) -> BenchReport
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    #[derive(Clone, Copy, PartialEq)]
    enum Header {
        None,
        Cpu,
        Details,
    }

    let mut report = BenchReport::default();
    let mut header = Header::None;

    for line in lines {
        let line = line.as_ref();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("7-Zip ") {
            let name = rest.split(" : Copyright").next().unwrap_or(rest).trim();
            report.version = Some(format!("7-Zip {name}"));
            continue;
        }
        if trimmed.starts_with("Windows ") {
            report.os = Some(trimmed.to_string());
            continue;
        }
        if trimmed.starts_with("Compiler:") {
            continue;
        }
        if trimmed.starts_with("x64 ") || trimmed.starts_with("32-bit ") {
            report.hardware_threads = after_marker(trimmed, "threads:");
            header = Header::Cpu;
            continue;
        }
        match header {
            Header::Cpu if !trimmed.starts_with('(') => {
                report.cpu = Some(trimmed.to_string());
                header = Header::Details;
                continue;
            }
            Header::Cpu | Header::Details if trimmed.starts_with('(') => {
                report.cpu_details = Some(trimmed.to_string());
                header = Header::None;
                continue;
            }
            _ => {}
        }
        if trimmed.starts_with("mt=") {
            report.params = Some(trimmed.to_string());
            continue;
        }
        if let Some(index) = trimmed.find(" CPU Freq (MHz):") {
            let threads = trimmed[..index].trim_end_matches('T').parse().unwrap_or(0);
            report.freqs.push(BenchFreq {
                threads,
                values: trimmed[index + " CPU Freq (MHz):".len()..]
                    .trim()
                    .to_string(),
            });
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("RAM size:") {
            report.ram_size_mb = first_number(rest);
            report.hardware_threads =
                after_marker(rest, "hardware threads:").or(report.hardware_threads);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("RAM usage:") {
            report.ram_usage_mb = first_number(rest);
            report.bench_threads = after_marker(rest, "Benchmark threads:");
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Avr:") {
            report.average = parse_pair(rest);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Tot:") {
            report.total = Some(parse_total(rest));
            continue;
        }
        if let Some((head, rest)) = trimmed.split_once(':')
            && !head.is_empty()
            && head.bytes().all(|byte| byte.is_ascii_digit())
            && let Some((compressing, decompressing)) = rest.split_once('|')
            && let Some(row) = parse_row(head, compressing, decompressing)
        {
            report.rows.push(row);
        }
    }

    report
}

/// Четыре числа после двоеточия — [скорость, нагрузка, рейтинг/нагр., рейтинг].
fn parse_values(part: &str) -> Vec<f64> {
    part.split_whitespace()
        .filter_map(|token| token.parse().ok())
        .collect()
}

fn value_from(values: &[f64]) -> Option<BenchValue> {
    (values.len() >= 4).then(|| BenchValue {
        speed: values[0],
        usage: values[1],
        per_usage: values[2],
        rating: values[3],
    })
}

fn parse_pair(rest: &str) -> Option<BenchPair> {
    let (compressing, decompressing) = rest.split_once('|')?;
    Some(BenchPair {
        compressing: value_from(&parse_values(compressing))?,
        decompressing: value_from(&parse_values(decompressing))?,
    })
}

fn parse_row(dict: &str, compressing: &str, decompressing: &str) -> Option<BenchRow> {
    Some(BenchRow {
        dict_log2: dict.parse().ok()?,
        compressing: value_from(&parse_values(compressing))?,
        decompressing: value_from(&parse_values(decompressing))?,
    })
}

/// В `Tot:` 7z печатает только три числа на сторону (без скорости), а
/// распаковку в этой версии не печатает вовсе — отсутствие остаётся нулями.
fn parse_total(rest: &str) -> BenchPair {
    let (compressing, decompressing) = match rest.split_once('|') {
        Some((compressing, decompressing)) => (compressing, Some(decompressing)),
        None => (rest, None),
    };
    let value = |part: &str| {
        let values = parse_values(part);
        if values.len() >= 3 {
            BenchValue {
                speed: 0.0,
                usage: values[0],
                per_usage: values[1],
                rating: values[2],
            }
        } else {
            BenchValue::default()
        }
    };
    BenchPair {
        compressing: value(compressing),
        decompressing: decompressing.map(value).unwrap_or_default(),
    }
}

fn first_number(rest: &str) -> Option<u64> {
    rest.split_whitespace().next()?.parse().ok()
}

fn after_marker(rest: &str, marker: &str) -> Option<u32> {
    rest.split(marker)
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Аргументы `7z b`. Пока без ключей: словарь и потоки 7z выбирает сам, как в
/// консольном тесте. `-bd` не ставим — он глушит индикатор, а нам нужен вывод.
pub fn build_args() -> Vec<String> {
    vec!["b".to_string()]
}

/// Запускает `7z b` и отдаёт сырые строки stdout по мере появления.
///
/// Блокирующая: вызывающий запускает её в фоновом потоке. Отмена — общий
/// `AtomicBool`: процесс убивается на ближайшей проверке (раз в 100 мс).
pub fn run_benchmark(
    exe: &Path,
    cancelled: &AtomicBool,
    mut on_line: impl FnMut(String),
) -> Result<BenchOutcome, RunError> {
    let args = build_args();
    let mut command = Command::new(exe);
    command
        .args(&args)
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
    let reader = std::thread::spawn(move || {
        if let Some(stdout) = stdout {
            read_lines(stdout, sender);
        }
    });
    let error_reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut stderr) = stderr {
            let _ = stderr.read_to_string(&mut text);
        }
        text
    });

    let mut was_cancelled = false;
    loop {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => on_line(line),
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
    let _ = reader.join();
    let stderr_text = error_reader.join().unwrap_or_default();

    Ok(BenchOutcome {
        code: status.code().unwrap_or(-1),
        stderr: stderr_text,
        cancelled: was_cancelled,
    })
}

/// Читает stdout и шлёт строки в канал. Пустые строки сохраняем: в выводе 7z
/// ими разделены шапка, таблица и итоги.
fn read_lines(mut stdout: impl Read, sender: Sender<String>) {
    let mut chunk = [0u8; 4096];
    let mut splitter = LineSplitter::new();
    loop {
        let read = match stdout.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        for &byte in &chunk[..read] {
            if let Some(line) = splitter.push(byte)
                && sender.send(line).is_err()
            {
                return;
            }
        }
    }
    if let Some(line) = splitter.finish() {
        let _ = sender.send(line);
    }
}

/// Режет поток байтов на строки. `\r`, `\n` и `\r\n` — один перевод строки;
/// `\r` в одиночку (индикатор прогресса) тоже граница. Пустые строки отдаются.
///
/// Вынесено из [`read_lines`] ради тестов: разбор границ не требует запуска 7z.
#[derive(Default)]
pub struct LineSplitter {
    token: Vec<u8>,
    /// Прошлый байт был `\r`: следующий `\n` (пара `\r\n`) не даёт второй строки.
    after_cr: bool,
}

impl LineSplitter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Скормить байт; вернуть строку, если он замкнул перевод строки.
    pub fn push(&mut self, byte: u8) -> Option<String> {
        match byte {
            b'\n' => {
                let after_cr = std::mem::take(&mut self.after_cr);
                // `\r\n`: строку уже отдал `\r`, а `\n` только доводит пару.
                if after_cr { None } else { Some(self.take()) }
            }
            b'\r' => {
                self.after_cr = true;
                Some(self.take())
            }
            _ => {
                self.after_cr = false;
                self.token.push(byte);
                None
            }
        }
    }

    /// Хвост без перевода строки в конце потока.
    pub fn finish(&mut self) -> Option<String> {
        if self.token.is_empty() {
            None
        } else {
            Some(self.take())
        }
    }

    fn take(&mut self) -> String {
        let line = String::from_utf8_lossy(&self.token).into_owned();
        self.token.clear();
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_are_plain_benchmark() {
        assert_eq!(build_args(), vec!["b".to_string()]);
    }

    /// Прогоняет строку через сплиттер по байтам, собирает выданные строки.
    fn split(input: &str) -> Vec<String> {
        let mut splitter = LineSplitter::new();
        let mut lines = Vec::new();
        for &byte in input.as_bytes() {
            if let Some(line) = splitter.push(byte) {
                lines.push(line);
            }
        }
        if let Some(line) = splitter.finish() {
            lines.push(line);
        }
        lines
    }

    #[test]
    fn splits_on_lf() {
        assert_eq!(split("a\nb\nc"), vec!["a", "b", "c"]);
    }

    #[test]
    fn crlf_is_one_break() {
        assert_eq!(split("a\r\nb\r\n"), vec!["a", "b"]);
    }

    #[test]
    fn lone_cr_is_a_break() {
        // Индикатор прогресса 7z перезаписывает строку через `\r`.
        assert_eq!(split("a\rb\rc"), vec!["a", "b", "c"]);
    }

    #[test]
    fn blank_lines_are_preserved() {
        assert_eq!(split("a\n\nb\n"), vec!["a", "", "b"]);
        assert_eq!(split("a\r\n\r\nb"), vec!["a", "", "b"]);
    }

    #[test]
    fn trailing_token_without_newline_is_flushed() {
        assert_eq!(split("Tot: 123"), vec!["Tot: 123"]);
    }

    #[test]
    fn empty_input_yields_nothing() {
        assert!(split("").is_empty());
    }

    #[test]
    fn multibyte_text_survives() {
        assert_eq!(split("Потоки: 32\n"), vec!["Потоки: 32"]);
    }

    /// Реальный вывод `7z b -mmt=32 -md=16m` из vendor-сборки.
    const OUTPUT_32T: &str = "7-Zip 26.06 (x64) : Copyright (c) 1999-2026 Igor Pavlov : 2026-11-06\n\
        \n\
         mt=32 d=16m\n\
        Compiler:  MSC 1400.140040310\n\
        Windows 10.0 26100 : Microsoft Hv : Hv#1 : 10.0.26100.3.0.3775\n\
        x64 6.B701 threads:32 128TB f:100005F310C2774C\n\
        Intel(R) Core(TM) i9-14900KF\n\
        (B0671 : Intel64 Family 6 Model 183 Stepping 1) (->) 3187 MHz\n\
        \n\
        1T CPU Freq (MHz):  5712  5152  5411  5460  5463  5432  5343\n\
        16T CPU Freq (MHz): 1344% 4114   1484% 4599  \n\
        32T CPU Freq (MHz): 2752% 3972   2804% 4087  \n\
        \n\
        RAM size:   65371 MB,  # CPU hardware threads:  32\n\
        RAM usage:   3647 MB,  # Benchmark threads:     32\n\
        \n\
                               Compressing  |                  Decompressing\n\
        Dict     Speed Usage    R/U Rating  |      Speed Usage    R/U Rating\n\
                 KiB/s     %   MIPS   MIPS  |      KiB/s     %   MIPS   MIPS\n\
        \n\
        22:     128056  2841   4385 124573  |    2593557  2919   7575 221125\n\
        23:     112711  2792   4114 114840  |    2508470  2954   7348 217017\n\
        24:      98007  2802   3761 105378  |    2390047  2959   7088 209719\n\
        ----------------------------------  | ------------------------------\n\
        Avr:    112925  2811   4087 114930  |    2497358  2944   7337 215954\n\
        Tot:            2878   5712 165442\n";

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(ToString::to_string).collect()
    }

    #[test]
    fn parses_header_and_cpu_block() {
        let report = parse_output(lines(OUTPUT_32T));
        assert_eq!(report.version.as_deref(), Some("7-Zip 26.06 (x64)"));
        assert_eq!(report.cpu.as_deref(), Some("Intel(R) Core(TM) i9-14900KF"));
        assert!(
            report
                .cpu_details
                .as_deref()
                .is_some_and(|d| d.starts_with("(B0671"))
        );
        assert!(
            report
                .os
                .as_deref()
                .is_some_and(|os| os.starts_with("Windows 10.0 26100"))
        );
        assert_eq!(report.params.as_deref(), Some("mt=32 d=16m"));
        assert_eq!(report.ram_size_mb, Some(65371));
        assert_eq!(report.ram_usage_mb, Some(3647));
        assert_eq!(report.hardware_threads, Some(32));
        assert_eq!(report.bench_threads, Some(32));
    }

    #[test]
    fn parses_frequencies_per_thread_count() {
        let report = parse_output(lines(OUTPUT_32T));
        let threads: Vec<u32> = report.freqs.iter().map(|freq| freq.threads).collect();
        assert_eq!(threads, [1, 16, 32]);
        assert_eq!(
            report.freqs[0].values,
            "5712  5152  5411  5460  5463  5432  5343"
        );
        assert_eq!(report.freqs[1].values, "1344% 4114   1484% 4599");
    }

    #[test]
    fn parses_dictionary_rows_and_averages() {
        let report = parse_output(lines(OUTPUT_32T));
        assert_eq!(report.rows.len(), 3);
        let first = report.rows[0];
        assert_eq!(first.dict_log2, 22);
        assert_eq!(first.compressing.speed, 128056.0);
        assert_eq!(first.compressing.usage, 2841.0);
        assert_eq!(first.compressing.per_usage, 4385.0);
        assert_eq!(first.compressing.rating, 124573.0);
        assert_eq!(first.decompressing.speed, 2593557.0);
        assert_eq!(first.decompressing.rating, 221125.0);

        let average = report.average.unwrap();
        assert_eq!(average.compressing.speed, 112925.0);
        assert_eq!(average.compressing.rating, 114930.0);
        assert_eq!(average.decompressing.rating, 215954.0);
        assert_eq!(report.dict_mb(), Some(16));
    }

    #[test]
    fn totals_take_rating_from_tot_and_speed_from_average() {
        let report = parse_output(lines(OUTPUT_32T));
        let total = report.total_compressing().unwrap();
        assert_eq!(total.speed, 112925.0, "скорость — из Avr");
        assert_eq!(total.usage, 2878.0);
        assert_eq!(total.per_usage, 5712.0);
        assert_eq!(total.rating, 165442.0);

        // В этой версии 7z не печатает итог распаковки — остаётся Avr.
        let unpacking = report.total_decompressing().unwrap();
        assert_eq!(unpacking.speed, 2497358.0);
        assert_eq!(unpacking.rating, 215954.0);

        let current = report.current().unwrap();
        assert_eq!(current.dict_log2, 24);
        assert_eq!(current.compressing.rating, 105378.0);
    }

    #[test]
    fn parses_crlf_lines_and_ignores_unknown_output() {
        // Режем по `\n`, оставляя `\r` в строках: разбор обязан их снять.
        let text = OUTPUT_32T.replace('\n', "\r\n");
        let report = parse_output(text.split('\n').map(ToString::to_string));
        assert_eq!(report.rows.len(), 3);
        assert_eq!(report.bench_threads, Some(32));
        assert_eq!(report.version.as_deref(), Some("7-Zip 26.06 (x64)"));

        let empty = parse_output(Vec::<String>::new());
        assert!(empty.rows.is_empty());
        assert!(empty.average.is_none());
        assert_eq!(empty.dict_mb(), None);
    }
}

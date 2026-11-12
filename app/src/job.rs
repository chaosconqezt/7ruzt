use crate::prelude::*;
/// Чем закончилась фоновая операция.
pub(crate) struct JobOutcome {
    pub(crate) cancelled: bool,
    /// Предупреждения 7z: код 1 означает «сделано, но не всё».
    pub(crate) warnings: String,
}

/// Сообщения из фонового потока.
pub(crate) enum JobMessage {
    Progress(Progress),
    /// Объём работы, посчитанный до запуска 7z: байты и число файлов.
    /// Для создания архива — обход источников, для распаковки/проверки —
    /// суммарный размер записей архива.
    Totals {
        bytes: u64,
        files: u64,
    },
    /// Отчёт `7z h` — приходит перед `Done`, диалог откроется в `finish_job`.
    Hashes(HashReport),
    /// Итоги `7z t` — приходят перед `Done`, окно результата откроется там же.
    TestStats(TestStats),
    Done(Result<JobOutcome, String>),
}

/// Какая операция идёт в фоне.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobKind {
    Extract,
    Create,
    Test,
    Copy,
    Move,
    /// Просмотр файла из архива: извлечение во временную папку, затем открытие.
    Open,
    /// Контрольные суммы (`7z h`): по готовности открывается диалог отчёта.
    Hash,
}

impl JobKind {
    /// Кнопка тулбара, которая на время этой операции становится «Отменой».
    pub(crate) fn tool_action(self) -> ToolAction {
        match self {
            Self::Extract => ToolAction::Extract,
            Self::Create => ToolAction::Add,
            Self::Test => ToolAction::Test,
            Self::Copy => ToolAction::Copy,
            Self::Move => ToolAction::Move,
            // Отдельной кнопки у просмотра нет: отменять его можно кнопкой
            // «Извлечь» — по сути это и есть извлечение.
            Self::Open => ToolAction::Extract,
            // Кнопки у хеширования нет: отмена — в окне прогресса.
            Self::Hash => ToolAction::None,
        }
    }

    /// Подпись операции в окне прогресса — как в 7-Zip.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Extract => "Распаковка",
            Self::Create => "Упаковка",
            Self::Test => "Тестирование",
            Self::Copy => "Копирование",
            Self::Move => "Перемещение",
            Self::Open => "Распаковка",
            Self::Hash => "Хеширование",
        }
    }

    /// Строка операции над текущим файлом («Добавление», «Извлечение»…).
    pub(crate) fn action(self) -> &'static str {
        match self {
            Self::Extract => "Извлечение",
            Self::Create => "Добавление",
            Self::Test => "Проверка",
            Self::Copy => "Копирование",
            Self::Move => "Перемещение",
            Self::Open => "Извлечение",
            Self::Hash => "Хеширование",
        }
    }
}

pub(crate) struct Job {
    pub(crate) kind: JobKind,
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) percent: u8,
    pub(crate) item: String,
    pub(crate) target: PathBuf,
    /// Когда операция началась: от этого считаются «Прошло» и «Осталось».
    pub(crate) started: Instant,
    /// Объём работы из [`JobMessage::Totals`]; пока не пришёл — нули (неизвестно).
    pub(crate) total_bytes: u64,
    pub(crate) total_files: u64,
    /// Сколько файлов 7z уже назвал (`-bb1`) — счётчик для «Файлов».
    pub(crate) seen_files: u64,
    /// Окно прогресса убрано кнопкой «Фон»: работа идёт, виден статус-бар.
    pub(crate) hidden: bool,
    /// Только для `JobKind::Open`: что открыть оболочкой после извлечения.
    pub(crate) open_after: Vec<PathBuf>,
    /// Только для `JobKind::Hash`: готовый отчёт, диалог откроется по Done.
    pub(crate) hashes: Option<HashReport>,
    /// Только для `JobKind::Test`: счётчики итогов, окно откроется по Done.
    pub(crate) test_stats: Option<TestStats>,
    /// Время, застывшее на завершении: у оставленного окна прогресса
    /// «Прошло» не должно тикать, пока человек читает итоги.
    pub(crate) done_elapsed: Option<Duration>,
    pub(crate) retry_dialog: Option<Modal>,
}

impl Job {
    pub(crate) fn verb(&self) -> &'static str {
        match self.kind {
            JobKind::Extract => "Извлечение",
            JobKind::Create => "Создание",
            JobKind::Test => "Проверка",
            JobKind::Copy => "Копирование",
            JobKind::Move => "Перемещение",
            JobKind::Open => "Открытие",
            JobKind::Hash => "Хеширование",
        }
    }

    pub(crate) fn text(&self) -> String {
        let mut text = format!(
            "{} {}% → {}",
            self.verb(),
            self.percent,
            self.target.display()
        );
        if !self.item.is_empty() {
            text.push_str(" · ");
            text.push_str(&self.item);
        }
        text
    }

    /// Оценка обработанных байт: 7z даёт только процент, объём известен из
    /// [`JobMessage::Totals`]. Ноль объёма — оценки нет.
    pub(crate) fn processed_bytes(&self) -> Option<u64> {
        (self.total_bytes > 0).then(|| self.total_bytes * self.percent as u64 / 100)
    }

    /// «Осталось» по темпу: время_прошло × (100 − p) / p. Это оценка, а не
    /// прогноз 7z (тот в CLI времени не сообщает).
    pub(crate) fn remaining(&self) -> Option<Duration> {
        if self.percent == 0 {
            return None;
        }
        let elapsed = self.started.elapsed().as_secs_f64();
        let remaining = elapsed * (100.0 - self.percent as f64) / self.percent as f64;
        Some(Duration::from_secs_f64(remaining.clamp(0.0, 359_999.0)))
    }

    /// Скорость в байтах в секунду: обработанное / прошедшее время.
    pub(crate) fn speed(&self) -> Option<u64> {
        let processed = self.processed_bytes()? as f64;
        let seconds = self
            .done_elapsed
            .unwrap_or_else(|| self.started.elapsed())
            .as_secs_f64();
        (seconds > 0.0 && processed > 0.0).then(|| (processed / seconds) as u64)
    }

    /// Размер файла архива на диске — только для упаковки и только пока он есть.
    pub(crate) fn archive_size(&self) -> Option<u64> {
        (self.kind == JobKind::Create)
            .then(|| std::fs::metadata(&self.target).ok().map(|meta| meta.len()))
            .flatten()
    }

    /// Степень сжатия: размер архива к обработанным байтам.
    pub(crate) fn ratio(&self) -> Option<u8> {
        let processed = self.processed_bytes()?;
        let archive = self.archive_size()?;
        (processed > 0).then(|| ((archive.min(processed) * 100) / processed) as u8)
    }
}

pub(crate) fn with_warnings(base: &str, outcome: &JobOutcome) -> String {
    let text = outcome.warnings.trim();
    if text.is_empty() {
        base.to_string()
    } else {
        format!("{base} с предупреждениями:\n{text}")
    }
}

/// Объём работы по путям/маскам внутри архива; пусто — весь архив.
/// `prefix*` — «всё в подпапке»: так `selected_paths` отдаёт выделенный
/// целиком уровень, и итог должен совпадать с выделением.
pub(crate) fn totals_for_entries(entries: &[Entry], selected: &[String]) -> (u64, u64) {
    let mut bytes = 0;
    let mut files = 0;
    for entry in entries {
        if entry.is_dir {
            continue;
        }
        let included = selected.is_empty()
            || selected.iter().any(|path| match path.strip_suffix('*') {
                Some(prefix) => entry.name.starts_with(prefix),
                None => entry.name == *path || entry.name.starts_with(&format!("{path}/")),
            });
        if included {
            bytes += entry.size;
            files += 1;
        }
    }
    (bytes, files)
}

/// Запускает операцию в фоновом потоке и отдаёт канал с прогрессом и результатом.
///
/// 7z на гигабайтах живёт минутами, поэтому окно ждать не должно: UI только
/// опрашивает канал. Прогресс из фонового потока приходит событиями, а не
/// напрямую в состояние — иначе пришлось бы держать мьютекс вокруг модели окна.
///
/// В замыкание передаётся отправитель сообщений: так операция может сначала
/// посчитать объём работы (`Totals`) и только потом запустить 7z.
pub(crate) fn spawn_job(
    operation: impl FnOnce(&mut dyn FnMut(JobMessage)) -> Result<JobOutcome, String> + Send + 'static,
) -> std::sync::mpsc::Receiver<JobMessage> {
    let (sender, receiver) = std::sync::mpsc::channel::<JobMessage>();

    std::thread::spawn(move || {
        let channel = sender.clone();
        let mut send = move |message: JobMessage| {
            let _ = channel.send(message);
        };
        let done = operation(&mut send);
        let _ = sender.send(JobMessage::Done(done));
    });

    receiver
}

impl MainWindow {
    pub(crate) fn cancel_job(&mut self) {
        if let Some(job) = &mut self.job {
            job.cancel.store(true, Ordering::Relaxed);
            // Окно прогресса закрываем, как 7-Zip: итог придёт в статус-бар.
            job.hidden = true;
            self.notice = Some("Отмена…".to_string());
        }
    }

    /// «Фон»: окно прогресса убираем, работа продолжается, статус-бар виден.
    pub(crate) fn job_to_background(&mut self) {
        if let Some(job) = &mut self.job {
            job.hidden = true;
        }
    }

    /// Объём работы для окна прогресса при распаковке и проверке.
    ///
    /// Считаем по полному листингу: выделенный каталог тянет за собой детей,
    /// поэтому сравниваем с префиксом, а не только с самим именем. В каталоге
    /// (архив ещё не открыт) объём неизвестен — окно покажет проценты без байтов.
    pub(crate) fn archive_totals(&self) -> (u64, u64) {
        let panel = self.active();
        if !panel.is_archive {
            return (0, 0);
        }
        self.totals_for(&panel.selected_paths())
    }

    /// Объём работы по явным путям внутри архива; пусто — весь архив.
    /// Выделенный каталог тянет за собой детей, поэтому сравниваем и с префиксом.
    pub(crate) fn totals_for(&self, selected: &[String]) -> (u64, u64) {
        totals_for_entries(&self.active().all_entries, selected)
    }

    /// Общая часть запуска фоновой операции: состояние задачи плюс опрос канала.
    pub(crate) fn start_job(
        &mut self,
        kind: JobKind,
        target: PathBuf,
        cancel: Arc<AtomicBool>,
        receiver: std::sync::mpsc::Receiver<JobMessage>,
        cx: &mut Context<Self>,
    ) {
        // Новая операция заменяет оставленное окно прошлой проверки.
        self.done_job = None;
        self.job = Some(Job {
            kind,
            cancel,
            percent: 0,
            item: String::new(),
            target,
            started: Instant::now(),
            total_bytes: 0,
            total_files: 0,
            seen_files: 0,
            hidden: false,
            open_after: Vec::new(),
            hashes: None,
            test_stats: None,
            done_elapsed: None,
            retry_dialog: None,
        });

        self.job_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let mut finished = false;
                let mut updated = false;
                // Канал закрылся, а значит фоновый поток упал или завершился, не прислав
                // результат. Без этого флага цикл крутился бы вечно каждые 100 мс.
                let mut disconnected = false;

                loop {
                    match receiver.try_recv() {
                        Ok(JobMessage::Progress(Progress::Percent(value))) => {
                            updated = true;
                            let _ = this.update(cx, |this, _| {
                                if let Some(job) = &mut this.job {
                                    job.percent = value;
                                }
                            });
                        }
                        Ok(JobMessage::Progress(Progress::Item(name))) => {
                            updated = true;
                            let _ = this.update(cx, |this, _| {
                                if let Some(job) = &mut this.job {
                                    job.item = name;
                                    // 7z с `-bb1` называет каждый файл: считаем их,
                                    // иначе «Файлов» в окне показывать нечего.
                                    job.seen_files += 1;
                                }
                            });
                        }
                        Ok(JobMessage::Totals { bytes, files }) => {
                            updated = true;
                            let _ = this.update(cx, |this, _| {
                                if let Some(job) = &mut this.job {
                                    job.total_bytes = bytes;
                                    job.total_files = files;
                                }
                            });
                        }
                        Ok(JobMessage::Hashes(report)) => {
                            updated = true;
                            let _ = this.update(cx, |this, _| {
                                if let Some(job) = &mut this.job {
                                    job.hashes = Some(report);
                                }
                            });
                        }
                        Ok(JobMessage::TestStats(stats)) => {
                            updated = true;
                            let _ = this.update(cx, |this, _| {
                                if let Some(job) = &mut this.job {
                                    job.test_stats = Some(stats);
                                }
                            });
                        }
                        Ok(JobMessage::Done(result)) => {
                            finished = true;
                            updated = true;
                            let _ = this.update(cx, |this, cx| this.finish_job(result, cx));
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            disconnected = true;
                            break;
                        }
                    }
                }

                if disconnected && !finished {
                    // Поток умер молча — показываем это как ошибку, а не висим.
                    let _ = this.update(cx, |this, cx| {
                        this.finish_job(
                            Err("фоновый поток завершился без результата".to_string()),
                            cx,
                        );
                    });
                    break;
                }

                if updated {
                    let _ = this.update(cx, |_, cx| cx.notify());
                }

                if finished {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
            }
        }));
    }

    pub(crate) fn finish_job(
        &mut self,
        result: Result<JobOutcome, String>,
        cx: &mut Context<Self>,
    ) {
        let mut job = self.job.take();
        let mut retry_dialog = job.as_mut().and_then(|job| job.retry_dialog.take());
        if let Err(error) = &result {
            match retry_dialog.as_mut() {
                Some(Modal::Create(dialog)) => dialog.error = Some(error.clone()),
                Some(Modal::Extract(dialog)) => dialog.error = Some(error.clone()),
                Some(Modal::Transfer(dialog)) => dialog.error = Some(error.clone()),
                _ => {}
            }
        } else {
            retry_dialog = None;
        }
        let kind = job.as_ref().map(|job| job.kind).unwrap_or(JobKind::Extract);
        let open_after = job
            .as_mut()
            .map(|job| std::mem::take(&mut job.open_after))
            .unwrap_or_default();
        let hashes = job.as_mut().and_then(|job| job.hashes.take());
        let test_stats = job.as_mut().and_then(|job| job.test_stats.take());

        // Окно прогресса проверки остаётся под окном итогов, как в 7-Zip.
        // Убранное в «Фон» не возвращаем; при отмене итогов нет — окно закрываем.
        let keep_progress = kind == JobKind::Test
            && !matches!(&result, Ok(outcome) if outcome.cancelled)
            && !job.as_ref().is_some_and(|job| job.hidden);
        if keep_progress && let Some(mut job) = job {
            job.percent = 100;
            job.done_elapsed = Some(job.started.elapsed());
            self.done_job = Some(job);
        }

        self.notice = Some(match (kind, result) {
            (JobKind::Extract, Ok(outcome)) if outcome.cancelled => {
                "Распаковка отменена".to_string()
            }
            (JobKind::Extract, Ok(outcome)) => with_warnings("Распаковка завершена", &outcome),
            (JobKind::Extract, Err(err)) => format!("Ошибка распаковки: {err}"),
            (JobKind::Create, Ok(outcome)) if outcome.cancelled => "Упаковка отменена".to_string(),
            (JobKind::Create, Ok(outcome)) => with_warnings("Архив обновлен", &outcome),
            (JobKind::Create, Err(err)) => format!("Ошибка упаковки: {err}"),
            (JobKind::Test, Ok(outcome)) if outcome.cancelled => "Проверка отменена".to_string(),
            (JobKind::Test, Ok(outcome)) => {
                // Как в 7-Zip: прогресс закрылся, поверх — маленькое окно итогов.
                // Открытую пользователем модалку не затираем: итог виден в статус-баре.
                if self.modal.is_none() {
                    self.modal = Some(Modal::TestResult(TestResultDialog {
                        stats: test_stats.unwrap_or_default(),
                        warnings: outcome.warnings.clone(),
                        error: None,
                    }));
                }
                // Код 1 — «проверено, но не всё»: архив цел, замечания в warnings.
                let base = if outcome.warnings.trim().is_empty() {
                    "Проверка завершена: ошибок нет"
                } else {
                    "Проверка завершена с замечаниями"
                };
                with_warnings(base, &outcome)
            }
            (JobKind::Test, Err(err)) => {
                if self.modal.is_none() {
                    self.modal = Some(Modal::TestResult(TestResultDialog {
                        stats: TestStats::default(),
                        warnings: String::new(),
                        error: Some(err.clone()),
                    }));
                }
                format!("Проверка не пройдена: {err}")
            }
            (JobKind::Copy, Ok(outcome)) if outcome.cancelled => "Копирование отменено".to_string(),
            (JobKind::Copy, Ok(outcome)) => with_warnings("Скопировано", &outcome),
            (JobKind::Copy, Err(err)) => format!("Ошибка копирования: {err}"),
            (JobKind::Move, Ok(outcome)) if outcome.cancelled => "Перемещение отменено".to_string(),
            (JobKind::Move, Ok(outcome)) => with_warnings("Перемещено", &outcome),
            (JobKind::Move, Err(err)) => format!("Ошибка перемещения: {err}"),
            (JobKind::Open, Ok(outcome)) if outcome.cancelled => "Открытие отменено".to_string(),
            (JobKind::Open, Ok(outcome)) => self.open_extracted(&open_after, &outcome),
            (JobKind::Open, Err(err)) => format!("Ошибка открытия: {err}"),
            (JobKind::Hash, Ok(outcome)) if outcome.cancelled => "Хеширование отменено".to_string(),
            (JobKind::Hash, Ok(outcome)) => {
                if let Some(report) = hashes
                    && self.modal.is_none()
                {
                    self.modal = Some(Modal::Hash(HashDialog {
                        report,
                        selected: 0,
                        scroll: UniformListScrollHandle::new(),
                    }));
                }
                with_warnings("Хеширование завершено", &outcome)
            }
            (JobKind::Hash, Err(err)) => format!("Ошибка хеширования: {err}"),
        });

        if let Some(dialog) = retry_dialog
            && self.modal.is_none()
        {
            self.modal = Some(dialog);
        }
        if self.modal.is_some() {
            self.address_edit = None;
        }

        // После создания/обновления архива, распаковки или копирования/перемещения
        // списки обеих панелей перечитываются: файлы могли появиться или удалиться.
        // Чтение — в фоне, окно не морозит даже большой сетевой папкой.
        if matches!(
            kind,
            JobKind::Create | JobKind::Extract | JobKind::Copy | JobKind::Move
        ) {
            let active = self.active_panel;
            self.reload_panel(active, cx, |_, _| {});
            if let Some(other) = self.two_panel.then_some(1 - active)
                && self.panels[other].loaded
            {
                self.reload_panel(other, cx, |_, _| {});
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod warning_tests {
    use super::{JobOutcome, with_warnings};

    #[test]
    fn operation_details_preserve_multiline_warnings() {
        let mut outcome = JobOutcome {
            cancelled: false,
            warnings: String::new(),
        };
        assert_eq!(with_warnings("Завершено", &outcome), "Завершено");
        outcome.warnings = "Warning\nlocked.txt\nAccess denied\n".to_string();
        assert_eq!(
            with_warnings("Завершено", &outcome),
            "Завершено с предупреждениями:\nWarning\nlocked.txt\nAccess denied"
        );
    }
}

#[cfg(test)]
mod totals_tests {
    use super::totals_for_entries;
    use archiver_core::Entry;

    fn sample() -> Vec<Entry> {
        let mut a = Entry::file("docs/a.txt");
        a.size = 10;
        let mut b = Entry::file("docs/sub/b.txt");
        b.size = 20;
        let mut c = Entry::file("other.txt");
        c.size = 40;
        vec![a, b, c, Entry::dir("docs")]
    }

    #[test]
    fn wildcard_mask_totals_match_selected_level() {
        let entries = sample();
        assert_eq!(
            totals_for_entries(&entries, &["docs/*".to_string()]),
            (30, 2),
            "маска уровня должна считать всех детей"
        );
    }

    #[test]
    fn explicit_paths_and_empty_selection() {
        let entries = sample();
        assert_eq!(
            totals_for_entries(&entries, &["docs/a.txt".to_string()]),
            (10, 1)
        );
        assert_eq!(totals_for_entries(&entries, &["docs".to_string()]), (30, 2));
        assert_eq!(totals_for_entries(&entries, &[]), (70, 3));
    }
}

use crate::prelude::*;

/// Сообщения из фонового прогона `7z b`.
pub(crate) enum BenchEvent {
    /// Очередная строка вывода 7z (шапка, строка таблицы, итог).
    Line(String),
    /// Пргон завершился: результат или ошибка запуска.
    Done(Result<BenchOutcome, String>),
}

/// Окно бенчмарка: сводка `7z b` в виде таблиц, как в 7-Zip FM.
///
/// Вывод 7z приходит одним куском (CRT буферизует пайп), поэтому разбираем его
/// в [`BenchReport`] по завершении прогона. Сырые строки храним до тех пор: если
/// формат вывода окажется незнакомым, окно покажет их как есть.
/// `task` держим здесь же: при закрытии/перезапуске диалог дропается вместе с
/// задачей опроса, а `cancel` убивает сам процесс 7z.
pub(crate) struct BenchDialog {
    pub(crate) lines: Vec<String>,
    pub(crate) report: BenchReport,
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) running: bool,
    pub(crate) cancelled: bool,
    pub(crate) error: Option<String>,
    /// Отсчёт «Прошло»: тикает, пока идёт прогон.
    pub(crate) started: Instant,
    pub(crate) elapsed: Option<Duration>,
    pub(crate) scroll: UniformListScrollHandle,
    pub(crate) task: Option<Task<()>>,
}

impl MainWindow {
    /// «Сервис → Тестирование производительности»: открыть окно и запустить прогон.
    pub(crate) fn open_bench(&mut self, cx: &mut Context<Self>) {
        if self.job.is_some() {
            self.notice = Some("Дождитесь окончания текущей операции".to_string());
            return;
        }
        self.start_bench(cx);
    }

    /// «Стоп»: убить текущий 7z. Окно перестанет быть «идущим», когда придёт
    /// `Done` — до этого момента флаг только просит процесс завершиться.
    pub(crate) fn stop_bench(&mut self, cx: &mut Context<Self>) {
        if let Some(Modal::Bench(dialog)) = &mut self.modal {
            dialog.cancel.store(true, Ordering::Relaxed);
        }
        cx.notify();
    }

    /// Запускает `7z b` в фоне и опрашивает канал строк раз в 250 мс.
    ///
    /// Повторный вызов (кнопка «Перезапуск») гасит прошлый прогон прежде, чем
    /// заменить диалог: старая задача опроса дропается вместе с прежним окном.
    fn start_bench(&mut self, cx: &mut Context<Self>) {
        if let Some(Modal::Bench(old)) = &self.modal {
            old.cancel.store(true, Ordering::Relaxed);
        }

        let tool = match Locator::for_current_exe()
            .map_err(|err| err.to_string())
            .and_then(|locator| locator.resolve().map_err(|err| err.to_string()))
        {
            Ok(tool) => tool,
            Err(err) => {
                self.notice = Some(format!("7-Zip недоступен: {err}"));
                return;
            }
        };

        let exe = tool.info.exe.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let (sender, receiver) = std::sync::mpsc::channel::<BenchEvent>();

        std::thread::spawn(move || {
            let mut emit = |line: String| {
                let _ = sender.send(BenchEvent::Line(line));
            };
            let result = run_benchmark(&exe, &flag, &mut emit).map_err(|err| err.to_string());
            let _ = sender.send(BenchEvent::Done(result));
        });

        self.notice = None;
        self.modal = Some(Modal::Bench(BenchDialog {
            lines: Vec::new(),
            report: BenchReport::default(),
            cancel,
            running: true,
            cancelled: false,
            error: None,
            started: Instant::now(),
            elapsed: None,
            scroll: UniformListScrollHandle::new(),
            task: None,
        }));

        let task = cx.spawn(async move |this, cx| {
            loop {
                let mut updated = false;
                let mut finished = false;

                loop {
                    match receiver.try_recv() {
                        Ok(BenchEvent::Line(line)) => {
                            updated = true;
                            let _ = this.update(cx, |this, _| {
                                if let Some(Modal::Bench(dialog)) = &mut this.modal {
                                    dialog.lines.push(line);
                                    let last = dialog.lines.len().saturating_sub(1);
                                    // Живой вывод держим внизу: новая строка сразу видна.
                                    dialog.scroll.scroll_to_item(last, ScrollStrategy::Nearest);
                                }
                            });
                        }
                        Ok(BenchEvent::Done(result)) => {
                            finished = true;
                            updated = true;
                            let _ = this.update(cx, |this, _| {
                                if let Some(Modal::Bench(dialog)) = &mut this.modal {
                                    dialog.running = false;
                                    dialog.elapsed = Some(dialog.started.elapsed());
                                    // Вывод приходит целиком: разбираем один раз здесь.
                                    dialog.report = parse_bench_output(&dialog.lines);
                                    match result {
                                        // Отмена — не ошибка: что успело прийти, уже разобрано.
                                        Ok(outcome) if outcome.cancelled => {
                                            dialog.cancelled = true;
                                        }
                                        Ok(outcome) if outcome.code != 0 => {
                                            dialog.error = Some(bench_error(&outcome));
                                        }
                                        Ok(_) => {}
                                        Err(err) => dialog.error = Some(err),
                                    }
                                }
                            });
                            // Поток шлёт Done последним и закрывает канал: следующее
                            // чтение даст `Disconnected`, это не обрыв, а штатный конец.
                            break;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            // Поток умер, не прислав Done: не оставляем «идёт тест».
                            finished = true;
                            let _ = this.update(cx, |this, _| {
                                if let Some(Modal::Bench(dialog)) = &mut this.modal {
                                    dialog.running = false;
                                    dialog.elapsed = Some(dialog.started.elapsed());
                                    dialog.report = parse_bench_output(&dialog.lines);
                                    dialog.error =
                                        Some("Поток тестирования завершился неожиданно".into());
                                }
                            });
                            break;
                        }
                    }
                }

                // Пока прогон идёт, перерисовываемся каждый тик: «Прошло» тикает.
                if updated || !finished {
                    let _ = this.update(cx, |_, cx| cx.notify());
                }
                if finished {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
            }
        });

        if let Some(Modal::Bench(dialog)) = &mut self.modal {
            dialog.task = Some(task);
        }
    }
}

/// Причина ненулевого кода: первая непустая строка stderr, иначе голый код.
fn bench_error(outcome: &BenchOutcome) -> String {
    let reason = outcome
        .stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    if reason.is_empty() {
        format!("7z завершился с кодом {}", outcome.code)
    } else {
        reason.to_string()
    }
}

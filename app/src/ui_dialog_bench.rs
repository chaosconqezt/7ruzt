use crate::prelude::*;

/// Моноширинная гарнитура для сырого вывода `7z b`.
///
/// Единственное место с моноширинным шрифтом, и только для запасного варианта:
/// если формат вывода 7z окажется незнакомым, показываем строки консоли как есть.
/// Это показ чужого вывода, а не наш текст — потому исключение из правила «одна
/// гарнитура». Windows-first: Cascadia Mono / Consolas есть в системе; на Linux/macOS GPUI
/// подставит запасной.
const MONO_FONT: &str = "Cascadia Mono";

/// Ширина окна: три поля настроек сверху, блок частот справа, две таблицы.
const BENCH_WIDTH: f32 = 780.;

/// Ширины колонок таблиц «Упаковка» и «Распаковка».
const COL_LABEL: f32 = 96.;
const COL_SIZE: f32 = 84.;
const COL_SPEED: f32 = 120.;
const COL_USAGE: f32 = 88.;
const COL_PER_USAGE: f32 = 112.;
const COL_RATING: f32 = 112.;

impl MainWindow {
    /// «Тестирование производительности»: разобранный вывод `7z b` как в
    /// 7-Zip FM — настройки, частоты CPU, таблицы сжатия и распаковки, итоги.
    ///
    /// 7z буферизует stdout в пайпе и отдаёт всё разом в конце, поэтому до
    /// `Done` показываем скелетоны: места под цифры уже размечены, и окно не
    /// прыгает, когда данные приходят.
    pub(crate) fn bench_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(Modal::Bench(dialog)) = &self.modal else {
            return div().into_any_element();
        };

        let c = Colors::new(cx.theme());
        let running = dialog.running;
        let cancelled = dialog.cancelled;
        let error = dialog.error.clone();
        let report = dialog.report.clone();
        let raw = dialog.lines.clone();
        let scroll = dialog.scroll.clone();
        let elapsed = dialog.elapsed.unwrap_or_else(|| dialog.started.elapsed());

        // Скелетоны только пока прогон идёт и разбирать ещё нечего.
        let loading = running && report.rows.is_empty();
        let current = report.current();
        let compressing_total = report.total_compressing();
        let decompressing_total = report.total_decompressing();

        let dict = report.dict_mb().map(|mb| format!("{mb} МБ"));
        let memory = match (report.ram_usage_mb, report.ram_size_mb) {
            (Some(used), Some(total)) => Some(format!("{used} MB / {total} MB")),
            (None, Some(total)) => Some(format!("{total} MB")),
            _ => None,
        };
        let threads = match (report.bench_threads, report.hardware_threads) {
            (Some(bench), Some(hardware)) => Some(format!("{bench} / {hardware}")),
            (Some(bench), None) => Some(bench.to_string()),
            _ => None,
        };

        let top = h_flex()
            .items_start()
            .gap_8()
            .child(
                v_flex()
                    .flex_none()
                    .gap_2()
                    .child(bench_control("Размер словаря:", dict, loading, &c))
                    .child(bench_control("Объём памяти:", memory, loading, &c))
                    .child(bench_control("Число потоков:", threads, loading, &c)),
            )
            .child(bench_freqs(&report, loading, &c));

        // Если разбор не дал ни одной строки (незнакомый вывод 7z), прежнее
        // поведение остаётся запасным вариантом: сырые строки консоли.
        let fallback = !running && report.rows.is_empty() && !raw.is_empty();
        let results = if fallback {
            let lines = raw.clone();
            let count = lines.len();
            let line_h = (theme::font_size() * 1.4).round();
            let list = uniform_list("bench-raw", count, move |range, _window, _cx| {
                let mut items: Vec<AnyElement> = Vec::with_capacity(range.len());
                for index in range {
                    let text = lines.get(index).cloned().unwrap_or_default();
                    items.push(
                        div()
                            .h(px(line_h))
                            .px_2()
                            .text_sm()
                            .font_family(MONO_FONT)
                            .whitespace_nowrap()
                            .child(text)
                            .into_any_element(),
                    );
                }
                items
            })
            .h(px(240.))
            .track_scroll(&scroll)
            .into_any_element();

            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(c.muted)
                        .child("Не удалось разобрать вывод 7z — показываем как есть:"),
                )
                .child(
                    div()
                        .border_1()
                        .border_color(c.border)
                        .rounded_sm()
                        .bg(c.bg)
                        .child(list),
                )
                .into_any_element()
        } else {
            v_flex()
                .gap_3()
                .child(bench_table(
                    "Упаковка",
                    current.map(|row| row.compressing),
                    compressing_total,
                    loading,
                    &c,
                ))
                .child(bench_table(
                    "Распаковка",
                    current.map(|row| row.decompressing),
                    decompressing_total,
                    loading,
                    &c,
                ))
                .into_any_element()
        };

        let status: AnyElement = if let Some(error) = &error {
            div()
                .text_sm()
                .text_color(c.error())
                .whitespace_normal()
                .child(error.clone())
                .into_any_element()
        } else if running {
            h_flex()
                .items_center()
                .gap_2()
                .child(Spinner::new().control_size().color(c.accent))
                .child(
                    div()
                        .text_sm()
                        .text_color(c.muted)
                        .child("Идёт тестирование…"),
                )
                .into_any_element()
        } else if cancelled {
            div()
                .text_sm()
                .text_color(c.muted)
                .child("Остановлено")
                .into_any_element()
        } else {
            div()
                .text_sm()
                .text_color(c.muted)
                .child("Готово")
                .into_any_element()
        };

        let body = v_flex()
            .px_4()
            .py_3()
            .gap_3()
            .child(top)
            .child(results)
            .child(
                h_flex()
                    .items_start()
                    .gap_8()
                    .child(
                        v_flex()
                            .flex_none()
                            .gap_1()
                            .child(bench_stat(
                                "Прошло:",
                                format!("{} с", elapsed.as_secs()),
                                &c,
                            ))
                            // 7z b без ключей делает один проход.
                            .child(bench_stat("Проходов:", "1".to_string(), &c)),
                    )
                    .child(bench_overall(&report, loading, &c)),
            )
            .child(bench_info(&report, loading, &c))
            .child(status);

        let stop = if running {
            dialog_button("bench-stop", "Стоп")
                .on_click(cx.listener(|this, _event, _window, cx| {
                    this.stop_bench(cx);
                }))
                .into_any_element()
        } else {
            Self::marked_button("Стоп").into_any_element()
        };

        let footer = h_flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(c.border)
            .child(Self::soon_button("✕ Помощь"))
            .child(div().flex_1())
            .child(
                dialog_button("bench-restart", "Перезапуск").on_click(cx.listener(
                    |this, _event, _window, cx| {
                        this.open_bench(cx);
                        cx.notify();
                    },
                )),
            )
            .child(stop)
            .child(
                dialog_button("bench-cancel", "Отмена").on_click(cx.listener(
                    |this, _event, _window, cx| {
                        this.close_modal();
                        cx.notify();
                    },
                )),
            );

        dialog_layer(
            &c,
            dialog_card(
                BENCH_WIDTH,
                "Тестирование производительности",
                self.dialog_close("bench-x", cx),
                body,
                footer,
                &c,
            ),
        )
        .into_any_element()
    }
}

/// Строка настройки: подпись и погашенное поле с фактическим значением.
///
/// Полей-выборов, как в 7-Zip FM, у нас нет: 7z сам подбирает словарь и потоки,
/// а мы показываем то, что он напечатал в шапке.
fn bench_control(
    label: &'static str,
    value: Option<String>,
    loading: bool,
    c: &Colors,
) -> AnyElement {
    h_flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(px(120.))
                .flex_none()
                .text_sm()
                .text_color(c.muted)
                .child(label),
        )
        .child(if loading {
            Skeleton::new()
                .secondary()
                .w(px(120.))
                .h(px(theme::font_size() * 1.6))
                .into_any_element()
        } else {
            div()
                .min_w(px(120.))
                .px_2()
                .py_0p5()
                .border_1()
                .border_color(c.border)
                .rounded_sm()
                .text_sm()
                .whitespace_nowrap()
                .child(value.unwrap_or_else(|| "—".to_string()))
                .into_any_element()
        })
        .into_any_element()
}

/// Частоты CPU по числу потоков — правый блок шапки, как в 7-Zip FM.
fn bench_freqs(report: &BenchReport, loading: bool, c: &Colors) -> AnyElement {
    let mut block = v_flex().flex_1().min_w(px(0.)).gap_1();
    if report.freqs.is_empty() {
        block = if loading {
            block
                .child(
                    Skeleton::new()
                        .secondary()
                        .w(px(280.))
                        .h(px(theme::font_size())),
                )
                .child(
                    Skeleton::new()
                        .secondary()
                        .w(px(240.))
                        .h(px(theme::font_size())),
                )
                .child(
                    Skeleton::new()
                        .secondary()
                        .w(px(260.))
                        .h(px(theme::font_size())),
                )
        } else {
            block.child(
                div()
                    .text_sm()
                    .text_color(c.muted)
                    .child("Частоты CPU недоступны"),
            )
        };
    } else {
        for freq in &report.freqs {
            block = block.child(
                v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .text_color(c.muted)
                            .child(format!("{}T Frequency (MHz):", freq.threads)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .whitespace_normal()
                            .child(freq.values.clone()),
                    ),
            );
        }
    }
    block.into_any_element()
}

/// Таблица одной операции: «Текущий» и «Итоговый» с теми же колонками, что в 7-Zip.
fn bench_table(
    title: &'static str,
    current: Option<BenchValue>,
    total: Option<BenchValue>,
    loading: bool,
    c: &Colors,
) -> AnyElement {
    v_flex()
        .gap_1()
        .child(div().text_sm().child(title))
        .child(
            v_flex()
                .border_1()
                .border_color(c.border)
                .rounded_sm()
                .overflow_hidden()
                .child(bench_head_row(c))
                .child(bench_value_row("Текущий", current, loading, c))
                .child(bench_value_row("Итоговый", total, loading, c)),
        )
        .into_any_element()
}

fn bench_head_row(c: &Colors) -> AnyElement {
    h_flex()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .bg(c.header())
        .child(div().w(px(COL_LABEL)).flex_none())
        .child(bench_head_cell("Размер", COL_SIZE, c))
        .child(bench_head_cell("Скорость", COL_SPEED, c))
        .child(bench_head_cell("Нагрузка", COL_USAGE, c))
        .child(bench_head_cell("Рейтинг / Нагр.", COL_PER_USAGE, c))
        .child(bench_head_cell("Рейтинг", COL_RATING, c))
        .into_any_element()
}

fn bench_value_row(
    label: &'static str,
    value: Option<BenchValue>,
    loading: bool,
    c: &Colors,
) -> AnyElement {
    let text = value.map(|value| {
        (
            bench_speed(value.speed),
            bench_usage(value.usage),
            bench_gips(value.per_usage),
            bench_gips(value.rating),
        )
    });
    let (speed, usage, per_usage, rating) = match text {
        Some((speed, usage, per_usage, rating)) => {
            (Some(speed), Some(usage), Some(per_usage), Some(rating))
        }
        None => (None, None, None, None),
    };

    h_flex()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .border_t_1()
        .border_color(c.border)
        .child(div().w(px(COL_LABEL)).flex_none().text_sm().child(label))
        // Размер обработанных данных 7z в консольном выводе не печатает —
        // цифру не выдумываем.
        .child(bench_num_cell(COL_SIZE, None, loading, c))
        .child(bench_num_cell(COL_SPEED, speed, loading, c))
        .child(bench_num_cell(COL_USAGE, usage, loading, c))
        .child(bench_num_cell(COL_PER_USAGE, per_usage, loading, c))
        .child(bench_num_cell(COL_RATING, rating, loading, c))
        .into_any_element()
}

fn bench_head_cell(text: &'static str, width: f32, c: &Colors) -> AnyElement {
    div()
        .w(px(width))
        .flex_none()
        .flex()
        .justify_end()
        .text_sm()
        .text_color(c.muted)
        .child(text)
        .into_any_element()
}

fn bench_num_cell(width: f32, text: Option<String>, loading: bool, c: &Colors) -> AnyElement {
    let cell = div()
        .w(px(width))
        .flex_none()
        .flex()
        .justify_end()
        .text_sm()
        .text_color(c.fg);
    match text {
        Some(text) => cell.child(text).into_any_element(),
        None if loading => cell
            .child(
                Skeleton::new()
                    .secondary()
                    .w(px((width - 24.).max(24.)))
                    .h(px(theme::font_size())),
            )
            .into_any_element(),
        None => cell.child("—").into_any_element(),
    }
}

/// Итоговые рейтинги: упаковка (`Tot`) и распаковка (`Avr`, если `Tot` её не дал).
fn bench_overall(report: &BenchReport, loading: bool, c: &Colors) -> AnyElement {
    let compressing = report
        .total_compressing()
        .map(|value| bench_gips(value.rating));
    let decompressing = report
        .total_decompressing()
        .map(|value| bench_gips(value.rating));

    let cell = |label: &'static str, value: Option<String>| -> AnyElement {
        h_flex()
            .items_center()
            .gap_2()
            .child(div().text_sm().text_color(c.muted).child(label))
            .child(match value {
                Some(text) => div().text_sm().child(text).into_any_element(),
                None if loading => Skeleton::new()
                    .secondary()
                    .w(px(84.))
                    .h(px(theme::font_size()))
                    .into_any_element(),
                None => div()
                    .text_sm()
                    .text_color(c.muted)
                    .child("—")
                    .into_any_element(),
            })
            .into_any_element()
    };

    v_flex()
        .gap_1()
        .child(div().text_sm().text_color(c.muted).child("Общий рейтинг"))
        .child(
            v_flex()
                .gap_1()
                .px_3()
                .py_2()
                .border_1()
                .border_color(c.border)
                .rounded_sm()
                .child(cell("Упаковка:", compressing))
                .child(cell("Распаковка:", decompressing)),
        )
        .into_any_element()
}

fn bench_stat(label: &'static str, value: String, c: &Colors) -> AnyElement {
    h_flex()
        .items_center()
        .gap_2()
        .child(div().text_sm().text_color(c.muted).child(label))
        .child(div().text_sm().child(value))
        .into_any_element()
}

/// Подписи системы по центру, как в подвале окна 7-Zip.
fn bench_info(report: &BenchReport, loading: bool, c: &Colors) -> AnyElement {
    let mut block = v_flex().items_center().gap_0p5();
    let mut any = false;
    for text in [
        report.cpu.clone(),
        report.cpu_details.clone(),
        report.os.clone(),
        report.version.clone(),
    ]
    .into_iter()
    .flatten()
    {
        any = true;
        block = block.child(
            div()
                .text_sm()
                .text_color(c.muted)
                .whitespace_normal()
                .child(text),
        );
    }
    if !any {
        block = block.child(if loading {
            Skeleton::new()
                .secondary()
                .w(px(320.))
                .h(px(theme::font_size()))
                .into_any_element()
        } else {
            div()
                .text_sm()
                .text_color(c.muted)
                .child("Данные о системе недоступны")
                .into_any_element()
        });
    }
    block.into_any_element()
}

fn bench_speed(kib_per_second: f64) -> String {
    format!(
        "{} КБ/с",
        group_digits(kib_per_second.round().max(0.) as u64)
    )
}

fn bench_usage(percent: f64) -> String {
    format!("{percent:.0}%")
}

fn bench_gips(mips: f64) -> String {
    format!("{:.3} GIPS", mips / 1000.)
}

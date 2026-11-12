use crate::prelude::*;
impl MainWindow {
    /// Окно прогресса операции — как в 7-Zip: проценты, время, текущий файл,
    /// полоса и «Фон / Пауза / Отмена». После проверки окно остаётся под
    /// итогами: время заморожено, единственная кнопка — «Закрыть».
    ///
    /// Показываем только то, что реально есть: процент и имя файла приходят от
    /// 7z (`-bsp1`, `-bb1`), объём — из своего обхода, остальное считается из
    /// них. Точных цифр GUI 7-Zip (он берёт их через COM) у нас нет, поэтому
    /// «Осталось» и скорость помечены как оценки, а «Пауза» — «Y».
    pub(crate) fn progress_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(job) = self
            .job
            .as_ref()
            .filter(|job| !job.hidden)
            .or(self.done_job.as_ref())
        else {
            return div().into_any_element();
        };
        let c = Colors::new(cx.theme());
        let percent = job.percent;
        let finished = job.done_elapsed.is_some();

        let stat = |label: &'static str, value: String, c: &Colors| {
            h_flex()
                .justify_between()
                .items_center()
                .gap_4()
                .child(div().text_sm().text_color(c.muted).child(label))
                .child(div().text_sm().child(value))
        };
        let size = |value: Option<u64>| value.map(human).unwrap_or_else(|| "—".to_string());

        let elapsed = format_clock(job.done_elapsed.unwrap_or_else(|| job.started.elapsed()));
        let remaining = if finished {
            format_clock(Duration::ZERO)
        } else {
            job.remaining()
                .map(|left| format!("≈ {}", format_clock(left)))
                .unwrap_or_else(|| "—".to_string())
        };
        let files = if job.total_files > 0 {
            format!("{} / {}", job.seen_files, job.total_files)
        } else {
            job.seen_files.to_string()
        };
        let item = if job.item.is_empty() {
            "—".to_string()
        } else {
            job.item.clone()
        };

        let mut right = v_flex()
            .flex_1()
            .min_w(px(0.))
            .gap_1()
            .child(stat(
                "Всего:",
                size((job.total_bytes > 0).then_some(job.total_bytes)),
                &c,
            ))
            .child(stat(
                "Скорость:",
                job.speed()
                    .map(|speed| format!("{}/s", human(speed)))
                    .unwrap_or_else(|| "—".to_string()),
                &c,
            ))
            .child(stat("Размер:", size(job.processed_bytes()), &c));

        if job.kind == JobKind::Create {
            right = right
                .child(stat("Сжатый:", size(job.archive_size()), &c))
                .child(stat(
                    "Степень сжатия:",
                    job.ratio()
                        .map(|ratio| format!("{ratio}%"))
                        .unwrap_or_else(|| "—".to_string()),
                    &c,
                ));
        }

        let body = v_flex()
            .px_4()
            .py_3()
            .gap_3()
            .child(
                h_flex()
                    .items_start()
                    .gap_8()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_1()
                            .child(stat("Прошло:", elapsed, &c))
                            .child(stat("Осталось:", remaining, &c))
                            .child(stat("Файлов:", files, &c))
                            .child(div().mt_2().text_sm().child(job.kind.action()))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(c.muted)
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .child(item),
                            ),
                    )
                    .child(right),
            )
            .child(
                ProgressBar::new("job-progress")
                    .value(percent as f32)
                    .color(c.green)
                    .h(px(18.)),
            );

        let footer = if finished {
            h_flex()
                .justify_end()
                .px_3()
                .py_2()
                .border_t_1()
                .border_color(c.border)
                .child(
                    dialog_button("progress-close", "Закрыть").on_click(cx.listener(
                        |this, _event, _window, cx| {
                            this.done_job = None;
                            cx.notify();
                        },
                    )),
                )
        } else {
            h_flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .border_t_1()
                .border_color(c.border)
                .child(dialog_button("progress-bg", "Фон").on_click(cx.listener(
                    |this, _event, window, cx| {
                        this.job_to_background();
                        window.focus(&this.focus, cx);
                        cx.notify();
                    },
                )))
                .child(Self::impossible_button("Y Пауза"))
                .child(div().flex_1())
                .child(
                    dialog_button("progress-cancel", "Отмена").on_click(cx.listener(
                        |this, _event, _window, cx| {
                            this.cancel_job();
                            cx.notify();
                        },
                    )),
                )
        };

        let title = format!("{percent}% {} {}", job.kind.title(), job.target.display());

        dialog_layer(
            &c,
            dialog_card(
                720.,
                div().whitespace_nowrap().overflow_hidden().child(title),
                div(),
                body,
                footer,
                &c,
            ),
        )
        .into_any_element()
    }
}

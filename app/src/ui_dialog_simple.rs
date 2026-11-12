use crate::prelude::*;
impl MainWindow {
    pub(crate) fn notice_overlay(&self, text: &str, cx: &mut Context<Self>) -> AnyElement {
        let c = Colors::new(cx.theme());
        let copy = text.to_string();
        let body = div()
            .id("notice-text")
            .max_h(px(360.))
            .overflow_y_scroll()
            .px_4()
            .py_3()
            .whitespace_normal()
            .child(text.to_string());
        let footer = h_flex()
            .justify_end()
            .gap_2()
            .px_3()
            .py_2()
            .child(
                dialog_button("notice-copy", "Копировать").on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                }),
            )
            .child(
                dialog_button("notice-close", "Закрыть").on_click(cx.listener(
                    |this, _, window, cx| {
                        this.close_modal();
                        window.focus(&this.focus, cx);
                        cx.notify();
                    },
                )),
            );
        dialog_layer(
            &c,
            dialog_card(
                640.,
                "Результат операции",
                self.dialog_close("notice-x", cx),
                body,
                footer,
                &c,
            ),
        )
        .into_any_element()
    }

    pub(crate) fn extract_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(Modal::Extract(dialog)) = &self.modal else {
            return div().into_any_element();
        };

        let c = Colors::new(cx.theme());
        let subfolder = dialog.subfolder;
        let show_password = dialog.show_password;
        let title = div()
            .max_w(px(590.))
            .whitespace_nowrap()
            .overflow_hidden()
            .child(format!("Извлечь : {}", dialog.archive.display()));

        let body = v_flex()
            .px_4()
            .py_3()
            .gap_2()
            .child(div().text_sm().child("Распаковывать в:"))
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(control_input(&dialog.destination).w_full()),
                    )
                    .child(dialog_button("extract-pick", "…").on_click(cx.listener(
                        |this, _event, window, cx| {
                            this.open_picker(PickerTarget::Extract, window, cx);
                            cx.notify();
                        },
                    ))),
            )
            // Галочка с именем архива: добавляет подпапку к пути — как «rox\»
            // в 7-Zip. Поле выше остаётся базовой папкой.
            .child(
                Checkbox::new("ex-subfolder")
                    .control_size()
                    .label(format!("{}\\", dialog.subfolder_name))
                    .checked(subfolder)
                    .on_change(cx.listener(|this, checked: &bool, _window, cx| {
                        if let Some(Modal::Extract(dialog)) = &mut this.modal {
                            dialog.subfolder = *checked;
                        }
                        cx.notify();
                    })),
            )
            .child(
                h_flex()
                    .items_start()
                    .gap_6()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_1()
                            .child(div().text_sm().child("Пути к файлам:"))
                            .child(Select::new(&dialog.paths_select).control_size().w_full())
                            .child(div().mt_1().text_sm().child("Перезапись:"))
                            .child(
                                Select::new(&dialog.overwrite_select)
                                    .control_size()
                                    .w_full(),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_1()
                            .child(div().text_sm().child("Пароль"))
                            .child(control_input(&dialog.password).w_full())
                            .child(
                                Checkbox::new("ex-show-pass")
                                    .control_size()
                                    .label("Показать пароль")
                                    .checked(show_password)
                                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                        if let Some(Modal::Extract(dialog)) = &mut this.modal {
                                            dialog.show_password = *checked;
                                            let masked = !*checked;
                                            dialog.password.update(cx, |state, cx| {
                                                state.set_masked(masked, window, cx)
                                            });
                                        }
                                        cx.notify();
                                    })),
                            )
                            // У CLI 7z нет отдельного переключателя прав: они
                            // восстанавливаются сами, если сохранены в архиве.
                            .child(
                                Checkbox::new("ex-perms")
                                    .control_size()
                                    .label("Устанавливать права доступа")
                                    .checked(false)
                                    .disabled(true)
                                    .tooltip(
                                        "7z восстанавливает права сам, если они сохранены \
                                         в архиве: отдельного переключателя в CLI нет",
                                    ),
                            ),
                    ),
            )
            .children(dialog.error.as_ref().map(|error| {
                div()
                    .text_color(c.error())
                    .whitespace_normal()
                    .child(error.clone())
            }));

        let footer = h_flex()
            .justify_end()
            .gap_2()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(c.border)
            .child(dialog_button("ex-ok", "OK").on_click(cx.listener(
                |this, _event, window, cx| {
                    this.submit_extract(cx);
                    if this.modal.is_none() {
                        window.focus(&this.focus, cx);
                    }
                    cx.notify();
                },
            )))
            .child(dialog_button("ex-cancel", "Отмена").on_click(cx.listener(
                |this, _event, window, cx| {
                    this.close_modal();
                    window.focus(&this.focus, cx);
                    cx.notify();
                },
            )))
            .child(Self::soon_button("Помощь"));

        dialog_layer(
            &c,
            dialog_card(
                660.,
                title,
                self.dialog_close("extract-x", cx),
                body,
                footer,
                &c,
            ),
        )
        .into_any_element()
    }

    /// Диалог проверки: нужен только для архива с зашифрованным содержимым.
    pub(crate) fn test_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(Modal::Test(dialog)) = &self.modal else {
            return div().into_any_element();
        };

        let c = Colors::new(cx.theme());

        let body = v_flex()
            .px_4()
            .py_3()
            .gap_2()
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(DIALOG_LABEL_W)).text_sm().child("Архив:"))
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(c.muted)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(dialog.archive.display().to_string()),
                    ),
            )
            .child(self.dialog_row("Пароль:", control_input(&dialog.password).w_full()))
            .child(
                div()
                    .text_sm()
                    .text_color(c.muted)
                    .whitespace_normal()
                    .child(
                        "Содержимое архива зашифровано: без пароля контрольные \
                         суммы не проверить.",
                    ),
            )
            // Пауза невозможна: CLI 7z не умеет приостановку, а COM/unsafe
            // запрещены. Помечаем «Y», а не изображаем работающим.
            .child(
                v_flex()
                    .mt_1()
                    .px_3()
                    .py_2()
                    .gap_1()
                    .border_1()
                    .border_color(c.border)
                    .rounded_sm()
                    .child(div().text_sm().text_color(c.muted).child("Ограничения:"))
                    .child(Self::impossible_row("Пауза: приостановка процесса", &c)),
            );

        let footer = h_flex()
            .justify_end()
            .gap_2()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(c.border)
            .child(dialog_button("test-ok", "Проверить").on_click(cx.listener(
                |this, _event, _window, cx| {
                    this.submit_test(cx);
                    cx.notify();
                },
            )))
            .child(dialog_button("test-cancel", "Отмена").on_click(cx.listener(
                |this, _event, window, cx| {
                    this.close_modal();
                    window.focus(&this.focus, cx);
                    cx.notify();
                },
            )));

        dialog_layer(
            &c,
            dialog_card(
                640.,
                "Проверить архив",
                self.dialog_close("test-x", cx),
                body,
                footer,
                &c,
            ),
        )
        .into_any_element()
    }

    /// Итог проверки как в 7-Zip: маленькое окно со счётчиками и вердиктом.
    /// Открывается поверх закрывшегося окна прогресса; при отмене итогов нет.
    pub(crate) fn test_result_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(Modal::TestResult(dialog)) = &self.modal else {
            return div().into_any_element();
        };

        let c = Colors::new(cx.theme());
        let row = |label: &'static str, value: String| {
            h_flex()
                .gap_2()
                .child(div().w(px(70.)).flex_none().text_sm().child(label))
                .child(div().text_sm().whitespace_nowrap().child(value))
        };
        let bytes = |value: u64| format!("{} байтов : {}", value, human_size(value));

        // Порядок строк — как в оригинале; чего 7z не напечатал, того не показываем.
        let mut rows = v_flex().gap_1().child(row("Архивов:", "1".to_string()));
        if let Some(compressed) = dialog.stats.compressed {
            rows = rows.child(row("Сжатый:", bytes(compressed)));
        }
        if let Some(folders) = dialog.stats.folders {
            rows = rows.child(row("Папок:", folders.to_string()));
        }
        if let Some(files) = dialog.stats.files {
            rows = rows.child(row("Файлов:", files.to_string()));
        }
        if let Some(size) = dialog.stats.size {
            rows = rows.child(row("Размер:", bytes(size)));
        }

        let verdict = if let Some(error) = &dialog.error {
            div()
                .text_sm()
                .text_color(c.error())
                .whitespace_normal()
                .child(error.clone())
        } else if !dialog.warnings.trim().is_empty() {
            div()
                .text_sm()
                .text_color(c.error())
                .whitespace_normal()
                .child(dialog.warnings.trim().to_string())
        } else {
            div().text_sm().child("Ошибок не найдено")
        };

        let body = v_flex().px_4().py_3().gap_3().child(rows).child(verdict);

        let footer = h_flex()
            .justify_end()
            .gap_2()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(c.border)
            .child(dialog_button("test-result-ok", "OK").on_click(cx.listener(
                |this, _event, window, cx| {
                    this.close_modal();
                    window.focus(&this.focus, cx);
                    cx.notify();
                },
            )));

        dialog_layer(
            &c,
            dialog_card(
                380.,
                "Тестирование",
                self.dialog_close("test-result-x", cx),
                body,
                footer,
                &c,
            ),
        )
        .into_any_element()
    }

    /// Пароль для открытия файла из шифрованного архива (Enter/F3).
    pub(crate) fn open_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(Modal::Open(dialog)) = &self.modal else {
            return div().into_any_element();
        };

        let c = Colors::new(cx.theme());

        let body = v_flex()
            .px_4()
            .py_3()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(c.muted)
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(dialog.archive.display().to_string()),
            )
            .child(self.dialog_row("Пароль:", control_input(&dialog.password).w_full()))
            .child(
                div()
                    .text_sm()
                    .text_color(c.muted)
                    .whitespace_normal()
                    .child("Содержимое архива зашифровано: без пароля файл не извлечь."),
            );

        let footer = h_flex()
            .justify_end()
            .gap_2()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(c.border)
            .child(dialog_button("open-ok", "Открыть").on_click(cx.listener(
                |this, _event, _window, cx| {
                    this.submit_open_password(cx);
                    cx.notify();
                },
            )))
            .child(dialog_button("open-cancel", "Отмена").on_click(cx.listener(
                |this, _event, window, cx| {
                    this.close_modal();
                    window.focus(&this.focus, cx);
                    cx.notify();
                },
            )));

        dialog_layer(
            &c,
            dialog_card(
                480.,
                "Открыть из архива",
                self.dialog_close("open-x", cx),
                body,
                footer,
                &c,
            ),
        )
        .into_any_element()
    }

    /// «Контрольная сумма» как в 7-Zip: список файлов сверху, размер и суммы
    /// выбранного файла снизу. «Копировать» кладёт в буфер весь отчёт.
    pub(crate) fn hash_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(Modal::Hash(dialog)) = &self.modal else {
            return div().into_any_element();
        };

        let c = Colors::new(cx.theme());
        let report = &dialog.report;
        let has_totals = report.files.len() > 1 && !report.totals.is_empty();

        // Список виртуализован: на архиве с тысячами файлов строить все строки
        // каждый кадр нельзя — как и в основном списке, только видимый диапазон.
        let files = report.files.clone();
        let selected = dialog.selected;
        let scroll = dialog.scroll.clone();
        let count = files.len() + usize::from(has_totals);
        let weak = cx.entity().downgrade();
        let list = uniform_list("hash-files", count, move |range, _window, _cx| {
            let mut items: Vec<AnyElement> = Vec::with_capacity(range.len());
            for index in range {
                let name = if index < files.len() {
                    files[index].name.clone()
                } else {
                    "Все данные".to_string()
                };
                items.push(
                    Self::hash_file_row(index, name, selected == index, &c, weak.clone())
                        .into_any_element(),
                );
            }
            items
        })
        .h(px(150.))
        .track_scroll(&scroll);

        let detail_row = |label: String, value: String| {
            h_flex()
                .gap_2()
                .child(
                    div()
                        .w(px(80.))
                        .flex_none()
                        .text_sm()
                        .text_color(c.muted)
                        .child(label),
                )
                .child(div().text_sm().whitespace_nowrap().child(value))
        };

        let mut details = v_flex().gap_1();
        if let Some(file) = report.files.get(dialog.selected) {
            details = details.child(detail_row(
                "Размер".to_string(),
                format!("{} байтов : {}", file.size, human_size(file.size)),
            ));
            for (algo, hash) in &file.hashes {
                details = details.child(detail_row(algo.clone(), hash.clone()));
            }
        } else {
            for (algo, hash) in &report.totals {
                details = details.child(detail_row(algo.clone(), hash.clone()));
            }
        }

        // SHA-512 длиннее карточки, поэтому у деталей своя прокрутка по обеим
        // осям; список файлов прокручивается отдельно.
        let body = v_flex()
            .px_4()
            .py_3()
            .gap_2()
            .child(
                v_flex()
                    .border_1()
                    .border_color(c.border)
                    .rounded_sm()
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .text_sm()
                            .bg(c.header())
                            .border_b_1()
                            .border_color(c.border)
                            .child("Имя"),
                    )
                    .child(list),
            )
            .child(
                div()
                    .id("hash-details")
                    .max_h(px(300.))
                    .overflow_scroll()
                    .child(details),
            );

        let footer = h_flex()
            .items_center()
            .justify_between()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(c.border)
            .child(
                dialog_button("hash-copy", "Копировать").on_click(cx.listener(
                    |this, _event, _window, cx| {
                        if let Some(Modal::Hash(dialog)) = &this.modal {
                            cx.write_to_clipboard(ClipboardItem::new_string(
                                dialog.report.to_text(),
                            ));
                        }
                        this.notice = Some("Суммы скопированы в буфер обмена".to_string());
                        cx.notify();
                    },
                )),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(dialog_button("hash-ok", "OK").on_click(cx.listener(
                        |this, _event, _window, cx| {
                            this.close_modal();
                            cx.notify();
                        },
                    )))
                    .child(dialog_button("hash-cancel", "Отмена").on_click(cx.listener(
                        |this, _event, _window, cx| {
                            this.close_modal();
                            cx.notify();
                        },
                    ))),
            );

        dialog_layer(
            &c,
            dialog_card(
                720.,
                "Контрольная сумма",
                self.dialog_close("hash-x", cx),
                body,
                footer,
                &c,
            ),
        )
        .into_any_element()
    }

    /// Строка списка файлов в отчёте сумм: клик показывает суммы этого файла.
    /// Строка списка файлов CRC. Фиксированная высота — требование
    /// виртуализации; клик через WeakEntity, как в строках основного списка.
    fn hash_file_row(
        index: usize,
        name: String,
        selected: bool,
        c: &Colors,
        weak: WeakEntity<MainWindow>,
    ) -> impl IntoElement {
        let hover = c.hover();
        let selected_bg = c.selected();
        div()
            .id(("hash-file", index))
            .h(px(theme::row_h()))
            .flex()
            .items_center()
            .px_2()
            .text_sm()
            .whitespace_nowrap()
            .cursor_pointer()
            .when(selected, |row| row.bg(selected_bg))
            .when(!selected, |row| row.hover(move |style| style.bg(hover)))
            .on_click(move |_event, _window, cx| {
                let _ = weak.update(cx, |this, cx| {
                    if let Some(Modal::Hash(dialog)) = &mut this.modal {
                        dialog.selected = index;
                    }
                    cx.notify();
                });
            })
            .child(name)
    }

    /// Однострочный диалог: имя папки/файла или новое имя.
    pub(crate) fn prompt_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(Modal::Prompt(dialog)) = &self.modal else {
            return div().into_any_element();
        };
        let c = Colors::new(cx.theme());

        let body = v_flex()
            .px_4()
            .py_3()
            .gap_2()
            .child(self.dialog_row(dialog.action.label(), control_input(&dialog.input).w_full()))
            .children(dialog.error.clone().map(|text| {
                div()
                    .text_sm()
                    .text_color(c.error())
                    .whitespace_normal()
                    .child(text)
            }));

        let footer = h_flex()
            .justify_end()
            .gap_2()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(c.border)
            .child(
                dialog_button("prompt-ok", dialog.action.button()).on_click(cx.listener(
                    |this, _event, _window, cx| {
                        this.submit_prompt(cx);
                        cx.notify();
                    },
                )),
            )
            .child(
                dialog_button("prompt-cancel", "Отмена").on_click(cx.listener(
                    |this, _event, window, cx| {
                        this.close_modal();
                        window.focus(&this.focus, cx);
                        cx.notify();
                    },
                )),
            );

        dialog_layer(
            &c,
            dialog_card(
                440.,
                dialog.action.title(),
                self.dialog_close("prompt-x", cx),
                body,
                footer,
                &c,
            ),
        )
        .into_any_element()
    }

    /// Диалог «Копировать в…» / «Переместить в…».
    pub(crate) fn transfer_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(Modal::Transfer(dialog)) = &self.modal else {
            return div().into_any_element();
        };
        let c = Colors::new(cx.theme());
        let overwrite = dialog.overwrite;

        let modes = RadioGroup::horizontal("transfer-mode")
            .selected_index(Some(overwrite_index(overwrite)))
            .on_change(cx.listener(|this, index: &usize, _window, cx| {
                if let Some(Modal::Transfer(dialog)) = &mut this.modal {
                    dialog.overwrite = OVERWRITE_MODES[(*index).min(OVERWRITE_MODES.len() - 1)].0;
                }
                cx.notify();
            }))
            .children(OVERWRITE_MODES.into_iter().map(|(mode, caption)| {
                Radio::new(("transfer-mode", overwrite_index(mode)))
                    .label(SharedString::from(caption))
                    .control_size()
            }));

        let first = dialog
            .sources
            .first()
            .map(|path| path.display().to_string())
            .unwrap_or_default();

        let body = v_flex()
            .px_4()
            .py_3()
            .gap_2()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .text_color(c.muted)
                            .child(format!("Объектов: {}", dialog.sources.len())),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(c.muted)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(first),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(div().w(px(DIALOG_LABEL_W)).text_sm().child("Куда:"))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .child(control_input(&dialog.destination).w_full()),
                            )
                            .child(dialog_button("transfer-pick", "…").on_click(cx.listener(
                                |this, _event, window, cx| {
                                    this.open_picker(PickerTarget::Transfer, window, cx);
                                    cx.notify();
                                },
                            ))),
                    ),
            )
            .children(dialog.error.clone().map(|text| {
                div()
                    .text_sm()
                    .text_color(c.error())
                    .whitespace_normal()
                    .child(text)
            }))
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(DIALOG_LABEL_W))
                            .text_sm()
                            .child("При совпадении:"),
                    )
                    .child(modes),
            );

        let footer = h_flex()
            .justify_end()
            .gap_2()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(c.border)
            .child(
                dialog_button("transfer-ok", dialog.kind.button()).on_click(cx.listener(
                    |this, _event, window, cx| {
                        this.submit_transfer(cx);
                        if this.modal.is_none() {
                            window.focus(&this.focus, cx);
                        }
                        cx.notify();
                    },
                )),
            )
            .child(
                dialog_button("transfer-cancel", "Отмена").on_click(cx.listener(
                    |this, _event, window, cx| {
                        this.close_modal();
                        window.focus(&this.focus, cx);
                        cx.notify();
                    },
                )),
            );

        dialog_layer(
            &c,
            dialog_card(
                640.,
                dialog.kind.title(),
                self.dialog_close("transfer-x", cx),
                body,
                footer,
                &c,
            ),
        )
        .into_any_element()
    }
}

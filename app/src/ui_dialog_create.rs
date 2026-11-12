use crate::prelude::*;
impl MainWindow {
    pub(crate) fn dialog_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(Modal::Create(dialog)) = &self.modal else {
            return div().into_any_element();
        };

        let c = Colors::new(cx.theme());
        let solid = dialog.solid;
        let encrypt_names = dialog.encrypt_names;
        let show_password = dialog.show_password;
        let full_paths = dialog.full_paths;
        let compress_open = dialog.compress_open;
        let level = dialog.level;
        let method = dialog.method;

        const LEVELS: [Level; 6] = [
            Level::Store,
            Level::Fastest,
            Level::Fast,
            Level::Normal,
            Level::Maximum,
            Level::Ultra,
        ];
        const METHODS: [Method; 6] = [
            Method::Lzma2,
            Method::Lzma,
            Method::Ppmd,
            Method::BZip2,
            Method::Deflate,
            Method::Store,
        ];

        let level_index = LEVELS.iter().position(|v| *v == level).unwrap_or(3);
        let levels = RadioGroup::horizontal("dlg-level")
            .selected_index(Some(level_index))
            .on_change(cx.listener(|this, index: &usize, _window, cx| {
                if let Some(Modal::Create(dialog)) = &mut this.modal {
                    dialog.level = LEVELS[(*index).min(LEVELS.len() - 1)];
                }
                cx.notify();
            }))
            .children(LEVELS.into_iter().map(|variant| {
                Radio::new(("dlg-level", variant.value() as usize))
                    .label(SharedString::from(variant.value().to_string()))
                    .control_size()
            }));

        let method_index = METHODS.iter().position(|v| *v == method).unwrap_or(0);
        let methods = RadioGroup::horizontal("dlg-method")
            .selected_index(Some(method_index))
            .on_change(cx.listener(|this, index: &usize, _window, cx| {
                if let Some(Modal::Create(dialog)) = &mut this.modal {
                    dialog.method = METHODS[(*index).min(METHODS.len() - 1)];
                }
                cx.notify();
            }))
            .children(METHODS.into_iter().enumerate().map(|(index, variant)| {
                Radio::new(("dlg-method", index))
                    .label(SharedString::from(variant.label_ru()))
                    .control_size()
            }));

        let input = |state: &Entity<InputState>| control_input(state).w_full();

        // Две колонки, как в 7-Zip: слева — что и как сжимаем, справа — пароль
        // и опции. Уровень и метод идут во всю ширину: шесть плашек в колонку
        // не влезают и читаются хуже.
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
                            .child(dialog.directory.display().to_string()),
                    )
                    .child(self.dialog_row("Имя архива:", input(&dialog.name))),
            )
            // Ошибка — в диалоге, а не только в статус-баре: «имя занято»
            // внизу окна легко не заметить (жалоба по «Создать папку»).
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
                            .child("Уровень сжатия:"),
                    )
                    .child(levels)
                    .child(div().text_sm().text_color(c.muted).child(level.label_ru())),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(DIALOG_LABEL_W)).text_sm().child("Метод сжатия:"))
                    .child(methods)
                    .child(div().text_sm().text_color(c.muted).child(method.id())),
            )
            .child(
                h_flex()
                    .items_start()
                    .gap_6()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_2()
                            .child(self.dialog_row("Словарь:", input(&dialog.dictionary)))
                            .child(
                                self.dialog_row(
                                    "Число потоков:",
                                    NumberInput::new(&dialog.threads)
                                        .placeholder("все ядра")
                                        .control_size()
                                        .w_full(),
                                ),
                            )
                            .child(self.dialog_row("Разбить на тома:", input(&dialog.volume)))
                            .child(
                                Checkbox::new("dlg-solid")
                                    .control_size()
                                    .label("Solid-блок")
                                    .checked(solid)
                                    .on_change(cx.listener(|this, checked: &bool, _window, cx| {
                                        if let Some(Modal::Create(dialog)) = &mut this.modal {
                                            dialog.solid = *checked;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(Self::soon_row("Формат архива: только 7z", &c))
                            .child(Self::soon_row("Размер слова и блока", &c))
                            .child(Self::impossible_row("Память упаковки и распаковки", &c)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_2()
                            .child(self.dialog_row("Пароль:", input(&dialog.password)))
                            .child(self.dialog_row("Повтор:", input(&dialog.repeat)))
                            .child(
                                Checkbox::new("dlg-show-pass")
                                    .control_size()
                                    .label("Показать пароль")
                                    .checked(show_password)
                                    .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                        if let Some(Modal::Create(dialog)) = &mut this.modal {
                                            dialog.show_password = *checked;
                                            // Маску переключаем у обоих полей:
                                            // пароль и его повтор должны
                                            // выглядеть одинаково.
                                            let masked = !*checked;
                                            dialog.password.update(cx, |state, cx| {
                                                state.set_masked(masked, window, cx)
                                            });
                                            dialog.repeat.update(cx, |state, cx| {
                                                state.set_masked(masked, window, cx)
                                            });
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Checkbox::new("dlg-enc")
                                    .control_size()
                                    .label("Шифровать имена")
                                    .checked(encrypt_names)
                                    .on_change(cx.listener(|this, checked: &bool, _window, cx| {
                                        if let Some(Modal::Create(dialog)) = &mut this.modal {
                                            dialog.encrypt_names = *checked;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(div().text_sm().text_color(c.muted).child(
                                if dialog.password.read(cx).value().is_empty() {
                                    "Шифрование выключено: пароль не задан"
                                } else {
                                    "Метод шифрования: AES-256"
                                },
                            ))
                            .child(
                                Checkbox::new("dlg-full-paths")
                                    .control_size()
                                    .label("Полные пути")
                                    .checked(full_paths)
                                    .on_change(cx.listener(|this, checked: &bool, _window, cx| {
                                        if let Some(Modal::Create(dialog)) = &mut this.modal {
                                            dialog.full_paths = *checked;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Checkbox::new("dlg-compress-open")
                                    .control_size()
                                    .label("Сжимать открытые файлы")
                                    .checked(compress_open)
                                    .on_change(cx.listener(|this, checked: &bool, _window, cx| {
                                        if let Some(Modal::Create(dialog)) = &mut this.modal {
                                            dialog.compress_open = *checked;
                                        }
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            // Полный набор 7-Zip: чего у нас нет — стоит погашенным с «✕»,
            // чтобы «нет» не путалось с «забыли нарисовать».
            .child(
                GroupBox::new()
                    .id("dlg-soon")
                    .outline()
                    .mt_1()
                    .title(
                        div()
                            .text_sm()
                            .text_color(c.muted)
                            .child("Недоступные возможности"),
                    )
                    .child(
                        h_flex()
                            .items_start()
                            .gap_6()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .gap_1()
                                    .child(Self::soon_row("Режим изменения", &c))
                                    .child(Self::soon_row("Параметры командной строки", &c)),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .gap_1()
                                    .child(Self::impossible_row("SFX — модуль не поставляется", &c))
                                    .child(Self::soon_row("Удалять файлы после сжатия", &c)),
                            ),
                    ),
            );

        let footer = h_flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(c.border)
            .child(Self::soon_button("✕ Настройки…"))
            .child(Self::soon_button("✕ Помощь"))
            .child(div().flex_1())
            .child(dialog_button("dlg-create", "Создать").on_click(cx.listener(
                |this, _event, window, cx| {
                    this.submit_create(cx);
                    if this.modal.is_none() {
                        window.focus(&this.focus, cx);
                    }
                    cx.notify();
                },
            )))
            .child(dialog_button("dlg-cancel", "Отмена").on_click(cx.listener(
                |this, _event, window, cx| {
                    this.close_modal();
                    window.focus(&this.focus, cx);
                    cx.notify();
                },
            )));

        dialog_layer(
            &c,
            dialog_card(
                760.,
                "Добавить к архиву",
                self.dialog_close("create-x", cx),
                body,
                footer,
                &c,
            ),
        )
        .into_any_element()
    }
}

use crate::prelude::*;

pub(crate) struct ToolButton {
    pub(crate) letter: &'static str,
    pub(crate) label: &'static str,
    pub(crate) action: ToolAction,
    pub(crate) color: ToolColor,
    /// Горячая клавиша для подсказки; пусто — клавиши у действия нет.
    pub(crate) hint: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ToolColor {
    Green,
    Blue,
    Cyan,
    Red,
    Yellow,
}

impl ToolColor {
    pub(crate) fn pick(self, c: &Colors) -> Hsla {
        match self {
            Self::Green => c.green,
            Self::Blue => c.blue,
            Self::Cyan => c.cyan,
            Self::Red => c.red,
            Self::Yellow => c.yellow,
        }
    }
}

const TOOLBAR: [ToolButton; 7] = [
    ToolButton {
        letter: "A",
        label: "Добавить",
        action: ToolAction::Add,
        color: ToolColor::Green,
        hint: "",
    },
    ToolButton {
        letter: "E",
        label: "Извлечь",
        action: ToolAction::Extract,
        color: ToolColor::Blue,
        hint: "",
    },
    ToolButton {
        letter: "T",
        label: "Тестировать",
        action: ToolAction::Test,
        color: ToolColor::Cyan,
        hint: "",
    },
    ToolButton {
        letter: "C",
        label: "Копировать",
        action: ToolAction::Copy,
        color: ToolColor::Blue,
        hint: "F5",
    },
    ToolButton {
        letter: "M",
        label: "Переместить",
        action: ToolAction::Move,
        color: ToolColor::Blue,
        hint: "F6",
    },
    ToolButton {
        letter: "D",
        label: "Удалить",
        action: ToolAction::Delete,
        color: ToolColor::Red,
        hint: "Del",
    },
    ToolButton {
        letter: "I",
        label: "Информация",
        action: ToolAction::Info,
        color: ToolColor::Yellow,
        hint: "Alt+Enter",
    },
];

impl MainWindow {
    pub(crate) fn toolbar(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let c = Colors::new(cx.theme());
        let running = self.job.as_ref().map(|job| job.kind.tool_action());
        let mut bar = Toolbar::new("main-toolbar")
            .px_2()
            .py_0p5()
            .gap_0p5()
            .border_b_1()
            .border_color(c.border);
        for (index, button) in TOOLBAR.into_iter().enumerate() {
            let action = button.action;
            let cancel_slot = Some(action) == running;
            let enabled = !self.modal_open()
                && !window.has_active_dialog(cx)
                && if running.is_some() {
                    cancel_slot
                } else {
                    self.tool_enabled(action)
                };
            let (letter, color, label, hint) = if cancel_slot {
                ("X", c.red, "Отмена", "")
            } else {
                (
                    button.letter,
                    button.color.pick(&c),
                    button.label,
                    button.hint,
                )
            };
            let tooltip = if hint.is_empty() {
                label.to_string()
            } else {
                format!("{label} ({hint})")
            };
            let item = Button::new(("tool", index))
                .ghost()
                .disabled(!enabled)
                .accessibility_label(label)
                .tooltip(tooltip)
                .h_auto()
                .px_1()
                .py_0p5()
                .child(
                    v_flex()
                        .items_center()
                        .gap_0p5()
                        .child(
                            div()
                                .flex_none()
                                .flex()
                                .min_w(px(theme::font_size() * 1.5))
                                .items_center()
                                .justify_center()
                                .text_size(px(theme::font_size() * 1.35))
                                .font_weight(FontWeight::LIGHT)
                                .text_color(color)
                                .child(letter),
                        )
                        .child(div().text_sm().whitespace_nowrap().child(label)),
                )
                .on_click(cx.listener(move |this, _event, window, cx| {
                    this.address_edit = None;
                    window.focus(&this.focus, cx);
                    if cancel_slot {
                        this.cancel_job();
                    } else {
                        this.run_tool_action(action, window, cx);
                    }
                    cx.notify();
                }));
            bar = bar.content(item);
        }
        bar
    }

    pub(crate) fn panel_view(&self, panel_index: usize, cx: &mut Context<Self>) -> AnyElement {
        let header = (self.panel(panel_index).view_mode == ViewMode::Details)
            .then(|| self.header_row(panel_index, cx));
        v_flex()
            .flex_1()
            .h_full()
            .min_w(px(0.))
            .min_h(px(0.))
            .child(self.panel_address_bar(panel_index, cx))
            .children(header)
            .child(self.list(panel_index, cx))
            .into_any_element()
    }

    pub(crate) fn panels_view(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.two_panel {
            return self.panel_view(0, cx);
        }
        let c = Colors::new(cx.theme());
        h_flex()
            .flex_1()
            .min_h(px(0.))
            .items_stretch()
            .child(self.panel_view(0, cx))
            .child(div().w(px(1.)).h_full().flex_none().bg(c.border))
            .child(self.panel_view(1, cx))
            .into_any_element()
    }

    pub(crate) fn panel_address_bar(
        &self,
        panel_index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let c = Colors::new(cx.theme());
        let active = !self.two_panel || self.active_panel == panel_index;
        let blocked = self.modal_open() || self.job.as_ref().is_some_and(|job| !job.hidden);
        let edit = self
            .address_edit
            .as_ref()
            .filter(|edit| edit.panel == panel_index);
        let error = edit.and_then(|edit| edit.error.clone());
        let address = self.panel(panel_index).address();
        let field = if let Some(edit) = edit {
            control_input(&edit.input)
                .w_full()
                .h_full()
                // Своя рамка у контейнера уже есть. Без этого кит рисует поверх
                // неё свою рамку и на фокусе — толстое кольцо.
                .bordered(false)
                .into_any_element()
        } else {
            Button::new(("address-edit", panel_index))
                .accessibility_label(address.clone())
                .tooltip(format!("{address}\nИзменить адрес (Ctrl+L)"))
                .ghost()
                .control_size()
                .text_sm()
                .w_full()
                // Та же высота, что у поля редактирования: строка адреса не
                // должна прыгать при входе в Ctrl+L/клик.
                .h_full()
                .justify_start()
                .disabled(blocked)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.edit_address(panel_index, window, cx)
                }))
                // Подпись кнопки кит центрирует; кладём её ребёнком во всю
                // ширину, чтобы путь стоял по левому краю, как в 7-Zip FM.
                .child(
                    div()
                        .w_full()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_left()
                        .child(address.clone()),
                )
                .into_any_element()
        };
        // Автодополнение — бледный хвост прямо в поле, как в терминале.
        // Невидимая копия набранного текста занимает ровно его ширину, поэтому
        // хвост начинается сразу после курсора без измерения глифов. Если текст
        // длиннее поля, Input прокручен — хвост не рисуем, иначе он встал бы
        // в начало строки.
        let ghost = edit.and_then(|edit| {
            let state = edit.input.read(cx);
            let value = state.value();
            let at_end = state.selected_range().is_empty()
                && state.cursor_position().character as usize == value.len()
                && f32::from(state.scroll_offset().x).abs() < 0.5;
            at_end
                .then(|| completion_ghost(&value, &edit.suggestions))
                .flatten()
                .map(|suffix| (value, suffix))
        });
        let ghost = ghost.map(|(value, suffix)| {
            h_flex()
                .absolute()
                // input_px(Size::Small) у кита — 8 px; отступ берём оттуда,
                // чтобы хвост начинался ровно там же, где текст Input.
                .left(px(8.))
                .right_0()
                .top_0()
                .h_full()
                .items_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(px(theme::font_size()))
                .font_family(cx.theme().font_family.clone())
                .child(div().flex_none().opacity(0.).child(value))
                .child(
                    div()
                        .flex_none()
                        .text_color(c.muted)
                        .opacity(0.8)
                        .child(suffix),
                )
        });
        v_flex()
            .border_b_1()
            .border_color(c.border)
            .bg(if active && self.two_panel {
                c.blue.opacity(0.08)
            } else {
                c.bg
            })
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_0p5()
                    .child(div().w(px(3.)).h_5().flex_none().bg(if active {
                        c.blue
                    } else {
                        c.border
                    }))
                    .child(
                        Button::new(("go-up", panel_index))
                            .icon(IconName::ArrowUp)
                            .tooltip("На уровень вверх (Backspace)")
                            .ghost()
                            .control_size()
                            .text_sm()
                            .disabled(blocked)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.activate_panel(panel_index);
                                this.address_edit = None;
                                this.go_up(panel_index, cx);
                                window.focus(&this.focus, cx);
                                this.sync_title(window);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(("go-root", panel_index))
                            .label("\\")
                            .tooltip("В корень (\\)")
                            .ghost()
                            .control_size()
                            .text_sm()
                            .disabled(blocked)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.activate_panel(panel_index);
                                this.address_edit = None;
                                this.run_action(MenuAction::GoRoot, window, cx);
                                window.focus(&this.focus, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            // Фиксированная высота контрола (китовый h_6):
                            // и кнопка-адрес, и поле редактирования заполняют
                            // её целиком — режимы не меняют высоту строки.
                            .h_6()
                            .border_1()
                            .border_color(if active { c.blue } else { c.border })
                            .rounded_sm()
                            .overflow_hidden()
                            // Пока адрес правится, стрелки и Tab принадлежат подсказкам.
                            .when(edit.is_some(), |this| {
                                this.key_context("AddressSuggestions")
                            })
                            .child(field)
                            .children(ghost),
                    ),
            )
            .children(error.map(|error| div().px_2().pb_1().text_color(c.error()).child(error)))
    }

    pub(crate) fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let c = Colors::new(cx.theme());
        let panel = self.active();
        let total = panel.entries.len();
        let selected = panel.selection.len();
        let size = human(panel.total_size());
        let ms = if panel.load_ms < 1 {
            "<1".to_string()
        } else {
            panel.load_ms.to_string()
        };
        let what = plural_objects(total);
        let middle = if let Some(job) = &self.job {
            if job.hidden {
                format!("{} · Подробнее…", job.text())
            } else {
                job.text()
            }
        } else if let Some(notice) = &self.notice {
            notice.clone()
        } else {
            match (&panel.error, panel.is_archive) {
                (Some(_), _) => "Ошибка".to_string(),
                (None, true) => format!(
                    "{} · {total} {what} · {size} · {ms} мс",
                    panel.kind.as_deref().unwrap_or("архив")
                ),
                (None, false) => format!("{total} {what} · {size} · {ms} мс"),
            }
        };
        let left = if self.two_panel {
            format!(
                "Панель {} · выделено объектов: {selected} / {total}",
                self.active_panel + 1
            )
        } else {
            format!("Выделено объектов: {selected} / {total}")
        };
        let actionable = !self.modal_open() && (self.job.is_some() || self.notice.is_some());
        StatusBar::new()
            .h(px(theme::status_h()))
            .text_sm()
            .border_color(c.border)
            .left(left)
            .child(
                Button::new("operation-details")
                    .ghost()
                    .control_size()
                    .text_sm()
                    .min_w(px(0.))
                    .label(middle.lines().next().unwrap_or_default().to_string())
                    .tooltip(middle)
                    .disabled(!actionable)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.address_edit = None;
                        window.focus(&this.focus, cx);
                        if let Some(job) = &mut this.job {
                            job.hidden = false;
                        } else if let Some(notice) = &this.notice {
                            this.modal = Some(Modal::Notice(notice.clone()));
                        }
                        cx.notify();
                    })),
            )
            .right(
                div()
                    .text_color(c.muted)
                    .whitespace_nowrap()
                    .child(self.meter.status_text()),
            )
    }
}

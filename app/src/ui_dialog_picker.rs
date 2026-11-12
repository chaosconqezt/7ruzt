use crate::prelude::*;
impl MainWindow {
    /// Дерево выбора папки — как «Обзор папок» в 7-Zip.
    pub(crate) fn folder_picker_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(picker) = &self.folder_picker else {
            return div().into_any_element();
        };
        let c = Colors::new(cx.theme());

        // Строку рисуем сами (отступ, стрелка, подпись), всё остальное —
        // выделение, раскрытие, прокрутка, клик — делает `Tree` из кита.
        let tree_view = tree(
            &picker.tree,
            move |_index, entry, _selected, _window, cx| {
                let c = Colors::new(cx.theme());
                let arrow = if entry.is_folder() {
                    if entry.is_expanded() { "▾" } else { "▸" }
                } else {
                    " "
                };
                ListItem::new(entry.item().id.clone()).child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .pl(px(6. + entry.depth() as f32 * 14.))
                        .child(div().w_4().flex_none().text_color(c.muted).child(arrow))
                        .child(
                            div()
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .child(entry.item().label.clone()),
                        ),
                )
            },
        )
        .h(px(320.))
        .w_full();

        let body = v_flex()
            .px_4()
            .py_3()
            .gap_2()
            .child(div().text_sm().text_color(c.muted).child("Укажите папку:"))
            .child(
                div()
                    .w_full()
                    .border_1()
                    .border_color(c.border)
                    .rounded_sm()
                    .overflow_hidden()
                    .child(tree_view),
            );

        let selected_text = picker
            .tree
            .read(cx)
            .selected_item()
            .map(|item| item.id.to_string())
            .unwrap_or_default();

        let footer = h_flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(c.border)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_sm()
                    .text_color(c.muted)
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(selected_text),
            )
            .child(dialog_button("picker-ok", "ОК").on_click(cx.listener(
                |this, _event, window, cx| {
                    this.picker_ok(window, cx);
                    cx.notify();
                },
            )))
            .child(
                dialog_button("picker-cancel", "Отмена").on_click(cx.listener(
                    |this, _event, _window, cx| {
                        this.folder_picker = None;
                        cx.notify();
                    },
                )),
            );

        dialog_layer(
            &c,
            dialog_card(
                560.,
                "Выбор папки",
                // Свой крестик: закрывать дерево, не закрывая диалог под ним.
                close_button("picker-x").on_click(cx.listener(|this, _event, _window, cx| {
                    this.folder_picker = None;
                    cx.notify();
                })),
                body,
                footer,
                &c,
            ),
        )
        .into_any_element()
    }
}

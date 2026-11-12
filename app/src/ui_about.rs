use crate::prelude::*;
impl MainWindow {
    /// «О программе» — как в макете: логотип 7-Zip и версии.
    pub(crate) fn about_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        if !matches!(self.modal, Some(Modal::About)) {
            return div().into_any_element();
        }

        let c = Colors::new(cx.theme());
        let body = v_flex()
            .px_4()
            .py_3()
            .gap_1()
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .bg(c.black)
                            .text_color(c.white)
                            .px_2()
                            .py_1()
                            .text_size(px(theme::font_size() * 1.7))
                            .child("7"),
                    )
                    .child(
                        div()
                            .border_2()
                            .border_color(c.black)
                            .px_2()
                            .py_1()
                            .text_size(px(theme::font_size() * 1.7))
                            .child("ZIP"),
                    ),
            )
            .child(div().text_sm().child("7-Zip 26.06 (x64)"))
            .child(div().text_sm().text_color(c.muted).child("2026-11-06"))
            .child("Copyright (c) 1999-2026 Igor Pavlov")
            .child(
                div()
                    .text_sm()
                    .text_color(c.muted)
                    .child("7-Zip является свободно распространяемой программой."),
            )
            .child(
                v_flex()
                    .mt_2()
                    .gap_1()
                    .text_sm()
                    .text_color(c.muted)
                    .child("Оболочка: archiver-app 0.1 · ядро archiver-core")
                    .child("UI: gpui-kit 0.9.1"),
            );

        let footer = h_flex()
            .items_center()
            .justify_end()
            .gap_2()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(c.border)
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(c.muted)
                    .child("www.7-zip.org"),
            )
            .child(dialog_button("about-ok", "ОК").on_click(cx.listener(
                |this, _event, _window, cx| {
                    this.close_modal();
                    cx.notify();
                },
            )));

        dialog_layer(
            &c,
            dialog_card(
                420.,
                "О программе 7-Zip",
                self.dialog_close("about-x", cx),
                body,
                footer,
                &c,
            ),
        )
        .into_any_element()
    }
}

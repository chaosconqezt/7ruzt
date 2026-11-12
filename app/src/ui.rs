use crate::prelude::*;
impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = Colors::new(cx.theme());
        // Ширину окна запоминаем на кадр: от неё зависит набор колонок в панели.
        self.window_width = f32::from(window.bounds().size.width);
        // Время сборки дерева: именно оно показывает, дорого ли нам даётся кадр.
        let (render_started, spin) = self.meter.begin();
        let kit_dialog = window.has_active_dialog(cx);
        let modal_open = self.modal_open() || kit_dialog;
        if modal_open {
            self.ui_log("render begin");
        }
        if spin {
            window.request_animation_frame();
        }

        let mut root = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(c.bg)
            .text_color(c.fg)
            // Один кегль и одна гарнитура на всё окно: дальше никто не меняет
            // размер текста (кроме плашек-иконок и логотипа в «О программе»).
            .text_sm()
            // Клавиатуру слушаем на корне: под модалкой список не рисуется,
            // и вместе с ним пропал бы весь ввод в диалог.
            .track_focus(&self.focus)
            // Tab в gpui-base занят переходом по фокусу (`focus_next`), поэтому
            // обычный `on_key_down` её не видит. Свой контекст и биндинг в
            // `main` перекрывают встроенный, а в диалогах мы сами зовём
            // `focus_next`, чтобы Tab продолжал ходить по полям.
            .key_context("Archiver")
            .on_action(cx.listener(|this, _: &window::Complete, window, cx| {
                // Tab в адресе: дополнить бледный хвост; без хвоста — закрыть
                // редактор, как раньше (Tab по панелям остаётся в контексте окна).
                this.complete_address(window, cx);
            }))
            .on_action(cx.listener(|this, _: &SwitchPanel, window, cx| {
                if this.modal_open() || window.has_active_dialog(cx) {
                    if window.modifiers().shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    return;
                }
                if this.address_edit.is_some() {
                    this.address_edit = None;
                    window.focus(&this.focus, cx);
                    cx.notify();
                    return;
                }
                if this.rename.is_some() {
                    return;
                }
                if this.job.as_ref().is_some_and(|job| !job.hidden) || !this.two_panel {
                    if window.modifiers().shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    return;
                }
                if this.two_panel {
                    this.active_panel = 1 - this.active_panel;
                    this.sync_title(window);
                    cx.notify();
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let now = Instant::now();
                let gap = this
                    .last_key_log
                    .map(|previous| now.duration_since(previous).as_millis())
                    .unwrap_or(0);
                this.last_key_log = Some(now);
                this.meter.key_at = Some(now);
                this.ui_log(&format!(
                    "key=\"{}\" char={:?} gap={gap}ms",
                    event.keystroke.key, event.keystroke.key_char
                ));
                this.handle_key(
                    &event.keystroke.key,
                    event.keystroke.modifiers.shift,
                    event.keystroke.modifiers.control,
                    event.keystroke.modifiers.alt,
                    window,
                    cx,
                );
                // Заголовок окна — тот же путь, что в адресной строке. Вызов
                // кешированный: SetWindowText трогаем только когда путь сменился
                // (Enter в каталог или архив, Backspace, переход по «\»).
                this.sync_title(window);
            }))
            .child(self.menu_bar(window, cx))
            .child(self.toolbar(window, cx))
            .child(if kit_dialog {
                // Под диалогом кита списки не строим: иначе смена фокуса в
                // диалоге перерисовывает тысячи строк.
                div().flex_1().bg(c.bg).into_any_element()
            } else {
                self.panels_view(cx)
            });

        // Рамки протяжки рисуются в корне окна: здесь координаты совпадают
        // с оконными, и они не участвуют в прокрутке списка. Панелей может
        // быть две — у каждой своя рамка.
        let mut rubbers = Vec::new();
        for index in 0..self.panel_count() {
            if let Some((x, y, w, h)) = self.panel(index).rubber_rect() {
                rubbers.push(
                    div()
                        .absolute()
                        .left(px(x))
                        .top(px(y))
                        .w(px(w))
                        .h(px(h))
                        .border_1()
                        .border_color(c.accent)
                        .bg(c.accent.opacity(0.15))
                        .into_any_element(),
                );
            }
        }
        root = root.children(rubbers);

        // Прогресс операции — под модалками: окно итогов проверки ложится
        // поверх оставленного окна прогресса, как в 7-Zip. Статус-бар выше
        // всех: счётчик кадра и итог операции остаются видны.
        if self.job.as_ref().is_some_and(|job| !job.hidden) || self.done_job.is_some() {
            root = root.child(self.progress_overlay(cx));
        }
        if let Some(modal) = &self.modal {
            root = root.child(match modal {
                Modal::Create(_) => self.dialog_overlay(cx),
                Modal::Extract(_) => self.extract_overlay(cx),
                Modal::Test(_) => self.test_overlay(cx),
                Modal::TestResult(_) => self.test_result_overlay(cx),
                Modal::Open(_) => self.open_overlay(cx),
                Modal::Hash(_) => self.hash_overlay(cx),
                Modal::Prompt(_) => self.prompt_overlay(cx),
                Modal::Transfer(_) => self.transfer_overlay(cx),
                Modal::Bench(_) => self.bench_overlay(cx),
                Modal::About => self.about_overlay(cx),
                Modal::Notice(text) => self.notice_overlay(text, cx),
            });
        }
        // Дерево папок — поверх диалога извлечения/переноса, но под статус-баром.
        if self.folder_picker.is_some() {
            root = root.child(self.folder_picker_overlay(cx));
        }
        root = root.child(self.status_bar(cx));
        if modal_open {
            let spent = render_started.elapsed().as_millis();
            self.ui_log(&format!("render end ({spent}ms)"));
        }

        self.meter.finish(render_started);

        root
    }
}

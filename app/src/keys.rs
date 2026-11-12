use crate::prelude::*;
/// Сколько строк пролистывает PageUp/PageDown. Точная высота списка зависит от DPI
/// и содержимого, а для постраничного шага оценки в 20 строк достаточно.
const PAGE_ROWS: usize = 20;

impl MainWindow {
    /// Стрелки и PageUp/PageDown двигают курсор; Shift — растягивает выделение.
    /// Enter — войти, Backspace — наверх, Ctrl+A — выделить всё, Escape — снять.
    pub(crate) fn handle_key(
        &mut self,
        key: &str,
        shift: bool,
        control: bool,
        alt: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Диалог кита (подтверждение удаления) сам владеет клавишами: иначе
        // Escape снял бы выделение под ним, а Enter открыл бы файл.
        if window.has_active_dialog(cx) {
            return;
        }

        if self.address_edit.is_some() {
            if key == "escape" {
                // Сначала закрываем подсказки; повторный Escape — само поле.
                if !self.dismiss_address_suggestions(cx) {
                    self.address_edit = None;
                    window.focus(&self.focus, cx);
                }
                cx.notify();
            }
            return;
        }

        // Дерево папок перекрывает диалог: сначала его клавиши.
        if self.folder_picker.is_some() {
            match key {
                "escape" => {
                    self.folder_picker = None;
                    cx.notify();
                }
                "enter" => {
                    self.picker_ok(window, cx);
                    cx.notify();
                }
                "up" => {
                    self.picker_step(true, cx);
                    cx.notify();
                }
                "down" => {
                    self.picker_step(false, cx);
                    cx.notify();
                }
                _ => {}
            }
            return;
        }
        // В диалоге ввод принадлежит `InputState`; мы отвечаем только за Escape/Enter.
        if self.modal.is_some() {
            match key {
                "escape" => {
                    self.close_modal();
                    window.focus(&self.focus, cx);
                    cx.notify();
                }
                "enter" if self.focus.is_focused(window) => {
                    self.confirm_modal(cx);
                    cx.notify();
                }
                _ => {}
            }
            return;
        }

        // Окно прогресса: список клавишам не отдаём, Escape = «Фон».
        if self.job.as_ref().is_some_and(|job| !job.hidden) {
            if key == "escape" {
                self.job_to_background();
                window.focus(&self.focus, cx);
                cx.notify();
            }
            return;
        }

        // Оставленное окно завершённой проверки: Escape/Enter = «Закрыть».
        if self.done_job.is_some() {
            if matches!(key, "escape" | "enter") {
                self.done_job = None;
                cx.notify();
            }
            return;
        }

        // Переименование в строке: ввод принадлежит полю, мы отвечаем только
        // за Escape и повторный F2 — иначе стрелки уводили бы курсор из поля в список.
        if self.rename.is_some() {
            match key {
                "escape" => {
                    self.rename = None;
                    window.focus(&self.focus, cx);
                    cx.notify();
                }
                "f2" => {
                    self.toggle_rename_extension(cx);
                    cx.notify();
                }
                _ => {}
            }
            return;
        }

        if self.open_menu.is_some() {
            return;
        }
        if !self.focus.is_focused(window)
            && matches!(
                key,
                "enter"
                    | "space"
                    | "up"
                    | "down"
                    | "left"
                    | "right"
                    | "home"
                    | "end"
                    | "pageup"
                    | "pagedown"
            )
        {
            return;
        }

        match key {
            "l" if control => {
                self.edit_address(self.active_panel, window, cx);
                cx.stop_propagation();
                return;
            }
            "\\" => {
                self.run_action(MenuAction::GoRoot, window, cx);
                return;
            }
            // F9 — как в 7-Zip FM: показать/скрыть вторую панель.
            "f9" => {
                self.toggle_two_panels(cx);
                // Tab и F9 не должны заодно переводить фокус по элементам кита.
                cx.stop_propagation();
                cx.notify();
                return;
            }
            // Alt+Up — открыть текущую папку в другой панели.
            "up" if alt => {
                self.mirror_in_other_panel(cx);
                cx.notify();
                return;
            }
            "enter" if alt => {
                self.open_entry_with(ShellVerb::Properties, window, cx);
                cx.notify();
                return;
            }
            "enter" if shift => {
                self.open_entry_with(ShellVerb::Open, window, cx);
                cx.notify();
                return;
            }
            "enter" => {
                self.open_selected(window, cx);
                cx.notify();
                return;
            }
            // Ctrl+F3…F6 — сортировка из меню «Вид». Стоят ДО простых F3…F6,
            // иначе Ctrl+F5 открыл бы «Копировать», а не сортировку по дате.
            "f3" if control => {
                self.sort_by(SortKey::Name);
                cx.notify();
                return;
            }
            "f4" if control => {
                self.sort_by(SortKey::Type);
                cx.notify();
                return;
            }
            "f5" if control => {
                self.sort_by(SortKey::Modified);
                cx.notify();
                return;
            }
            "f6" if control => {
                self.sort_by(SortKey::Size);
                cx.notify();
                return;
            }
            "f3" => {
                self.open_entry_with(ShellVerb::Open, window, cx);
                cx.notify();
                return;
            }
            "f4" => {
                self.open_entry_with(ShellVerb::Edit, window, cx);
                cx.notify();
                return;
            }
            "r" if control => {
                let index = self.active_panel;
                self.reload_panel(index, cx, move |this, _| {
                    this.notice = Some(format!("Обновлено за {} мс", this.panels[index].load_ms));
                });
                return;
            }
            "f2" => {
                self.open_rename(window, cx);
                cx.notify();
                return;
            }
            "f5" => {
                self.open_transfer(TransferKind::Copy, window, cx);
                cx.notify();
                return;
            }
            "f6" => {
                self.open_transfer(TransferKind::Move, window, cx);
                cx.notify();
                return;
            }
            "delete" => {
                self.confirm_delete(window, cx);
                cx.notify();
                return;
            }
            "f7" => {
                self.open_prompt(PromptAction::CreateFolder, window, cx);
                cx.notify();
                return;
            }
            "n" if control => {
                self.open_prompt(PromptAction::CreateFile, window, cx);
                cx.notify();
                return;
            }
            "1" if control => {
                self.active_mut().set_view(ViewMode::LargeIcons);
                cx.notify();
                return;
            }
            "2" if control => {
                self.active_mut().set_view(ViewMode::SmallIcons);
                cx.notify();
                return;
            }
            "3" if control => {
                self.active_mut().set_view(ViewMode::List);
                cx.notify();
                return;
            }
            "4" if control => {
                self.active_mut().set_view(ViewMode::Details);
                cx.notify();
                return;
            }
            // Numpad — классика 7-Zip FM: маска, тип, всё/снять/обратить.
            // GPUI на Windows отдаёт нумпад как "+", "-", "*" (без отдельных
            // имён), поэтому основная «+» из верхнего ряда работает так же.
            "+" if alt => {
                self.select_same_type(false);
                cx.notify();
                return;
            }
            "-" if alt => {
                self.select_same_type(true);
                cx.notify();
                return;
            }
            "+" if shift => {
                self.run_action(MenuAction::SelectAll, window, cx);
                return;
            }
            "-" if shift => {
                self.run_action(MenuAction::ClearSelection, window, cx);
                return;
            }
            "+" => {
                self.open_select_mask(false, window, cx);
                cx.notify();
                return;
            }
            "-" => {
                self.open_select_mask(true, window, cx);
                cx.notify();
                return;
            }
            "*" => {
                self.run_action(MenuAction::InvertSelection, window, cx);
                return;
            }
            "backspace" => {
                self.go_up(self.active_panel, cx);
                cx.notify();
                return;
            }
            "escape" => {
                self.active_mut().clear_selection();
                cx.notify();
                return;
            }
            // Insert — как в 7-Zip: переключить выделение текущей строки.
            "insert" => {
                if let Some(index) = self.active().active {
                    self.active_mut().toggle_selection(index);
                    cx.notify();
                }
                return;
            }
            "a" if control => {
                let panel = self.active_mut();
                panel.selection = Arc::new((0..panel.entries.len()).collect());
                panel.active = panel.entries.len().checked_sub(1);
                panel.anchor = Some(0);
                if let Some(index) = panel.active {
                    panel.scroll_to_entry(index);
                }
                cx.notify();
                return;
            }
            _ => {}
        }

        // Дальше — движение по активной панели. Заём её заканчиваем до
        // `cx.notify`, иначе одновременно не одолжить `self` и панель.
        let moved = {
            let panel = self.active_mut();
            let count = panel.entries.len();
            if count == 0 {
                false
            } else {
                let last = count - 1;
                // В сетке стрелки вверх/вниз двигают по строкам значков, а не по одному.
                let step = panel.grid_columns().max(1);
                let page = PAGE_ROWS * step;

                let next = match key {
                    "up" => Some(panel.active.map_or(0, |index| index.saturating_sub(step))),
                    "down" => Some(panel.active.map_or(0, |index| (index + step).min(last))),
                    "left" => Some(panel.active.map_or(0, |index| index.saturating_sub(1))),
                    "right" => Some(panel.active.map_or(0, |index| (index + 1).min(last))),
                    "home" => Some(0),
                    "end" => Some(last),
                    "pageup" => Some(panel.active.map_or(0, |index| index.saturating_sub(page))),
                    "pagedown" => Some(panel.active.map_or(0, |index| (index + page).min(last))),
                    _ => None,
                };

                if let Some(index) = next {
                    if shift {
                        panel.extend_selection(index);
                    } else if control {
                        // Ctrl+стрелка двигает курсор, не схлопывая выделение — как в проводнике.
                        panel.active = Some(index);
                    } else {
                        panel.select_only(index);
                    }
                    panel.scroll_to_entry(index);
                    true
                } else {
                    false
                }
            }
        };

        if moved {
            cx.notify();
        }
    }
}

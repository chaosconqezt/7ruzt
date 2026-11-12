use crate::prelude::*;
impl MainWindow {
    /// «Создать папку» / «Создать файл» — диалог с одним полем.
    pub(crate) fn open_prompt(
        &mut self,
        action: PromptAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.job.is_some() {
            self.notice = Some("Дождитесь окончания текущей операции".to_string());
            return;
        }
        if self.active().is_archive {
            self.notice =
                Some("Внутри архива операции с файлами недоступны: сначала извлеките".to_string());
            return;
        }

        let input = cx.new(|cx| InputState::new(window, cx));

        cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.submit_prompt(cx);
            }
        })
        .detach();

        let focus = input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);

        self.notice = None;
        self.modal = Some(Modal::Prompt(PromptDialog {
            action,
            input,
            error: None,
        }));
    }

    /// F2: переименование прямо в строке списка.
    ///
    /// Как в проводнике: поле появляется на месте подписи, Enter сохраняет,
    /// Escape отменяет, клик в другое место сохраняет. Отдельного окна нет —
    /// человек видит, что именно правит.
    pub(crate) fn open_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.job.is_some() {
            self.notice = Some("Дождитесь окончания текущей операции".to_string());
            return;
        }
        if self.active().is_archive {
            self.notice =
                Some("Внутри архива операции с файлами недоступны: сначала извлеките".to_string());
            return;
        }

        let panel = self.active_panel;
        let cursor = {
            let panel = self.active();
            panel
                .active
                .or_else(|| panel.selection.iter().next().copied())
                .and_then(|index| {
                    panel
                        .entries
                        .get(index)
                        .map(|entry| (index, entry.is_dir, entry.name.clone()))
                })
        };
        let Some((index, is_dir, name)) = cursor else {
            self.notice = Some("Переименовывать нечего: ничего не выделено".to_string());
            return;
        };

        let input = cx.new(|cx| InputState::new(window, cx).default_value(name.clone()));

        // Как в проводнике: у файла выделяем имя без расширения (чтобы сразу
        // можно было печатать новое), у папки — всё имя. Если точки нет или она
        // в начале (`.gitignore`), выделяем целиком.
        let select = if is_dir {
            name.len()
        } else {
            match name.rfind('.') {
                Some(dot) if dot > 0 => dot,
                _ => name.len(),
            }
        };
        input.update(cx, |state, cx| state.set_selected_range(0..select, cx));

        cx.subscribe(&input, |this, _, event: &InputEvent, cx| match event {
            InputEvent::PressEnter { .. } => this.submit_rename(cx, false),
            InputEvent::Blur => this.submit_rename(cx, true),
            _ => {}
        })
        .detach();

        let focus = input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);

        self.notice = None;
        self.rename = Some(RenameEdit {
            panel,
            index,
            original: name,
            input,
        });
    }

    pub(crate) fn submit_prompt(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::Prompt(mut dialog)) = self.modal.take() else {
            return;
        };

        let name = dialog.input.read(cx).value().trim().to_string();
        // Маска — не имя файла: check_name к ней не применяется.
        if matches!(
            dialog.action,
            PromptAction::SelectMask | PromptAction::DeselectMask
        ) {
            self.submit_select_mask(dialog, &name);
            return;
        }
        if let Err(err) = check_name(&name) {
            dialog.error = Some(err.to_string());
            self.modal = Some(Modal::Prompt(dialog));
            return;
        }

        let path = self.active().path.clone();
        let result = match dialog.action {
            PromptAction::CreateFolder => create_folder(&path, &name).map(|_| "Папка создана"),
            PromptAction::CreateFile => create_empty_file(&path, &name).map(|_| "Файл создан"),
            PromptAction::SelectMask | PromptAction::DeselectMask => {
                unreachable!("маска разобрана выше")
            }
        };

        match result {
            Ok(text) => {
                self.notice = Some(format!("{text}: {name}"));
                let index = self.active_panel;
                self.reload_panel(index, cx, move |this, _| {
                    this.panels[index].select_by_name(&name);
                });
            }
            Err(err) => {
                // Ошибку показываем в самом диалоге: в статус-баре её не видно.
                dialog.error = Some(fs_error(&err));
                self.modal = Some(Modal::Prompt(dialog));
            }
        }
    }

    /// Повторный F2 при переименовании: выделение переключается между именем
    /// без расширения и самим расширением — как в некоторых файловых менеджерах.
    /// У папок и файлов без расширения (включая `.gitignore`) переключать нечего.
    pub(crate) fn toggle_rename_extension(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.rename.as_ref() else {
            return;
        };
        let name = edit.input.read(cx).value();
        let Some(dot) = name
            .rfind('.')
            .filter(|dot| *dot > 0 && *dot + 1 < name.len())
        else {
            return;
        };
        let stem = 0..dot;
        let extension = dot + 1..name.len();
        let current = edit.input.read(cx).selected_range();
        let next = if current == stem { extension } else { stem };
        let input = edit.input.clone();
        input.update(cx, |state, cx| state.set_selected_range(next, cx));
    }

    /// Завершает переименование в строке. Ошибку возвращаем в статус-бар:
    /// встроенного поля для красной строки в строке списка нет.
    ///
    /// `via_blur` — сохранение кликом мимо поля: только такой клик должен
    /// проглотить следующее нажатие (индекс строки относится к прежнему списку).
    /// После Enter глотать нечего, а фокус возвращаем списку.
    pub(crate) fn submit_rename(&mut self, cx: &mut Context<Self>, via_blur: bool) {
        let Some(edit) = self.rename.take() else {
            return;
        };

        let name = edit.input.read(cx).value().trim().to_string();
        if name == edit.original {
            if !via_blur {
                self.focus_list(cx);
            }
            return;
        }
        if let Err(err) = check_name(&name) {
            self.notice = Some(format!("{err}: {name}"));
            // Поле не бросаем: набранное имя должно остаться editable.
            self.rename = Some(edit);
            return;
        }

        let panel = edit.panel;
        let base = self.panels[panel].path.clone();
        match rename_in(&base, &edit.original, &name) {
            Ok(_) => {
                if via_blur {
                    self.rename_finished = Some(panel);
                }
                self.notice = Some(format!("Переименовано: {name}"));
                self.reload_panel(panel, cx, move |this, cx| {
                    this.panels[panel].select_by_name(&name);
                    if !via_blur {
                        this.with_window(cx, |this, window, cx| window.focus(&this.focus, cx));
                    }
                });
            }
            Err(err) => {
                self.notice = Some(format!("Не получилось: {}", fs_error(&err)));
                self.rename = Some(edit);
            }
        }
    }

    /// Фокус списку из контекста без Window (подписка на поле).
    fn focus_list(&self, cx: &mut Context<Self>) {
        self.with_window(cx, |this, window, cx| window.focus(&this.focus, cx));
    }

    /// Del: удаление выделенного — с подтверждением и без корзины.
    ///
    /// Подтверждение — штатный `AlertDialog` кита, а не свой оверлей: так
    /// Escape/Enter и фокус ведёт кит, а модальное состояние окна не нужно.
    pub(crate) fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.job.is_some() {
            self.notice = Some("Дождитесь окончания текущей операции".to_string());
            return;
        }
        if self.active().is_archive {
            self.notice =
                Some("Внутри архива операции с файлами недоступны: сначала извлеките".to_string());
            return;
        }

        let paths: Vec<PathBuf> = {
            let panel = self.active();
            panel
                .selection
                .iter()
                .filter_map(|index| panel.entries.get(*index))
                .map(|entry| panel.path.join(&entry.name))
                .collect()
        };
        if paths.is_empty() {
            self.notice = Some("Ничего не выделено".to_string());
            return;
        }

        let message = if paths.len() == 1 {
            format!(
                "Удалить «{}»?\nОбъект будет удалён безвозвратно: корзина не используется.",
                name_of(&paths[0])
            )
        } else {
            format!(
                "Удалить выделенные объекты ({})?\nБезвозвратно: корзина не используется.",
                paths.len()
            )
        };

        self.notice = None;
        let weak = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _window, _cx| {
            let weak = weak.clone();
            let paths = paths.clone();
            alert
                .title("Удаление")
                .description(message.clone())
                .ok_text("Удалить")
                .cancel_text("Отмена")
                .show_cancel(true)
                .on_ok(move |_, _window, cx| {
                    let weak = weak.clone();
                    let paths = paths.clone();
                    let _ = weak.update(cx, |this, cx| this.delete_now(paths, cx));
                    true
                })
        });
    }

    /// Удаляет то, что подтвердили в диалоге (в фоне, чтобы не морозить окно).
    pub(crate) fn delete_now(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        // Пока идёт удаление, пользователь может переключить панель — reload
        // должен попасть в ту, откуда удаляли, а не в текущую активную.
        let origin = self.active_panel;
        self.notice = Some("Удаление…".to_string());
        cx.spawn(async move |this, cx| {
            let summary = cx
                .background_executor()
                .spawn(async move { delete_paths(&paths) })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.notice = Some(match (summary.deleted, summary.errors.first()) {
                    (0, Some((path, err))) => {
                        format!("Не удалось удалить «{}»: {}", name_of(path), fs_error(err))
                    }
                    (deleted, Some((path, err))) => format!(
                        "Удалено: {deleted}; ошибка на «{}»: {}",
                        name_of(path),
                        fs_error(err)
                    ),
                    (deleted, None) => format!("Удалено: {deleted}"),
                });
                this.reload_panel(origin, cx, |_, _| {});
                cx.notify();
            });
        })
        .detach();
    }
}

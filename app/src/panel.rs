//! Одна панель списка.
//!
//! Панелей в окне две, и они равноправны: в двухпанельном режиме (F9) любая
//! может стать активной (Tab). Поэтому всё, что относится к «где мы и что
//! видим», живёт здесь, а не в [`MainWindow`]: путь, содержимое, выделение,
//! сортировка, вид, прокрутка и рамка. Глобальное (меню, тулбар, диалоги,
//! фоновая операция, статус-бар) остаётся в окне.

use crate::prelude::*;

pub(crate) struct AddressEdit {
    pub(crate) panel: usize,
    pub(crate) input: Entity<InputState>,
    pub(crate) error: Option<String>,
    /// Подсказки автодополнения: пути и признак каталога.
    pub(crate) suggestions: Vec<(PathBuf, bool)>,
    /// Поколение подсказок: устаревший фоновый листинг не применяется.
    pub(crate) suggestions_gen: u64,
}

impl MainWindow {
    pub(crate) fn edit_address(
        &mut self,
        panel: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.modal_open()
            || self.rename.is_some()
            || self.job.as_ref().is_some_and(|job| !job.hidden)
        {
            return;
        }
        self.activate_panel(panel);
        let address = self.panel(panel).address();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(address));
        cx.subscribe_in(
            &input,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if !this
                    .address_edit
                    .as_ref()
                    .is_some_and(|edit| &edit.input == input)
                {
                    return;
                }
                match event {
                    InputEvent::PressEnter { .. } => {
                        // Enter принимает бледный хвост и сразу переходит:
                        // набранный наполовину адрес без этого даёт ошибку.
                        let has_ghost = this.address_edit.as_ref().is_some_and(|edit| {
                            completion_ghost(&edit.input.read(cx).value(), &edit.suggestions)
                                .is_some()
                        });
                        if has_ghost {
                            this.complete_address(window, cx);
                        }
                        this.submit_address(window, cx);
                    }
                    InputEvent::Change => this.refresh_address_suggestions(cx),
                    InputEvent::Blur => {
                        this.address_edit = None;
                        cx.notify();
                    }
                    _ => {}
                }
            },
        )
        .detach();
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.select_all(window, cx);
        });
        self.address_edit = Some(AddressEdit {
            panel,
            input,
            error: None,
            suggestions: Vec::new(),
            suggestions_gen: 0,
        });
        self.refresh_address_suggestions(cx);
        cx.notify();
    }

    /// Пересчитывает подсказки адреса в фоне: read_dir на сетевых путях
    /// не должен морозить окно. Устаревшие результаты отбрасываются.
    pub(crate) fn refresh_address_suggestions(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.address_edit.as_mut() else {
            return;
        };
        edit.suggestions_gen = edit.suggestions_gen.wrapping_add(1);
        let generation = edit.suggestions_gen;
        let input = edit.input.clone();
        let panel = edit.panel;
        let value = input.read(cx).value().to_string();
        let base = {
            let panel = self.panel(panel);
            if panel.is_archive {
                panel.path.parent().unwrap_or(&panel.path).to_path_buf()
            } else {
                panel.path.clone()
            }
        };
        cx.spawn(async move |this, cx| {
            let suggestions = cx
                .background_executor()
                .spawn(async move {
                    let (dir, prefix) = split_address_hint(&base, &value);
                    list_address_suggestions(&dir, &prefix)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(edit) = this.address_edit.as_mut() else {
                    return;
                };
                if edit.suggestions_gen != generation || edit.input != input {
                    return;
                }
                edit.suggestions = suggestions;
                cx.notify();
            });
        })
        .detach();
    }

    /// Tab: дополнить набранный адрес бледным хвостом.
    ///
    /// Если подсказок нет — закрываем поле, как раньше. Если есть общий хвост,
    /// он и подставляется; когда совпадения расходятся, Tab берёт первый
    /// вариант. Поле работает как терминал: Tab — дополнить, Enter — перейти.
    pub(crate) fn complete_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.address_edit.as_ref() else {
            return;
        };
        if edit.suggestions.is_empty() {
            self.address_edit = None;
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        let value = edit.input.read(cx).value().to_string();
        let Some(ghost) = completion_ghost(&value, &edit.suggestions) else {
            cx.notify();
            return;
        };
        let text = format!("{value}{ghost}");
        let input = edit.input.clone();
        input.update(cx, |state, cx| state.set_value(text, window, cx));
        if let Some(edit) = self.address_edit.as_mut() {
            edit.error = None;
        }
        // set_value не шлёт Change — пересчитываем подсказки сами.
        self.refresh_address_suggestions(cx);
        cx.notify();
    }

    /// Escape в подсказках: сначала убираем бледный хвост, поле остаётся.
    pub(crate) fn dismiss_address_suggestions(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(edit) = self.address_edit.as_mut() else {
            return false;
        };
        if edit.suggestions.is_empty() {
            return false;
        }
        edit.suggestions.clear();
        cx.notify();
        true
    }

    fn submit_address(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &self.address_edit else {
            return;
        };
        let index = edit.panel;
        let input = edit.input.clone();
        let raw = edit.input.read(cx).value().trim().to_string();
        let base = {
            let panel = self.panel(index);
            if panel.is_archive {
                panel.path.parent().unwrap_or(&panel.path).to_path_buf()
            } else {
                panel.path.clone()
            }
        };
        let sort_key = self.panel(index).sort_key;
        let sort_asc = self.panel(index).sort_asc;
        let view_mode = self.panel(index).view_mode;
        // Новый адрес отменяет незавершённую загрузку этой панели.
        self.panels[index].reload_gen = self.panels[index].reload_gen.wrapping_add(1);
        let generation = self.panels[index].reload_gen;

        // Разбор адреса, чтение и проверка подкаталога — в фоне: на сетевых
        // дисках и больших архивах это заметные блокировки.
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let (path, prefix) = resolve_address(&base, &raw)?;
                    let read = read_panel(&path)?;
                    if !prefix.is_empty() && !prefix_exists(&read.entries, &prefix) {
                        return Err("Папка внутри архива не найдена".to_string());
                    }
                    Ok((path, prefix, read))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                // Поле могло закрыться или смениться — результат не нужен.
                let still_current = this
                    .address_edit
                    .as_ref()
                    .is_some_and(|edit| edit.input == input);
                if !still_current || this.panels[index].reload_gen != generation {
                    return;
                }
                match result {
                    Ok((path, prefix, read)) => {
                        let panel = &mut this.panels[index];
                        panel.path = path;
                        panel.prefix = if prefix.is_empty() {
                            String::new()
                        } else {
                            format!("{prefix}/")
                        };
                        panel.sort_key = sort_key;
                        panel.sort_asc = sort_asc;
                        panel.view_mode = view_mode;
                        panel.is_archive = read.is_archive;
                        panel.kind = read.kind;
                        panel.encrypted = read.encrypted;
                        panel.all_entries = read.entries;
                        panel.error = None;
                        panel.loading = false;
                        panel.loaded = true;
                        panel.clear_selection();
                        panel.rebuild_view();
                        panel.scroll_to_top();
                        this.address_edit = None;
                        this.notice = None;
                        this.with_window(cx, |this, window, cx| {
                            this.sync_title(window);
                            window.focus(&this.focus, cx);
                        });
                        this.prefetch_icons(index, cx);
                    }
                    Err(error) => {
                        if let Some(edit) = &mut this.address_edit {
                            edit.error = Some(error);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

pub(crate) struct Panel {
    pub(crate) path: PathBuf,
    /// Текущий уровень внутри архива: `""`, `"docs/"`, `"docs/img/"`.
    pub(crate) prefix: String,
    /// Полный плоский перечень архива; `entries` — то, что видно на уровне.
    pub(crate) all_entries: Vec<Entry>,
    pub(crate) entries: Arc<Vec<Entry>>,
    /// Что выделено. Множественное выделение — как в проводнике.
    /// Arc, чтобы кадр не клонировал всё выделение: рендер берёт клон Arc,
    /// мутации идут через `Arc::make_mut`.
    pub(crate) selection: Arc<BTreeSet<usize>>,
    /// Курсор клавиатуры: Enter и стрелки ходят по нему.
    pub(crate) active: Option<usize>,
    /// Якорь для Shift+клик и Shift+стрелки.
    pub(crate) anchor: Option<usize>,
    pub(crate) sort_key: SortKey,
    pub(crate) sort_asc: bool,
    /// Как показан список: таблица, значки, список (Ctrl+1…4).
    pub(crate) view_mode: ViewMode,
    pub(crate) is_archive: bool,
    pub(crate) kind: Option<String>,
    /// Содержимое архива зашифровано: проверка спросит пароль до запуска.
    pub(crate) encrypted: bool,
    pub(crate) error: Option<String>,
    pub(crate) load_ms: u128,
    /// Прокрутка списка: нужна, чтобы выделение не уезжало за край.
    pub(crate) list_scroll: UniformListScrollHandle,
    /// Идёт протяжка мышью. `None` — кнопка не нажата.
    pub(crate) rubber: Option<RubberBand>,
    /// Панель уже читала содержимое. Вторая панель создаётся лениво: не хочется
    /// на старте второй раз запускать 7z ради скрытого списка.
    pub(crate) loaded: bool,
    /// Идёт фоновое чтение содержимого: список показывает «Загрузка…».
    pub(crate) loading: bool,
    /// Поколение загрузки: каждый новый reload увеличивает счётчик, устаревший
    /// фоновый результат с другим поколением не применяется (гонка навигации).
    pub(crate) reload_gen: u64,
    /// Подкаталог архива, который нужно применить после первой загрузки
    /// (стартовый `a.7z\docs` из аргументов).
    pub(crate) pending_prefix: Option<String>,
}

impl Panel {
    /// Панель без чтения содержимого. `path` — куда она посмотрит при `reload`.
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            prefix: String::new(),
            all_entries: Vec::new(),
            entries: Arc::new(Vec::new()),
            selection: Arc::new(BTreeSet::new()),
            active: None,
            anchor: None,
            sort_key: SortKey::Name,
            sort_asc: true,
            view_mode: ViewMode::Details,
            is_archive: false,
            kind: None,
            encrypted: false,
            error: None,
            load_ms: 0,
            list_scroll: UniformListScrollHandle::new(),
            rubber: None,
            loaded: false,
            loading: false,
            reload_gen: 0,
            pending_prefix: None,
        }
    }

    /// Открыть путь (каталог или архив) и, если задан, подкаталог архива.
    /// Содержимое читается позже в фоне (`reload_panel`): конструктор не должен
    /// морозить окно запуском 7z.
    pub(crate) fn open(path: PathBuf, prefix: Option<String>) -> Self {
        let mut panel = Self::new(path);
        panel.pending_prefix = prefix;
        panel
    }

    pub(crate) fn address(&self) -> String {
        let mut text = self.path.display().to_string();
        if !text.ends_with(std::path::MAIN_SEPARATOR) {
            text.push(std::path::MAIN_SEPARATOR);
        }
        if self.is_archive {
            text.push_str(&self.prefix.replace('/', std::path::MAIN_SEPARATOR_STR));
        }
        text
    }

    pub(crate) fn total_size(&self) -> u64 {
        self.entries
            .iter()
            .filter(|entry| !entry.is_dir)
            .map(|entry| entry.size)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    // Не `super::*`: он тянет макрос `test` из prelude кита, и `#[test]`
    // раскручивается сам в себя до предела рекурсии.
    use crate::prelude::{Entry, Panel, prefix_exists, resolve_address, temp_open_dir};

    #[test]
    fn address_roundtrip_resolves_directory() {
        let directory = temp_open_dir().unwrap();
        let panel = Panel::new(directory.clone());
        let (path, prefix) = resolve_address(&panel.path, &panel.address()).unwrap();
        assert_eq!(path, directory.canonicalize().unwrap());
        assert!(prefix.is_empty());
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn relative_address_opens_child_and_invalid_address_is_error() {
        let directory = temp_open_dir().unwrap();
        let child = directory.join("child");
        std::fs::create_dir(&child).unwrap();
        let (path, prefix) = resolve_address(&directory, "child").unwrap();
        assert_eq!(path, child.canonicalize().unwrap());
        assert!(prefix.is_empty());
        assert!(resolve_address(&directory, "").is_err());
        assert!(resolve_address(&directory, "missing").is_err());
        std::fs::remove_dir(child).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn archive_address_splits_prefix_and_validates_it() {
        let directory = temp_open_dir().unwrap();
        let archive = directory.join("a.7z");
        std::fs::write(&archive, b"x").unwrap();
        let (path, prefix) = resolve_address(&directory, "a.7z/docs/img").unwrap();
        assert_eq!(path, archive);
        assert_eq!(prefix, "docs/img");
        let entries = vec![Entry::file("docs/img/logo.png"), Entry::dir("docs")];
        assert!(prefix_exists(&entries, "docs/img"));
        assert!(prefix_exists(&entries, "docs"));
        assert!(!prefix_exists(&entries, "nope"));
        std::fs::remove_dir_all(directory).unwrap();
    }
}

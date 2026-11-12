use crate::prelude::*;
impl Panel {
    /// Пересобирает видимый уровень из `all_entries` и сортирует его.
    pub(crate) fn rebuild_view(&mut self) {
        let mut entries = if self.is_archive {
            view_at(&self.all_entries, &self.prefix)
        } else {
            self.all_entries.clone()
        };
        sort_entries(&mut entries, self.sort_key, self.sort_asc);
        self.entries = Arc::new(entries);
    }

    /// После смены каталога список начинается с первой строки — как в 7-Zip FM.
    pub(crate) fn scroll_to_top(&mut self) {
        self.list_scroll.scroll_to_item(0, ScrollStrategy::Nearest);
    }

    /// Переключение вида (Ctrl+1…4 и меню «Вид»).
    pub(crate) fn set_view(&mut self, mode: ViewMode) {
        if self.view_mode == mode {
            return;
        }
        self.view_mode = mode;
        if let Some(index) = self.active {
            self.scroll_to_entry(index);
        }
    }

    /// Сколько значков в строке при текущей ширине списка.
    ///
    /// Ширину берём у самого списка (прошлый кадр): окно меняет размер, а
    /// считать геометрию руками в трёх местах — источник расхождений.
    pub(crate) fn grid_columns(&self) -> usize {
        if !self.view_mode.is_grid() {
            return 1;
        }
        let width = f32::from(self.list_scroll.0.borrow().base_handle.bounds().size.width);
        let tile = theme::tile_width(self.view_mode);
        if width <= 0.0 || tile <= 0.0 {
            return 6;
        }
        ((width / tile).floor() as usize).max(1)
    }

    /// Прокрутить к объекту: в сетке строк меньше, чем объектов.
    pub(crate) fn scroll_to_entry(&mut self, index: usize) {
        let row = if self.view_mode.is_grid() {
            index / self.grid_columns().max(1)
        } else {
            index
        };
        self.list_scroll
            .scroll_to_item(row, ScrollStrategy::Nearest);
    }

    /// Ставит курсор и выделение на запись с таким именем.
    ///
    /// Нужно при выходе из архива: 7-Zip FM возвращает в каталог с выделенным
    /// архивом, иначе Enter → Backspace → Enter начинается с поиска строки заново.
    pub(crate) fn select_by_name(&mut self, name: &str) {
        if let Some(index) = self.entries.iter().position(|entry| entry.name == name) {
            self.select_only(index);
            self.scroll_to_entry(index);
        }
    }

    /// Сортирует текущий уровень, сохраняя объекты, а не прежние индексы строк.
    pub(crate) fn set_sort(&mut self, key: SortKey, ascending: bool) {
        // Имена берём из видимого уровня: в архиве all_entries содержит полные пути.
        let previous = Arc::clone(&self.entries);
        let selected: BTreeSet<_> = self
            .selection
            .iter()
            .filter_map(|index| previous.get(*index))
            .map(|entry| entry.name.as_str())
            .collect();
        let active = self
            .active
            .and_then(|index| previous.get(index))
            .map(|entry| entry.name.as_str());
        let anchor = self
            .anchor
            .and_then(|index| previous.get(index))
            .map(|entry| entry.name.as_str());

        self.sort_key = key;
        self.sort_asc = ascending;
        self.rebuild_view();
        self.clear_selection();
        let selection = Arc::make_mut(&mut self.selection);
        for (index, entry) in self.entries.iter().enumerate() {
            if selected.contains(entry.name.as_str()) {
                selection.insert(index);
            }
            // Курсор и якорь могут стоять на невыделенных объектах.
            if active == Some(entry.name.as_str()) {
                self.active = Some(index);
            }
            if anchor == Some(entry.name.as_str()) {
                self.anchor = Some(index);
            }
        }
        if let Some(index) = self.active {
            self.scroll_to_entry(index);
        }
    }

    pub(crate) fn toggle_sort(&mut self, key: SortKey) {
        let ascending = self.sort_key != key || !self.sort_asc;
        self.set_sort(key, ascending);
    }

    /// Элементы текущего уровня, выделенные пользователем, в виде путей архива.
    ///
    /// На корне пустой список означает «весь архив» — ровно так 7z понимает
    /// отсутствие шаблонов: команда короче и не теряются вложенные элементы.
    /// Внутри подпапки «выделено всё» — это только её содержимое, поэтому
    /// отдаём маску `prefix*`, а не пустой список: иначе распаковка одного
    /// уровня вытащила бы весь архив.
    pub(crate) fn selected_paths(&self) -> Vec<String> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        if self.selection.len() >= self.entries.len() {
            if self.prefix.is_empty() {
                return Vec::new();
            }
            return vec![format!("{}*", self.prefix)];
        }
        self.selection
            .iter()
            .filter_map(|index| self.entries.get(*index))
            .map(|entry| format!("{}{}", self.prefix, entry.name))
            .collect()
    }
}

impl MainWindow {
    /// Асинхронная перезагрузка панели: чтение каталога/архива идёт в фоновом
    /// потоке, результат применяется только если он ещё актуален (поколение).
    /// `then` выполняется на UI-потоке после успешного чтения.
    pub(crate) fn reload_panel(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + Send + 'static,
    ) {
        let panel = &mut self.panels[index];
        panel.reload_gen = panel.reload_gen.wrapping_add(1);
        let generation = panel.reload_gen;
        let path = panel.path.clone();
        panel.loading = true;
        panel.error = None;
        panel.clear_selection();
        panel.entries = Arc::new(Vec::new());
        panel.is_archive = !path.is_dir();
        cx.spawn(async move |this, cx| {
            let (result, elapsed) = cx
                .background_executor()
                .spawn(async move {
                    let started = Instant::now();
                    let result = read_panel(&path);
                    (result, started.elapsed())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.panels[index].reload_gen != generation {
                    return;
                }
                let ok = {
                    let panel = &mut this.panels[index];
                    panel.loading = false;
                    panel.loaded = true;
                    panel.load_ms = elapsed.as_millis();
                    match result {
                        Ok(read) => {
                            panel.is_archive = read.is_archive;
                            panel.kind = read.kind;
                            panel.encrypted = read.encrypted;
                            panel.all_entries = read.entries;
                            panel.error = None;
                            if let Some(prefix) = panel.pending_prefix.take()
                                && panel.is_archive
                            {
                                panel.prefix = if prefix.ends_with('/') {
                                    prefix
                                } else {
                                    format!("{prefix}/")
                                };
                            }
                            panel.rebuild_view();
                            true
                        }
                        Err(err) => {
                            panel.all_entries = Vec::new();
                            panel.entries = Arc::new(Vec::new());
                            panel.error = Some(err);
                            false
                        }
                    }
                };
                if ok {
                    then(this, cx);
                    this.prefetch_icons(index, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Догружает системные значки текущего уровня в фоне.
    ///
    /// Вызывается после каждого чтения панели, а не в `render`: оболочка
    /// Windows отвечает синхронно, и запрос из кадра подвесил бы окно.
    /// Запросы идемпотентны — готовые и уже заказанные значки пропускаются.
    pub(crate) fn prefetch_icons(&mut self, index: usize, cx: &mut Context<Self>) {
        let panel = &self.panels[index];
        let requests = self
            .icons
            .requests(panel.entries.as_slice(), &panel.path, panel.is_archive);
        if requests.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move {
                    requests
                        .into_iter()
                        .map(|request| {
                            let image = load_system_icon(&request);
                            (request, image)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.icons.complete(results);
                cx.notify();
            });
        })
        .detach();
    }

    /// Действие с окном из контекста без Window (завершение фоновой задачи):
    /// откладываем до конца текущего обновления, иначе окно занято повторно.
    /// Корень окна — обёртка Root кита, поэтому обновляем сущность напрямую,
    /// а не через downcast корневого view.
    pub(crate) fn with_window(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let entity = cx.entity();
        cx.defer(move |cx| {
            if let Some(handle) = cx.active_window() {
                let _ = handle.update(cx, |_root, window, cx| {
                    entity.update(cx, |this, cx| f(this, window, cx));
                });
            }
        });
    }

    /// На уровень вверх. Из корня архива — в каталог, где лежит сам архив,
    /// с выделением на нём. Чтение нового каталога — в фоне.
    pub(crate) fn go_up(&mut self, index: usize, cx: &mut Context<Self>) {
        // Что выделить в каталоге, куда возвращаемся (имя папки или архива).
        let mut landed: Option<String> = None;
        let mut needs_reload = false;

        let panel = &mut self.panels[index];
        if panel.is_archive {
            match parent_prefix(&panel.prefix) {
                Some(parent) => {
                    let child_part = panel
                        .prefix
                        .strip_prefix(&parent)
                        .unwrap_or(&panel.prefix)
                        .trim_end_matches('/');
                    if !child_part.is_empty() {
                        landed = Some(child_part.to_string());
                    }
                    panel.prefix = parent;
                    panel.clear_selection();
                    panel.rebuild_view();
                }
                None => {
                    if let Some(parent) = panel.path.parent() {
                        landed = panel
                            .path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned());
                        panel.path = parent.to_path_buf();
                        panel.prefix.clear();
                        needs_reload = true;
                    }
                }
            }
        } else if let Some(parent) = panel.path.parent() {
            landed = panel
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
            panel.path = parent.to_path_buf();
            needs_reload = true;
        }

        if needs_reload {
            self.reload_panel(index, cx, move |this, cx| {
                match landed {
                    Some(name) => this.panels[index].select_by_name(&name),
                    None => this.panels[index].scroll_to_top(),
                }
                this.with_window(cx, |this, window, _| this.sync_title(window));
            });
        } else {
            match landed {
                Some(name) => self.panels[index].select_by_name(&name),
                None => self.panels[index].scroll_to_top(),
            }
            self.prefetch_icons(index, cx);
        }
    }

    /// Вход по Enter / двойному щелчку: каталог — внутрь, файл — открытие.
    ///
    /// Как в 7-Zip FM: Enter на файле архива открывает его так же, как на папке;
    /// обычный файл открывается внешней программой, а внутри архива — сначала
    /// извлекается во временную папку. Что считать архивом, решает ядро
    /// (`is_archive_name`) — по расширению, без чтения заголовка.
    pub(crate) fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel_index = self.active_panel;
        let panel = self.active();
        let Some(entry) = panel
            .active
            .or_else(|| panel.selection.iter().next().copied())
            .and_then(|index| panel.entries.get(index).cloned())
        else {
            return;
        };

        if panel.is_archive {
            if entry.is_dir {
                let panel = self.active_mut();
                panel.prefix = child_prefix(&panel.prefix, &entry.name);
                panel.clear_selection();
                panel.rebuild_view();
                panel.scroll_to_top();
                self.prefetch_icons(panel_index, cx);
            } else {
                let name = format!("{}{}", panel.prefix, entry.name);
                self.open_from_archive(name, window, cx);
            }
            return;
        }

        let next = panel.path.join(&entry.name);
        if entry.is_dir {
            if next.is_dir() {
                self.panels[panel_index].path = next;
                self.reload_panel(panel_index, cx, move |this, cx| {
                    this.panels[panel_index].scroll_to_top();
                    this.with_window(cx, |this, window, _| this.sync_title(window));
                });
            }
            return;
        }

        if !next.is_file() {
            return;
        }
        if is_archive_name(&entry.name) {
            self.enter_archive(next, cx);
        } else {
            self.open_entry_with(ShellVerb::Open, window, cx);
        }
    }

    /// Вход в архив в активной панели; листинг читается в фоне. Переезжаем
    /// только после успеха: если 7z файл не принял, панель остаётся в каталоге
    /// и объясняет причину в статус-баре.
    pub(crate) fn enter_archive(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let index = self.active_panel;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let panel = &mut self.panels[index];
        panel.reload_gen = panel.reload_gen.wrapping_add(1);
        let generation = panel.reload_gen;
        panel.loading = true;
        panel.clear_selection();

        cx.spawn(async move |this, cx| {
            let read_path = path.clone();
            let (result, elapsed) = cx
                .background_executor()
                .spawn(async move {
                    let started = Instant::now();
                    let result = read_archive(&read_path);
                    (result, started.elapsed())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.panels[index].reload_gen != generation {
                    return;
                }
                let panel = &mut this.panels[index];
                panel.loading = false;
                match result {
                    Ok(listing) => {
                        panel.error = None;
                        panel.path = path;
                        panel.is_archive = true;
                        panel.prefix.clear();
                        panel.kind = listing.kind;
                        panel.encrypted = listing.encrypted;
                        panel.all_entries = listing.entries;
                        panel.clear_selection();
                        panel.rebuild_view();
                        panel.load_ms = elapsed.as_millis();
                        panel.loaded = true;
                        panel.scroll_to_top();
                        this.notice = None;
                        this.with_window(cx, |this, window, _| this.sync_title(window));
                        this.prefetch_icons(index, cx);
                    }
                    Err(err) => {
                        this.notice = Some(open_failure(&name, &err));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Держит заголовок окна в согласии с адресом активной панели: 7-Zip FM показывает путь.
    ///
    /// Вызывать только там, где путь меняется. `SetWindowText` на каждое нажатие
    /// клавиши — это лишний синхронный заход в систему на каждый символ.
    pub(crate) fn sync_title(&mut self, window: &mut Window) {
        let address = self.active().address();
        if self.title_cache != address {
            window.set_window_title(&address);
            self.title_cache = address;
        }
    }
}

#[cfg(test)]
mod tests {
    // Не `super::*`: он тянет макрос `test` из prelude кита, и `#[test]`
    // раскручивается сам в себя до предела рекурсии.
    use crate::prelude::{Arc, BTreeSet, Entry, Panel, PathBuf, SortKey};

    fn sample_panel() -> Panel {
        let mut panel = Panel::new(PathBuf::from("."));
        panel.all_entries = [("a.txt", 30), ("b.txt", 10), ("c.txt", 40), ("d.txt", 20)]
            .into_iter()
            .map(|(name, size)| {
                let mut entry = Entry::file(name);
                entry.size = size;
                entry
            })
            .collect();
        panel.rebuild_view();
        panel
    }

    fn names(panel: &Panel) -> Vec<&str> {
        panel
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect()
    }

    #[test]
    fn set_sort_keeps_multiple_selection_and_unselected_cursor() {
        let mut panel = sample_panel();
        panel.selection = Arc::new(BTreeSet::from([0, 2])); // a, c
        panel.active = Some(1); // b, не выделен
        panel.anchor = Some(0); // a

        panel.set_sort(SortKey::Size, true);

        assert_eq!(names(&panel), ["b.txt", "d.txt", "a.txt", "c.txt"]);
        assert_eq!(panel.sort_key, SortKey::Size);
        assert!(panel.sort_asc);
        assert_eq!(*panel.selection, BTreeSet::from([2, 3])); // a, c
        assert_eq!(panel.active, Some(0)); // b
        assert!(!panel.selection.contains(&0));
        assert_eq!(panel.anchor, Some(2)); // a
    }

    #[test]
    fn toggle_sort_keeps_objects_in_both_directions() {
        let mut panel = sample_panel();
        panel.selection = Arc::new(BTreeSet::from([0, 2])); // a, c
        panel.active = Some(1); // b
        panel.anchor = Some(0); // a

        panel.toggle_sort(SortKey::Name);

        assert_eq!(names(&panel), ["d.txt", "c.txt", "b.txt", "a.txt"]);
        assert!(!panel.sort_asc);
        assert_eq!(*panel.selection, BTreeSet::from([1, 3]));
        assert_eq!(panel.active, Some(2));
        assert_eq!(panel.anchor, Some(3));

        panel.toggle_sort(SortKey::Name);

        assert_eq!(names(&panel), ["a.txt", "b.txt", "c.txt", "d.txt"]);
        assert!(panel.sort_asc);
        assert_eq!(*panel.selection, BTreeSet::from([0, 2]));
        assert_eq!(panel.active, Some(1));
        assert_eq!(panel.anchor, Some(0));
    }

    #[test]
    fn toggle_sort_new_key_keeps_unselected_anchor_for_shift_selection() {
        let mut panel = sample_panel();
        panel.toggle_sort(SortKey::Name); // d, c, b, a
        panel.selection = Arc::new(BTreeSet::from([0, 1])); // d, c
        panel.active = Some(2); // b
        panel.anchor = Some(3); // a, не выделен и не совпадает с курсором

        panel.toggle_sort(SortKey::Size);

        assert_eq!(names(&panel), ["b.txt", "d.txt", "a.txt", "c.txt"]);
        assert!(panel.sort_asc);
        assert_eq!(*panel.selection, BTreeSet::from([1, 3])); // d, c
        assert_eq!(panel.active, Some(0)); // b
        assert_eq!(panel.anchor, Some(2)); // a
        panel.extend_selection(0);
        assert_eq!(*panel.selection, BTreeSet::from([0, 1, 2])); // b..a
        assert_eq!(panel.anchor, Some(2));
    }

    #[test]
    fn set_sort_ascending_restores_menu_order_without_selecting_cursor() {
        let mut panel = sample_panel();
        panel.toggle_sort(SortKey::Name); // d, c, b, a
        panel.active = Some(3); // a, пустое выделение и нет якоря

        panel.set_sort(SortKey::Name, true);

        assert_eq!(names(&panel), ["a.txt", "b.txt", "c.txt", "d.txt"]);
        assert!(panel.sort_asc);
        assert!(panel.selection.is_empty());
        assert_eq!(panel.active, Some(0));
        assert_eq!(panel.anchor, None);
    }

    #[test]
    fn sorting_without_cursor_does_not_create_selection() {
        for mut panel in [Panel::new(PathBuf::from(".")), sample_panel()] {
            panel.toggle_sort(SortKey::Name);
            panel.set_sort(SortKey::Size, true);

            assert!(panel.selection.is_empty());
            assert_eq!(panel.active, None);
            assert_eq!(panel.anchor, None);
        }
    }

    #[test]
    fn sorting_archive_uses_visible_names_and_keeps_current_level() {
        let mut panel = Panel::new(PathBuf::from("archive.7z"));
        panel.is_archive = true;
        panel.prefix = "docs/".to_string();
        panel.all_entries = vec![
            Entry::file("docs/b.txt"),
            Entry::file("docs/a.txt"),
            Entry::file("other/a.txt"),
        ];
        panel.rebuild_view();
        panel.selection = Arc::new(BTreeSet::from([0])); // a
        panel.active = Some(1); // b
        panel.anchor = Some(1);

        panel.toggle_sort(SortKey::Name);

        assert_eq!(names(&panel), ["b.txt", "a.txt"]);
        assert_eq!(*panel.selection, BTreeSet::from([1]));
        assert_eq!(panel.active, Some(0));
        assert_eq!(panel.anchor, Some(0));
        assert_eq!(panel.path, PathBuf::from("archive.7z"));
        assert_eq!(panel.prefix, "docs/");
    }

    #[test]
    fn selected_archive_paths_use_current_prefix() {
        let mut panel = Panel::new(PathBuf::from("archive.7z"));
        panel.is_archive = true;
        panel.prefix = "docs/".to_string();
        panel.all_entries = vec![
            Entry::file("docs/a.txt"),
            Entry::file("docs/b.txt"),
            Entry::file("other/c.txt"),
        ];
        panel.rebuild_view();
        panel.selection = Arc::new(BTreeSet::from([1]));

        assert_eq!(panel.selected_paths(), ["docs/b.txt"]);
    }

    #[test]
    fn select_all_inside_archive_folder_does_not_mean_whole_archive() {
        let mut panel = Panel::new(PathBuf::from("archive.7z"));
        panel.is_archive = true;
        panel.prefix = "docs/".to_string();
        panel.all_entries = vec![
            Entry::file("docs/a.txt"),
            Entry::file("docs/b.txt"),
            Entry::file("other/c.txt"),
        ];
        panel.rebuild_view();
        panel.selection = Arc::new(BTreeSet::from([0, 1]));

        assert_eq!(panel.selected_paths(), ["docs/*"]);
    }

    #[test]
    fn select_all_at_archive_root_means_whole_archive() {
        let mut panel = Panel::new(PathBuf::from("archive.7z"));
        panel.is_archive = true;
        panel.all_entries = vec![Entry::file("a.txt"), Entry::file("b.txt")];
        panel.rebuild_view();
        panel.selection = Arc::new(BTreeSet::from([0, 1]));

        assert!(panel.selected_paths().is_empty());
    }
}

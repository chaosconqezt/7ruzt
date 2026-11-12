use crate::prelude::*;
use gpui_kit::component::menu::ContextMenuExt as _;
/// Какие колонки влезают в ширину панели.
///
/// Ширины колонок фиксированные, а в двухпанельном режиме на панель приходится
/// половина окна. Если рисовать все семь, «Имя» сжимается в ноль — ровно это и
/// случилось на первом кадре двух панелей. Поэтому лишние колонки убираем по
/// мере сужения, а имя оставляем читаемым: это главная колонка.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Columns {
    pub(crate) created: bool,
    pub(crate) comment: bool,
    pub(crate) dirs: bool,
    pub(crate) files: bool,
}

impl Columns {
    pub(crate) fn for_width(width: f32) -> Self {
        // Имя не должно стать уже ~160 px; прокрутка тоже занимает место.
        let base = theme::COL_SIZE + theme::COL_MODIFIED + 160. + theme::scrollbar_w();
        let created = width >= base + theme::COL_CREATED;
        let comment = created && width >= base + theme::COL_CREATED + theme::COL_COMMENT;
        let dirs =
            comment && width >= base + theme::COL_CREATED + theme::COL_COMMENT + theme::COL_DIRS;
        let files = dirs
            && width
                >= base
                    + theme::COL_CREATED
                    + theme::COL_COMMENT
                    + theme::COL_DIRS
                    + theme::COL_FILES;
        Self {
            created,
            comment,
            dirs,
            files,
        }
    }
}

/// Данные строки списка — без элементов и без заимствований: их строит `uniform_list`.
pub(crate) struct RowData {
    pub(crate) name: String,
    pub(crate) size: String,
    pub(crate) modified: String,
    pub(crate) created: String,
    pub(crate) is_dir: bool,
    pub(crate) is_archive: bool,
    /// Системный значок из кеша; `None` — рисуем встроенный SVG.
    pub(crate) icon: Option<EntryIcon>,
    /// Что тащим, если потянуть за эту строку. `None` внутри архива: файлов
    /// на диске там нет, отдавать системе нечего.
    pub(crate) drag: Option<std::rc::Rc<Vec<(PathBuf, bool)>>>,
    /// Записи архива, которые тащим из открытого архива. В отличие от `drag`,
    /// это не пути на диске: при сбросе на папку их надо сначала распаковать.
    pub(crate) archive_drag: Option<ArchiveDragData>,
}

/// Записи архива для перетаскивания (имена внутри архива, уже с префиксом).
#[derive(Clone)]
pub(crate) struct ArchiveDragData {
    pub(crate) archive: PathBuf,
    pub(crate) entries: std::rc::Rc<Vec<String>>,
    pub(crate) encrypted: bool,
}

/// Перетаскиваемые записи архива.
///
/// Внешней системе их отдать нельзя — файлов на диске ещё нет, — поэтому
/// такой призрак принимают только наши панели: сброс на папку запускает
/// распаковку выбранных записей.
#[derive(Clone)]
pub(crate) struct DragArchive {
    pub(crate) label: String,
    pub(crate) archive: PathBuf,
    pub(crate) entries: std::rc::Rc<Vec<String>>,
    pub(crate) encrypted: bool,
}

/// Перетаскиваемый из списка набор файлов.
///
/// Показывается «призраком» под курсором, а при выходе за окно уходит системе:
/// платформа сама решает, примет ли её проводник.
#[derive(Clone)]
pub(crate) struct DragFiles {
    pub(crate) label: String,
    pub(crate) files: std::rc::Rc<Vec<(PathBuf, bool)>>,
}

impl Render for DragFiles {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        drag_ghost(self.label.clone(), cx)
    }
}

impl Render for DragArchive {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        drag_ghost(self.label.clone(), cx)
    }
}

fn drag_ghost(label: String, cx: &mut App) -> impl IntoElement {
    let c = Colors::new(cx.theme());
    div()
        .px_2()
        .py_1()
        .rounded_sm()
        .border_1()
        .border_color(c.accent)
        .bg(c.bg)
        .text_sm()
        .text_color(c.fg)
        .child(label)
}

impl MainWindow {
    /// Набор колонок панели: в двухпанельном режиме на неё приходится половина окна.
    pub(crate) fn columns(&self, _panel_index: usize) -> Columns {
        let divider = if self.two_panel { 1. } else { 0. };
        let width = (self.window_width - divider) / self.panel_count() as f32;
        Columns::for_width(width)
    }

    /// Ячейка шапки: клик сортирует по колонке, активная показывает направление.
    ///
    /// Текст всех заголовков — по центру, независимо от того, как выровнены
    /// данные в колонке: так шапка выглядит ровно при любой ширине окна.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn header_cell(
        &self,
        panel_index: usize,
        label: &'static str,
        width: Option<f32>,
        key: SortKey,
        c: &Colors,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let panel = self.panel(panel_index);
        let marker = if panel.sort_key == key {
            if panel.sort_asc { " ^" } else { " v" }
        } else {
            ""
        };

        let cell = div()
            // id по метке, а не по SortKey: «Размер», «Папок» и «Файлов»
            // сортируют по одному ключу, но id элементов должны быть разными.
            .id(format!("head-{label}-{panel_index}"))
            .h_full()
            .px_2()
            .border_r_1()
            .border_color(c.border.opacity(0.6))
            .text_color(c.muted)
            .whitespace_nowrap()
            .overflow_hidden()
            .text_center()
            .cursor_pointer()
            .hover(|style| style.bg(c.hover()))
            .child(format!("{label}{marker}"))
            .on_click(cx.listener(move |this, _event, window, cx| {
                this.activate_panel(panel_index);
                this.panel_mut(panel_index).toggle_sort(key);
                this.sync_title(window);
                cx.notify();
            }));

        match width {
            Some(width) => cell.w(px(width)),
            None => cell.flex_1(),
        }
    }

    pub(crate) fn header_row(
        &self,
        panel_index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let c = Colors::new(cx.theme());
        let columns = self.columns(panel_index);
        let mut row = h_flex()
            .items_center()
            .h(px(theme::header_h()))
            .px_2()
            .bg(c.header())
            .border_b_1()
            .border_color(c.border)
            .child(self.header_cell(panel_index, "Имя", None, SortKey::Name, &c, cx))
            .child(self.header_cell(
                panel_index,
                "Размер",
                Some(theme::COL_SIZE),
                SortKey::Size,
                &c,
                cx,
            ))
            .child(self.header_cell(
                panel_index,
                "Изменён",
                Some(theme::COL_MODIFIED),
                SortKey::Modified,
                &c,
                cx,
            ));
        if columns.created {
            row = row.child(self.header_cell(
                panel_index,
                "Создан",
                Some(theme::COL_CREATED),
                SortKey::Created,
                &c,
                cx,
            ));
        }
        if columns.comment {
            row = row.child(self.header_cell(
                panel_index,
                "Комментарий",
                Some(theme::COL_COMMENT),
                SortKey::Comment,
                &c,
                cx,
            ));
        }
        if columns.dirs {
            row = row.child(self.header_cell(
                panel_index,
                "Папок",
                Some(theme::COL_DIRS),
                SortKey::Size,
                &c,
                cx,
            ));
        }
        if columns.files {
            row = row.child(self.header_cell(
                panel_index,
                "Файлов",
                Some(theme::COL_FILES),
                SortKey::Size,
                &c,
                cx,
            ));
        }
        row
            // Заглушка под полосу прокрутки: без неё шапка шире строк и колонки разъезжаются.
            .child(
                div()
                    .w(px(theme::scrollbar_w()))
                    .h_full()
                    .border_l_1()
                    .border_color(c.border.opacity(0.6)),
            )
    }

    /// Один обработчик клика на строку: он живёт в замыкании элемента и не имеет
    /// доступа к `self`, поэтому состояние достаём через `WeakEntity`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn row_clicked(
        &mut self,
        panel_index: usize,
        index: usize,
        modifiers: Modifiers,
        click_count: usize,
        dragged: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_panel(panel_index);
        window.focus(&self.focus, cx);

        // Переименование только что сохранилось по потере фокуса (этот же клик):
        // индекс строки относится к прежнему списку, выделение уже поставлено.
        // Клик в другой панели этим не гасим — там своё выделение.
        if self.rename_finished.take() == Some(panel_index) {
            cx.notify();
            return;
        }

        // Кнопку могли отпустить над той же строкой, где начали протяжку:
        // это конец рамки, а не клик — выделение уже построено.
        if dragged {
            cx.notify();
            return;
        }

        {
            let panel = self.panel_mut(panel_index);
            if modifiers.shift {
                panel.extend_selection(index);
            } else if modifiers.control {
                panel.toggle_selection(index);
            } else {
                panel.select_only(index);
            }
            panel.scroll_to_entry(index);
        }

        if click_count == 2 {
            self.open_selected(window, cx);
            self.sync_title(window);
        }

        self.ui_log(&format!(
            "click panel={panel_index} row={index} count={click_count} ctrl={} shift={} -> selection={}",
            modifiers.control,
            modifiers.shift,
            self.panel(panel_index).selection.len()
        ));
        cx.notify();
    }

    /// Элемент строки. Живёт без `self`: всё нужное приходит аргументами.
    ///
    /// `rename_input` — если эта строка сейчас правится (F2), вместо подписи
    /// ставим поле ввода прямо в строку, как в проводнике.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn row_element(
        panel_index: usize,
        data: &RowData,
        index: usize,
        selected: bool,
        current: bool,
        active: bool,
        columns: Columns,
        rename_input: Option<Entity<InputState>>,
        c: &Colors,
        weak: WeakEntity<MainWindow>,
    ) -> impl IntoElement {
        let renaming = rename_input.is_some();
        let full_name = data.name.clone();
        let hover = c.hover();
        // Неактивная панель: выделение приглушённое и без синей рамки, чтобы
        // было видно, какая панель сейчас принимает клавиатуру.
        let selection_bg = if active {
            c.selected()
        } else {
            c.fg.opacity(0.12)
        };
        let current_border = if current && active {
            c.blue
        } else {
            c.bg.opacity(0.)
        };

        let cell = |width: f32, right: bool, value: String| {
            let mut cell = div()
                .w(px(width))
                .px_2()
                .whitespace_nowrap()
                .overflow_hidden();
            if right {
                cell = cell.text_right();
            }
            cell.child(value)
        };

        // Для нажатия на сам объект (значок и имя): там начинается
        // перетаскивание файла, а не рамка выделения.
        let weak_name = weak.clone();
        let weak_archive = weak.clone();

        let mut row = h_flex()
            .id(("row", index * 2 + panel_index))
            .tooltip(move |window, cx| Tooltip::new(full_name.clone()).build(window, cx))
            .items_center()
            .w_full()
            .h(px(theme::row_h()))
            .px_2()
            // Справа резерв под полосу прокрутки — как в шапке: иначе колонки
            // шапки и строк разъезжаются на её ширину (скроллбар — overlay).
            .pr(px(theme::scrollbar_w()))
            .rounded_sm()
            .border_1()
            .border_color(current_border)
            .when(selected, |this| this.bg(selection_bg))
            .when(!selected, |this| this.hover(move |style| style.bg(hover)));

        if data.is_dir || data.is_archive {
            row = row
                .drag_over::<DragFiles>(|style, _files, _window, cx| {
                    style.bg(Colors::new(cx.theme()).accent.opacity(0.35))
                })
                .drag_over::<ExternalPaths>(|style, _paths, _window, cx| {
                    style.bg(Colors::new(cx.theme()).accent.opacity(0.35))
                });
        }
        if data.is_dir {
            row = row.drag_over::<DragArchive>(|style, _archive, _window, cx| {
                style.bg(Colors::new(cx.theme()).accent.opacity(0.35))
            });
        }

        let row = row.on_click(move |event, window, cx| {
            // Пока правится имя, клики по этой строке не трогаем: иначе
            // клик по самому полю ввода уводил бы фокус в список, а потеря
            // фокуса завершала бы правку — каретку поставить было нельзя.
            if renaming {
                return;
            }
            let count = event.click_count();
            let modifiers = event.modifiers();
            // Смещение между нажатием и отпусканием: так отличается клик
            // от конца протяжки рамкой (та идёт через `row_clicked`).
            let dragged = match &event {
                ClickEvent::Mouse(mouse) => {
                    let dx = f32::from(mouse.up.position.x - mouse.down.position.x);
                    let dy = f32::from(mouse.up.position.y - mouse.down.position.y);
                    dx.abs() > DRAG_THRESHOLD || dy.abs() > DRAG_THRESHOLD
                }
                _ => false,
            };
            let _ = weak.update(cx, |this, cx| {
                this.row_clicked(panel_index, index, modifiers, count, dragged, window, cx);
            });
        });

        // Поле правки имени вместо подписи. Ширина тянется, чтобы длинное имя
        // не обрезалось сразу.
        let name_part = match rename_input {
            Some(input) => div()
                .flex_1()
                .min_w(px(0.))
                .child(control_input(&input).w_full().appearance(false))
                .into_any_element(),
            None => div()
                .whitespace_nowrap()
                .overflow_hidden()
                .child(data.name.clone())
                .into_any_element(),
        };

        // Сам объект — значок и имя. Только за него тянется файл; остальное
        // место строки (пустой хвост колонки имени, размер, даты) заводит
        // рамку выделения, как в 7-Zip FM.
        let mut object = h_flex()
            .id(("row-object", index * 2 + panel_index))
            .items_center()
            .gap_2()
            // Поле правки должно занять всю колонку имени: без `flex_1` контейнер
            // сжимается до содержимого, и поле ввода вырождается в узкую плашку.
            .when(renaming, |this| this.flex_1().min_w(px(0.)))
            .child(entry_icon(
                data.icon.clone(),
                data.is_dir,
                data.is_archive,
                theme::ICON_DETAILS,
                c,
            ))
            .child(name_part);

        if let Some(files) = &data.drag
            && !renaming
        {
            let label = if files.len() > 1 {
                format!("{} объекта", files.len())
            } else {
                data.name.clone()
            };
            let payload = DragFiles {
                label,
                files: files.clone(),
            };
            object = object
                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    // Нажатие на объект — это перетаскивание файла, а не рамка.
                    // Но клик по имени обязан выделять строку. Раньше здесь
                    // останавливалось всплытие, и `on_click` строки не получал
                    // нажатие — выделялось только пустое место справа от имени.
                    // Уже выделенную строку не схлопываем: иначе Ctrl-набор
                    // потерялся бы в момент начала перетаскивания.
                    if !selected
                        || event.modifiers.control
                        || event.modifiers.shift
                        || event.click_count == 2
                    {
                        let _ = weak_name.update(cx, |this, cx| {
                            this.row_clicked(
                                panel_index,
                                index,
                                event.modifiers,
                                event.click_count,
                                false,
                                window,
                                cx,
                            );
                        });
                    } else {
                        let _ = weak_name.update(cx, |this, cx| {
                            this.activate_panel(panel_index);
                            window.focus(&this.focus, cx);
                        });
                    }
                    cx.stop_propagation();
                })
                .on_drag(payload, |payload, _offset, _window, cx| {
                    cx.new(|_| payload.clone())
                })
                .external_drag_payload::<DragFiles>(|payload, _window, _cx| {
                    Some(ExternalDragPayload::Files(FileDragPaths::new(
                        payload.files.iter().cloned(),
                    )))
                });
        } else if let Some(archive) = &data.archive_drag
            && !renaming
        {
            let label = if archive.entries.len() > 1 {
                format!("{} объекта", archive.entries.len())
            } else {
                data.name.clone()
            };
            let payload = DragArchive {
                label,
                archive: archive.archive.clone(),
                entries: archive.entries.clone(),
                encrypted: archive.encrypted,
            };
            object = object
                .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    // Внутри архива тот же порядок: нажатие выделяет запись,
                    // а протяжка начинает перетаскивание. Выделенный набор
                    // не схлопываем — иначе Ctrl-выбор потерялся бы.
                    if !selected
                        || event.modifiers.control
                        || event.modifiers.shift
                        || event.click_count == 2
                    {
                        let _ = weak_archive.update(cx, |this, cx| {
                            this.row_clicked(
                                panel_index,
                                index,
                                event.modifiers,
                                event.click_count,
                                false,
                                window,
                                cx,
                            );
                        });
                    } else {
                        let _ = weak_archive.update(cx, |this, cx| {
                            this.activate_panel(panel_index);
                            window.focus(&this.focus, cx);
                        });
                    }
                    cx.stop_propagation();
                })
                .on_drag(payload, |payload, _offset, _window, cx| {
                    cx.new(|_| payload.clone())
                });
        }

        row.child(
            h_flex()
                .flex_1()
                .min_w(px(0.))
                .items_center()
                .overflow_hidden()
                .child(object),
        )
        .child(cell(theme::COL_SIZE, true, data.size.clone()))
        .child(cell(theme::COL_MODIFIED, false, data.modified.clone()))
        .when(columns.created, |this| {
            this.child(cell(theme::COL_CREATED, false, data.created.clone()))
        })
        .when(columns.comment, |this| {
            this.child(cell(theme::COL_COMMENT, false, String::new()))
        })
        .when(columns.dirs, |this| {
            this.child(cell(theme::COL_DIRS, true, String::new()))
        })
        .when(columns.files, |this| {
            this.child(cell(theme::COL_FILES, true, String::new()))
        })
    }

    pub(crate) fn list(&self, panel_index: usize, cx: &mut Context<Self>) -> AnyElement {
        let c = Colors::new(cx.theme());
        // Пока открыт диалог, всё перекрыто оверлеем: строить строки незачем,
        // именно это и давало лаги при вводе в поле имени.
        let modal_open = self.modal_open();
        let panel = self.panel(panel_index);

        if let Some(error) = &panel.error {
            return div()
                .flex_1()
                .bg(c.bg)
                .px_2()
                .py_2()
                .text_color(c.error())
                .whitespace_normal()
                .child(error.clone())
                .into_any_element();
        }

        if panel.loading {
            return div()
                .flex_1()
                .bg(c.bg)
                .px_2()
                .py_2()
                .text_color(c.muted)
                .child("Загрузка…")
                .into_any_element();
        }

        if modal_open {
            return div().flex_1().bg(c.bg).into_any_element();
        }

        if panel.entries.is_empty() {
            return self.list_container(
                panel_index,
                cx,
                div()
                    .flex_1()
                    .bg(c.bg)
                    .px_2()
                    .py_2()
                    .text_color(c.muted)
                    .child("Папка пуста"),
            );
        }

        match panel.view_mode {
            ViewMode::Details => self.details_list(panel_index, cx),
            ViewMode::List => self.plain_list(panel_index, cx),
            ViewMode::LargeIcons | ViewMode::SmallIcons => self.grid_list(panel_index, cx),
        }
    }

    /// Таблица с колонками — вид по умолчанию.
    pub(crate) fn details_list(&self, panel_index: usize, cx: &mut Context<Self>) -> AnyElement {
        let c = Colors::new(cx.theme());
        let panel = self.panel(panel_index);
        let entries = panel.entries.clone();
        let total = entries.len();
        let selection = panel.selection.clone();
        let current = panel.active;
        let is_archive = panel.is_archive;
        let base_path = panel.path.clone();

        let selected_drag: Option<std::rc::Rc<Vec<(PathBuf, bool)>>> =
            if !is_archive && panel.selection.len() > 1 {
                Some(std::rc::Rc::new(
                    panel
                        .selection
                        .iter()
                        .filter_map(|index| entries.get(*index))
                        .map(|entry| (base_path.join(&entry.name), entry.is_dir))
                        .collect(),
                ))
            } else {
                None
            };

        let archive_prefix = panel.prefix.clone();
        let encrypted = panel.encrypted;
        // Выделено несколько записей архива — тянем их все, как и файлы.
        let selected_entries: Option<std::rc::Rc<Vec<String>>> =
            if is_archive && panel.selection.len() > 1 {
                Some(std::rc::Rc::new(
                    panel
                        .selection
                        .iter()
                        .filter_map(|index| entries.get(*index))
                        .map(|entry| format!("{archive_prefix}{}", entry.name))
                        .collect(),
                ))
            } else {
                None
            };

        let weak = cx.entity().downgrade();
        let icons = self.icons.clone();
        let columns = self.columns(panel_index);
        let active = !self.two_panel || self.active_panel == panel_index;
        // Строка, которая сейчас правится: её отрисуем с полем ввода.
        let rename = self
            .rename
            .as_ref()
            .filter(|edit| edit.panel == panel_index)
            .map(|edit| (edit.index, edit.input.clone()));

        let list = uniform_list(
            ("file-list", panel_index),
            total,
            move |range, _window, _cx| {
                let mut items: Vec<AnyElement> = Vec::with_capacity(range.len());
                for index in range {
                    let Some(entry) = entries.get(index) else {
                        continue;
                    };
                    let selected = selection.contains(&index);
                    let rename_input = rename
                        .as_ref()
                        .and_then(|(target, input)| (*target == index).then(|| input.clone()));

                    let drag = if is_archive {
                        None
                    } else if selected && selected_drag.is_some() {
                        selected_drag.clone()
                    } else {
                        Some(std::rc::Rc::new(vec![(
                            base_path.join(&entry.name),
                            entry.is_dir,
                        )]))
                    };

                    let archive_drag = is_archive.then(|| ArchiveDragData {
                        archive: base_path.clone(),
                        entries: if selected && selected_entries.is_some() {
                            selected_entries.clone().unwrap_or_default()
                        } else {
                            std::rc::Rc::new(vec![format!("{archive_prefix}{}", entry.name)])
                        },
                        encrypted,
                    });

                    let data = RowData {
                        name: entry.name.clone(),
                        size: if entry.is_dir {
                            String::new()
                        } else {
                            entry.size_text()
                        },
                        modified: entry.modified.clone().unwrap_or_default(),
                        created: entry.created.clone().unwrap_or_default(),
                        is_dir: entry.is_dir,
                        is_archive: !entry.is_dir && is_archive_name(&entry.name),
                        icon: icons.lookup(
                            &entry.name,
                            entry.is_dir,
                            is_archive,
                            &base_path,
                            IconSize::Small,
                        ),
                        drag,
                        archive_drag,
                    };

                    items.push(
                        MainWindow::row_element(
                            panel_index,
                            &data,
                            index,
                            selected,
                            current == Some(index),
                            active,
                            columns,
                            rename_input,
                            &c,
                            weak.clone(),
                        )
                        .into_any_element(),
                    );
                }
                items
            },
        )
        .flex_1()
        .min_w(px(0.))
        .bg(c.bg)
        .track_scroll(&panel.list_scroll)
        .drag_over::<ExternalPaths>(|style, _paths, _window, cx| {
            style.bg(Colors::new(cx.theme()).accent.opacity(0.10))
        })
        .drag_over::<DragFiles>(|style, _files, _window, cx| {
            style.bg(Colors::new(cx.theme()).accent.opacity(0.10))
        })
        .drag_over::<DragArchive>(|style, _archive, _window, cx| {
            style.bg(Colors::new(cx.theme()).accent.opacity(0.10))
        });

        self.list_container(panel_index, cx, list)
    }

    /// «Список» (Ctrl+3): одна колонка, плашка и имя.
    pub(crate) fn plain_list(&self, panel_index: usize, cx: &mut Context<Self>) -> AnyElement {
        let c = Colors::new(cx.theme());
        let panel = self.panel(panel_index);
        let entries = panel.entries.clone();
        let total = entries.len();
        let selection = panel.selection.clone();
        let current = panel.active;
        let is_archive = panel.is_archive;
        let base_path = panel.path.clone();

        let selected_drag: Option<std::rc::Rc<Vec<(PathBuf, bool)>>> =
            if !is_archive && panel.selection.len() > 1 {
                Some(std::rc::Rc::new(
                    panel
                        .selection
                        .iter()
                        .filter_map(|index| entries.get(*index))
                        .map(|entry| (base_path.join(&entry.name), entry.is_dir))
                        .collect(),
                ))
            } else {
                None
            };

        let archive_prefix = panel.prefix.clone();
        let encrypted = panel.encrypted;
        let selected_entries: Option<std::rc::Rc<Vec<String>>> =
            if is_archive && panel.selection.len() > 1 {
                Some(std::rc::Rc::new(
                    panel
                        .selection
                        .iter()
                        .filter_map(|index| entries.get(*index))
                        .map(|entry| format!("{archive_prefix}{}", entry.name))
                        .collect(),
                ))
            } else {
                None
            };

        let weak = cx.entity().downgrade();
        let icons = self.icons.clone();
        let active = !self.two_panel || self.active_panel == panel_index;
        // Строка, которая сейчас правится: её отрисуем с полем ввода.
        let rename = self
            .rename
            .as_ref()
            .filter(|edit| edit.panel == panel_index)
            .map(|edit| (edit.index, edit.input.clone()));

        let list = uniform_list(
            ("file-list", panel_index),
            total,
            move |range, _window, _cx| {
                let mut items: Vec<AnyElement> = Vec::with_capacity(range.len());
                for index in range {
                    let Some(entry) = entries.get(index) else {
                        continue;
                    };
                    let selected = selection.contains(&index);
                    let rename_input = rename
                        .as_ref()
                        .and_then(|(target, input)| (*target == index).then(|| input.clone()));

                    let drag = if is_archive {
                        None
                    } else if selected && selected_drag.is_some() {
                        selected_drag.clone()
                    } else {
                        Some(std::rc::Rc::new(vec![(
                            base_path.join(&entry.name),
                            entry.is_dir,
                        )]))
                    };

                    let archive_drag = is_archive.then(|| ArchiveDragData {
                        archive: base_path.clone(),
                        entries: if selected && selected_entries.is_some() {
                            selected_entries.clone().unwrap_or_default()
                        } else {
                            std::rc::Rc::new(vec![format!("{archive_prefix}{}", entry.name)])
                        },
                        encrypted,
                    });

                    let data = RowData {
                        name: entry.name.clone(),
                        size: if entry.is_dir {
                            String::new()
                        } else {
                            entry.size_text()
                        },
                        modified: entry.modified.clone().unwrap_or_default(),
                        created: entry.created.clone().unwrap_or_default(),
                        is_dir: entry.is_dir,
                        is_archive: !entry.is_dir && is_archive_name(&entry.name),
                        icon: icons.lookup(
                            &entry.name,
                            entry.is_dir,
                            is_archive,
                            &base_path,
                            IconSize::Small,
                        ),
                        drag,
                        archive_drag,
                    };

                    items.push(
                        MainWindow::plain_row(
                            panel_index,
                            &data,
                            index,
                            selected,
                            current == Some(index),
                            active,
                            rename_input,
                            &c,
                            weak.clone(),
                        )
                        .into_any_element(),
                    );
                }
                items
            },
        )
        .flex_1()
        .min_w(px(0.))
        .bg(c.bg)
        .track_scroll(&panel.list_scroll)
        .drag_over::<ExternalPaths>(|style, _paths, _window, cx| {
            style.bg(Colors::new(cx.theme()).accent.opacity(0.10))
        })
        .drag_over::<DragFiles>(|style, _files, _window, cx| {
            style.bg(Colors::new(cx.theme()).accent.opacity(0.10))
        })
        .drag_over::<DragArchive>(|style, _archive, _window, cx| {
            style.bg(Colors::new(cx.theme()).accent.opacity(0.10))
        });

        self.list_container(panel_index, cx, list)
    }

    /// Сетка значков (Ctrl+1 — крупные, Ctrl+2 — мелкие).
    ///
    /// Виртуализация по строкам: одна строка `uniform_list` — ряд клеток,
    /// поэтому 2000 объектов рисуются так же быстро, как и таблица.
    pub(crate) fn grid_list(&self, panel_index: usize, cx: &mut Context<Self>) -> AnyElement {
        let c = Colors::new(cx.theme());
        let panel = self.panel(panel_index);
        let mode = panel.view_mode;
        let columns = panel.grid_columns().max(1);
        let tile_h = theme::tile_height(mode);
        let entries = panel.entries.clone();
        let total = entries.len();
        let rows = total.div_ceil(columns);
        let selection = panel.selection.clone();
        let current = panel.active;
        let is_archive = panel.is_archive;
        let base_path = panel.path.clone();

        let selected_drag: Option<std::rc::Rc<Vec<(PathBuf, bool)>>> =
            if !is_archive && panel.selection.len() > 1 {
                Some(std::rc::Rc::new(
                    panel
                        .selection
                        .iter()
                        .filter_map(|index| entries.get(*index))
                        .map(|entry| (base_path.join(&entry.name), entry.is_dir))
                        .collect(),
                ))
            } else {
                None
            };

        let archive_prefix = panel.prefix.clone();
        let encrypted = panel.encrypted;
        let selected_entries: Option<std::rc::Rc<Vec<String>>> =
            if is_archive && panel.selection.len() > 1 {
                Some(std::rc::Rc::new(
                    panel
                        .selection
                        .iter()
                        .filter_map(|index| entries.get(*index))
                        .map(|entry| format!("{archive_prefix}{}", entry.name))
                        .collect(),
                ))
            } else {
                None
            };

        let weak = cx.entity().downgrade();
        let icons = self.icons.clone();
        let active = !self.two_panel || self.active_panel == panel_index;
        // Клетка, которая правится, получает поле ввода вместо подписи.
        let rename = self
            .rename
            .as_ref()
            .filter(|edit| edit.panel == panel_index)
            .map(|edit| (edit.index, edit.input.clone()));

        let list = uniform_list(
            ("file-list", panel_index),
            rows,
            move |range, _window, _cx| {
                let mut items: Vec<AnyElement> = Vec::with_capacity(range.len());
                for row in range {
                    let mut line = h_flex().items_start().w_full().h(px(tile_h));
                    for column in 0..columns {
                        let index = row * columns + column;
                        let Some(entry) = entries.get(index) else {
                            break;
                        };
                        let selected = selection.contains(&index);
                        let rename_input = rename
                            .as_ref()
                            .and_then(|(target, input)| (*target == index).then(|| input.clone()));

                        let drag = if is_archive {
                            None
                        } else if selected && selected_drag.is_some() {
                            selected_drag.clone()
                        } else {
                            Some(std::rc::Rc::new(vec![(
                                base_path.join(&entry.name),
                                entry.is_dir,
                            )]))
                        };

                        let archive_drag = is_archive.then(|| ArchiveDragData {
                            archive: base_path.clone(),
                            entries: if selected && selected_entries.is_some() {
                                selected_entries.clone().unwrap_or_default()
                            } else {
                                std::rc::Rc::new(vec![format!("{archive_prefix}{}", entry.name)])
                            },
                            encrypted,
                        });

                        let data = RowData {
                            name: entry.name.clone(),
                            size: if entry.is_dir {
                                String::new()
                            } else {
                                entry.size_text()
                            },
                            modified: entry.modified.clone().unwrap_or_default(),
                            created: entry.created.clone().unwrap_or_default(),
                            is_dir: entry.is_dir,
                            is_archive: !entry.is_dir && is_archive_name(&entry.name),
                            icon: icons.lookup(
                                &entry.name,
                                entry.is_dir,
                                is_archive,
                                &base_path,
                                if mode == ViewMode::LargeIcons {
                                    IconSize::Large
                                } else {
                                    IconSize::Small
                                },
                            ),
                            drag,
                            archive_drag,
                        };

                        line = line.child(MainWindow::tile_element(
                            panel_index,
                            &data,
                            index,
                            selected,
                            current == Some(index),
                            active,
                            mode,
                            rename_input,
                            &c,
                            weak.clone(),
                        ));
                    }
                    items.push(line.into_any_element());
                }
                items
            },
        )
        .flex_1()
        .min_w(px(0.))
        .bg(c.bg)
        .track_scroll(&panel.list_scroll)
        .drag_over::<ExternalPaths>(|style, _paths, _window, cx| {
            style.bg(Colors::new(cx.theme()).accent.opacity(0.10))
        })
        .drag_over::<DragFiles>(|style, _files, _window, cx| {
            style.bg(Colors::new(cx.theme()).accent.opacity(0.10))
        })
        .drag_over::<DragArchive>(|style, _archive, _window, cx| {
            style.bg(Colors::new(cx.theme()).accent.opacity(0.10))
        });

        self.list_container(panel_index, cx, list)
    }

    /// Общая обвязка любого вида: мышь, рамка, приём файлов из проводника
    /// и полоса прокрутки. Вид передаёт только сам список.
    pub(crate) fn list_container(
        &self,
        panel_index: usize,
        cx: &mut Context<Self>,
        list: impl IntoElement,
    ) -> AnyElement {
        let c = Colors::new(cx.theme());
        let scroll = self.panel(panel_index).list_scroll.clone();
        // Контекстное меню строится китом; ему передаём снимок состояния и
        // `WeakEntity`, как верхнему меню. В момент открытия состояние
        // перечитываем: ПКМ успевает поставить выделение.
        let menu_state = self.menu_state();
        let menu_weak = cx.entity().downgrade();
        let renaming = self.rename.is_some();
        let container = div()
            .id(("list-container", panel_index))
            .drag_over::<DragFiles>(|style, _, _, cx| {
                style.bg(Colors::new(cx.theme()).blue.opacity(0.1))
            })
            .drag_over::<DragArchive>(|style, _, _, cx| {
                style.bg(Colors::new(cx.theme()).blue.opacity(0.1))
            })
            .drag_over::<ExternalPaths>(|style, _, _, cx| {
                style.bg(Colors::new(cx.theme()).blue.opacity(0.1))
            })
            .flex()
            .flex_row()
            .flex_1()
            .min_h(px(0.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    let panel = this.panel(panel_index);
                    // Клик по полю правки не должен уводить фокус из него:
                    // иначе постановка курсора сразу сохраняла бы имя. В сетке
                    // объект под курсором ищется по двум осям.
                    let on_rename_row = !panel.below_rows(event.position.y)
                        && this.rename.as_ref().is_some_and(|edit| {
                            edit.panel == panel_index
                                && panel.entry_at(event.position) == Some(edit.index)
                        });
                    if !on_rename_row {
                        this.activate_panel(panel_index);
                        this.sync_title(window);
                        window.focus(&this.focus, cx);
                        // Рамку заводим с любого места списка. Нажатие прямо
                        // на файл/папку (значок и имя) сюда не доходит: там
                        // начинается перетаскивание объекта.
                        this.panel_mut(panel_index)
                            .rubber_start(event.position, event.modifiers.control);
                    }
                    cx.notify();
                }),
            )
            // ПКМ по строке: невыбранную выделяем, у выбранной сохраняем
            // множественное выделение — как в 7-Zip. Само меню открывает
            // `ContextMenu` кита на следующем кадре.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if this.rename.is_some() {
                        return;
                    }
                    this.activate_panel(panel_index);
                    // Ниже последней строки записи нет: entry_at clamp'ит
                    // индекс, и без проверки меню открылось бы на «последнем файле».
                    let index = {
                        let panel = this.panel(panel_index);
                        (!panel.below_rows(event.position.y))
                            .then(|| panel.entry_at(event.position))
                            .flatten()
                    };
                    {
                        let panel = this.panel_mut(panel_index);
                        match index {
                            Some(index) => {
                                if !panel.selection.contains(&index) {
                                    panel.select_only(index);
                                }
                                panel.active = Some(index);
                            }
                            None if !event.modifiers.control && !event.modifiers.shift => {
                                panel.clear_selection();
                            }
                            None => {}
                        }
                    }
                    this.sync_title(window);
                    window.focus(&this.focus, cx);
                    cx.notify();
                }),
            )
            .on_mouse_move(
                cx.listener(move |this, event: &MouseMoveEvent, _window, cx| {
                    let panel = this.panel_mut(panel_index);
                    if panel.rubber.is_some() && event.dragging() {
                        panel.rubber_update(event.position);
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _event: &MouseUpEvent, _window, cx| {
                    // Нажатие было на пустом месте — щелчок без протяжки
                    // снимает выделение. Если начали на строке, клик обработает
                    // сама строка, и выделение трогать нельзя.
                    let panel = this.panel_mut(panel_index);
                    let on_empty = panel
                        .rubber
                        .as_ref()
                        .is_some_and(|band| band.started_on_empty);
                    let dragged = panel.rubber_finish();
                    if on_empty && !dragged {
                        panel.clear_selection();
                    }
                    cx.notify();
                }),
            )
            // Отпустили за пределами списка — иначе рамка залипла бы.
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(move |this, _event, _window, cx| {
                    this.panel_mut(panel_index).rubber_finish();
                    cx.notify();
                }),
            )
            // Файлы из проводника: подсветка при наведении и приём дропа.
            // События даёт платформа (OLE RegisterDragDrop на Windows): Entered
            // создаёт активный drag, Pending двигает его, Submit превращается
            // в mouse-up — без Entered дропа не будет вовсе.
            .on_drop::<ExternalPaths>(cx.listener(
                move |this, paths: &ExternalPaths, window, cx| {
                    this.activate_panel(panel_index);
                    this.sync_title(window);
                    // Перетаскивание из проводника/Finder/Nautilus — это
                    // копирование в папку (Shift — перемещение), а не открытие
                    // пути: одиночную папку тоже копируем.
                    let files: Vec<(PathBuf, bool)> = paths
                        .paths()
                        .iter()
                        .map(|path| (path.clone(), path.is_dir()))
                        .collect();
                    this.drop_files_on_panel(
                        &files,
                        panel_index,
                        window.mouse_position(),
                        window.modifiers().shift,
                        cx,
                    );
                    cx.notify();
                },
            ))
            // Внутреннее перетаскивание: файлы из другой панели копируются
            // сюда (Shift — перемещение). Это отдельный тип payload, поэтому
            // не конфликтует с приёмом файлов из проводника.
            .on_drop::<DragFiles>(cx.listener(move |this, payload: &DragFiles, window, cx| {
                this.activate_panel(panel_index);
                this.sync_title(window);
                this.drop_files_on_panel(
                    payload.files.as_slice(),
                    panel_index,
                    window.mouse_position(),
                    window.modifiers().shift,
                    cx,
                );
                cx.notify();
            }))
            // Записи, вытащенные из архива: сброс на папку распаковывает
            // именно их, а не весь архив.
            .on_drop::<DragArchive>(cx.listener(move |this, payload: &DragArchive, window, cx| {
                this.activate_panel(panel_index);
                this.sync_title(window);
                this.drop_archive_entries(
                    &payload.archive,
                    payload.entries.as_slice(),
                    payload.encrypted,
                    panel_index,
                    window.mouse_position(),
                    cx,
                );
                cx.notify();
            }))
            .child(
                // Полоса прокрутки лежит поверх списка, а не отдельной колонкой:
                // иначе её фон добавляет к разделителю панелей лишние 13 px и
                // стык выглядит толстым. Тянется она так же — кит синхронизирует
                // её с `UniformListScrollHandle`.
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .min_h(px(0.))
                    .child(list)
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .right_0()
                            .bottom_0()
                            .w(px(theme::scrollbar_w()))
                            .child(
                                Scrollbar::vertical(&scroll)
                                    .mode(ScrollbarMode::Always)
                                    .styles(|styles| {
                                        styles
                                            .track(|track| track.bg(c.bg.opacity(0.)))
                                            .thumb(|thumb| thumb.bg(c.muted.opacity(0.55)))
                                    }),
                            ),
                    ),
            );

        // Во время правки имени ПКМ не открываем: меню увело бы фокус из поля.
        if renaming {
            container.into_any_element()
        } else {
            container
                .context_menu(move |menu, window, cx| {
                    let state = menu_weak
                        .read_with(cx, |this, _| this.menu_state())
                        .unwrap_or(menu_state);
                    push_entries(menu, &CONTEXT_MENU, window, cx, &menu_weak, state)
                })
                .into_any_element()
        }
    }

    /// Строка вида «Список»: плашка типа и имя во всю ширину.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn plain_row(
        panel_index: usize,
        data: &RowData,
        index: usize,
        selected: bool,
        current: bool,
        active: bool,
        rename_input: Option<Entity<InputState>>,
        c: &Colors,
        weak: WeakEntity<MainWindow>,
    ) -> impl IntoElement {
        let renaming = rename_input.is_some();
        let full_name = data.name.clone();
        let hover = c.hover();
        // В неактивной панели выделение приглушено и без синей рамки.
        let selection_bg = if active {
            c.selected()
        } else {
            c.fg.opacity(0.12)
        };
        let current_border = if current && active {
            c.blue
        } else {
            c.bg.opacity(0.)
        };

        let name_part = match rename_input {
            Some(input) => div()
                .flex_1()
                .min_w(px(0.))
                .child(control_input(&input).w_full().appearance(false))
                .into_any_element(),
            None => div()
                .whitespace_nowrap()
                .overflow_hidden()
                .child(data.name.clone())
                .into_any_element(),
        };

        let mut row = h_flex()
            .id(("plain-row", index * 2 + panel_index))
            .tooltip(move |window, cx| Tooltip::new(full_name.clone()).build(window, cx))
            .items_center()
            .w_full()
            .h(px(theme::row_h()))
            .px_2()
            .gap_2()
            .rounded_sm()
            .border_1()
            .border_color(current_border)
            .when(selected, |this| this.bg(selection_bg))
            .when(!selected, |this| this.hover(move |style| style.bg(hover)));

        if data.is_dir || data.is_archive {
            row = row
                .drag_over::<DragFiles>(|style, _files, _window, cx| {
                    style.bg(Colors::new(cx.theme()).accent.opacity(0.35))
                })
                .drag_over::<ExternalPaths>(|style, _paths, _window, cx| {
                    style.bg(Colors::new(cx.theme()).accent.opacity(0.35))
                });
        }
        if data.is_dir {
            row = row.drag_over::<DragArchive>(|style, _archive, _window, cx| {
                style.bg(Colors::new(cx.theme()).accent.opacity(0.35))
            });
        }

        let weak_drag = weak.clone();
        let row = row
            .on_click(move |event, window, cx| {
                // Пока правится имя, клики по строке не трогаем: иначе клик по
                // полю ввода уводил бы фокус и завершал правку.
                if renaming {
                    return;
                }
                let count = event.click_count();
                let modifiers = event.modifiers();
                let dragged = match &event {
                    ClickEvent::Mouse(mouse) => {
                        let dx = f32::from(mouse.up.position.x - mouse.down.position.x);
                        let dy = f32::from(mouse.up.position.y - mouse.down.position.y);
                        dx.abs() > DRAG_THRESHOLD || dy.abs() > DRAG_THRESHOLD
                    }
                    _ => false,
                };
                let _ = weak.update(cx, |this, cx| {
                    this.row_clicked(panel_index, index, modifiers, count, dragged, window, cx);
                });
            })
            .child(entry_icon(
                data.icon.clone(),
                data.is_dir,
                data.is_archive,
                theme::ICON_DETAILS,
                c,
            ))
            .child(name_part);

        // Протяжка за строку: файлы на диске уходят системе, записи архива
        // принимают только наши панели (их сначала надо распаковать).
        if let Some(files) = &data.drag
            && !renaming
        {
            let label = if files.len() > 1 {
                format!("{} объекта", files.len())
            } else {
                data.name.clone()
            };
            let payload = DragFiles {
                label,
                files: files.clone(),
            };
            row.on_drag(payload, move |payload, _offset, _window, cx| {
                let _ = weak_drag.update(cx, |this, cx| {
                    this.activate_panel(panel_index);
                    this.select_for_drag(panel_index, index);
                    cx.notify();
                });
                cx.new(|_| payload.clone())
            })
            .external_drag_payload::<DragFiles>(|payload, _window, _cx| {
                Some(ExternalDragPayload::Files(FileDragPaths::new(
                    payload.files.iter().cloned(),
                )))
            })
        } else if let Some(archive) = &data.archive_drag
            && !renaming
        {
            let label = if archive.entries.len() > 1 {
                format!("{} объекта", archive.entries.len())
            } else {
                data.name.clone()
            };
            let payload = DragArchive {
                label,
                archive: archive.archive.clone(),
                entries: archive.entries.clone(),
                encrypted: archive.encrypted,
            };
            row.on_drag(payload, move |payload, _offset, _window, cx| {
                let _ = weak_drag.update(cx, |this, cx| {
                    this.activate_panel(panel_index);
                    this.select_for_drag(panel_index, index);
                    cx.notify();
                });
                cx.new(|_| payload.clone())
            })
        } else {
            row
        }
    }

    /// Клетка сетки значков: плашка-иконка и имя под ней.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn tile_element(
        panel_index: usize,
        data: &RowData,
        index: usize,
        selected: bool,
        current: bool,
        active: bool,
        mode: ViewMode,
        rename_input: Option<Entity<InputState>>,
        c: &Colors,
        weak: WeakEntity<MainWindow>,
    ) -> AnyElement {
        let renaming = rename_input.is_some();
        let full_name = data.name.clone();
        let icon = match mode {
            ViewMode::LargeIcons => theme::ICON_LARGE,
            _ => theme::ICON_SMALL,
        };
        let hover = c.hover();
        let width = theme::tile_width(mode);
        // В неактивной панели выделение приглушено и без синей рамки.
        let selection_bg = if active {
            c.selected()
        } else {
            c.fg.opacity(0.12)
        };
        let current_border = if current && active {
            c.blue
        } else {
            c.bg.opacity(0.)
        };

        // Поле ввода в клетке: у мелких значков иконку убираем — поле выше клетки.
        let name_part = match rename_input {
            Some(input) => div()
                .w(px(width - 8.))
                .child(control_input(&input).w_full().appearance(false))
                .into_any_element(),
            None => div()
                .max_w(px(width - 8.))
                .text_sm()
                .text_center()
                .whitespace_nowrap()
                .overflow_hidden()
                .child(data.name.clone())
                .into_any_element(),
        };

        let mut tile = div()
            .id(("tile", index * 2 + panel_index))
            .tooltip(move |window, cx| Tooltip::new(full_name.clone()).build(window, cx))
            .w(px(width))
            .h(px(theme::tile_height(mode)))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .rounded_sm()
            .border_1()
            .border_color(current_border)
            .when(selected, |this| this.bg(selection_bg))
            .when(!selected, |this| this.hover(move |style| style.bg(hover)));

        if data.is_dir || data.is_archive {
            tile = tile
                .drag_over::<DragFiles>(|style, _files, _window, cx| {
                    style.bg(Colors::new(cx.theme()).accent.opacity(0.35))
                })
                .drag_over::<ExternalPaths>(|style, _paths, _window, cx| {
                    style.bg(Colors::new(cx.theme()).accent.opacity(0.35))
                });
        }
        if data.is_dir {
            tile = tile.drag_over::<DragArchive>(|style, _archive, _window, cx| {
                style.bg(Colors::new(cx.theme()).accent.opacity(0.35))
            });
        }

        let weak_drag = weak.clone();
        let tile = tile
            .on_click(move |event, window, cx| {
                if renaming {
                    return;
                }
                let count = event.click_count();
                let modifiers = event.modifiers();
                let dragged = match &event {
                    ClickEvent::Mouse(mouse) => {
                        let dx = f32::from(mouse.up.position.x - mouse.down.position.x);
                        let dy = f32::from(mouse.up.position.y - mouse.down.position.y);
                        dx.abs() > DRAG_THRESHOLD || dy.abs() > DRAG_THRESHOLD
                    }
                    _ => false,
                };
                let _ = weak.update(cx, |this, cx| {
                    this.row_clicked(panel_index, index, modifiers, count, dragged, window, cx);
                });
            })
            .when(!renaming || mode == ViewMode::LargeIcons, |this| {
                this.child(entry_icon(
                    data.icon.clone(),
                    data.is_dir,
                    data.is_archive,
                    icon,
                    c,
                ))
            })
            .child(name_part);

        if let Some(files) = &data.drag
            && !renaming
        {
            let label = if files.len() > 1 {
                format!("{} объекта", files.len())
            } else {
                data.name.clone()
            };
            let payload = DragFiles {
                label,
                files: files.clone(),
            };
            tile.on_drag(payload, move |payload, _offset, _window, cx| {
                let _ = weak_drag.update(cx, |this, cx| {
                    this.activate_panel(panel_index);
                    this.select_for_drag(panel_index, index);
                    cx.notify();
                });
                cx.new(|_| payload.clone())
            })
            .external_drag_payload::<DragFiles>(|payload, _window, _cx| {
                Some(ExternalDragPayload::Files(FileDragPaths::new(
                    payload.files.iter().cloned(),
                )))
            })
            .into_any_element()
        } else if let Some(archive) = &data.archive_drag
            && !renaming
        {
            let label = if archive.entries.len() > 1 {
                format!("{} объекта", archive.entries.len())
            } else {
                data.name.clone()
            };
            let payload = DragArchive {
                label,
                archive: archive.archive.clone(),
                entries: archive.entries.clone(),
                encrypted: archive.encrypted,
            };
            tile.on_drag(payload, move |payload, _offset, _window, cx| {
                let _ = weak_drag.update(cx, |this, cx| {
                    this.activate_panel(panel_index);
                    this.select_for_drag(panel_index, index);
                    cx.notify();
                });
                cx.new(|_| payload.clone())
            })
            .into_any_element()
        } else {
            tile.into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Columns;

    #[test]
    fn narrow_panel_keeps_name_visible() {
        // Половина окна 1000 px: влезают только имя, размер и «Изменён».
        let columns = Columns::for_width(500.);
        assert!(!columns.created);
        assert!(!columns.comment);
        assert!(!columns.dirs);
        assert!(!columns.files);
    }

    #[test]
    fn wide_panel_shows_all_columns() {
        let columns = Columns::for_width(1000.);
        assert!(columns.created);
        assert!(columns.comment);
        assert!(columns.dirs);
        assert!(columns.files);
    }
}

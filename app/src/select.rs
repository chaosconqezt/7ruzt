use crate::prelude::*;
pub(crate) const DRAG_THRESHOLD: f32 = 4.0;

/// Протяжка мышью по списку — рамка выделения, как в проводнике и 7-Zip FM.
///
/// Выделение меняется только после сдвига на [`DRAG_THRESHOLD`]: до этого это
/// обычный клик, и его обрабатывает строка списка.
pub(crate) struct RubberBand {
    /// Где нажали и где мышь сейчас — в координатах окна.
    pub(crate) start: Point<Pixels>,
    pub(crate) current: Point<Pixels>,
    /// Строка под точкой нажатия. Привязана к содержимому: автопрокрутка
    /// не должна «утаскивать» начало рамки.
    pub(crate) anchor_row: Option<usize>,
    /// Y нажатия в координатах содержимого (без учёта прокрутки) — якорь рамки.
    pub(crate) content_y: f32,
    /// Выделение до протяжки: с Ctrl оно остаётся базой, без Ctrl — пусто.
    pub(crate) base: Arc<BTreeSet<usize>>,
    /// Нажатие было на пустом месте (ниже строк): клик по нему снимает выделение.
    pub(crate) started_on_empty: bool,
}

impl RubberBand {
    pub(crate) fn dragged(&self) -> bool {
        let dx = f32::from(self.current.x - self.start.x);
        let dy = f32::from(self.current.y - self.start.y);
        dx.abs() > DRAG_THRESHOLD || dy.abs() > DRAG_THRESHOLD
    }
}

impl Panel {
    pub(crate) fn clear_selection(&mut self) {
        Arc::make_mut(&mut self.selection).clear();
        self.active = None;
        self.anchor = None;
    }

    pub(crate) fn select_only(&mut self, index: usize) {
        let selection = Arc::make_mut(&mut self.selection);
        selection.clear();
        selection.insert(index);
        self.active = Some(index);
        self.anchor = Some(index);
    }

    pub(crate) fn toggle_selection(&mut self, index: usize) {
        let selection = Arc::make_mut(&mut self.selection);
        if !selection.remove(&index) {
            selection.insert(index);
        }
        self.active = Some(index);
        self.anchor = Some(index);
    }

    /// Shift: диапазон от якоря до курсора. Якорь не двигаем — как в проводнике.
    pub(crate) fn extend_selection(&mut self, index: usize) {
        let anchor = self.anchor.unwrap_or(index);
        let (from, to) = if anchor <= index {
            (anchor, index)
        } else {
            (index, anchor)
        };
        let selection = Arc::make_mut(&mut self.selection);
        selection.clear();
        for position in from..=to {
            selection.insert(position);
        }
        self.active = Some(index);
    }

    /// Grey +/-: добавить к выделению (или снять) всё, что совпало с маской.
    /// Курсор и якорь не двигаются — как в 7-Zip FM. При выделении возвращает
    /// число совпавших, при снятии — число реально снятых.
    pub(crate) fn select_by_mask(&mut self, mask: &str, deselect: bool) -> usize {
        let mut count = 0;
        let selection = Arc::make_mut(&mut self.selection);
        for (index, entry) in self.entries.iter().enumerate() {
            if mask_matches(&entry.name, mask) {
                if deselect {
                    if selection.remove(&index) {
                        count += 1;
                    }
                } else {
                    selection.insert(index);
                    count += 1;
                }
            }
        }
        count
    }

    /// Alt+Grey+/-: выделить/снять объекты того же типа, что и текущая строка.
    /// Тип файла — его расширение; для папки тип — «папка» (выделяются все папки).
    /// `None` — курсор ни на чём не стоит.
    pub(crate) fn select_by_type(&mut self, deselect: bool) -> Option<usize> {
        let (is_dir, extension) = {
            let current = self.active.and_then(|index| self.entries.get(index))?;
            (current.is_dir, file_extension(&current.name).to_lowercase())
        };
        let mut count = 0;
        let selection = Arc::make_mut(&mut self.selection);
        for (index, entry) in self.entries.iter().enumerate() {
            let same = if is_dir {
                entry.is_dir
            } else {
                !entry.is_dir && file_extension(&entry.name).to_lowercase() == extension
            };
            if same {
                count += 1;
                if deselect {
                    selection.remove(&index);
                } else {
                    selection.insert(index);
                }
            }
        }
        Some(count)
    }

    /// Нажатие в списке: начало возможной протяжки рамкой.
    ///
    /// Выделение пока не трогаем — его меняет [`Self::rubber_update`] после
    /// сдвига. Иначе клик и рамка работали бы одновременно и мигали.
    pub(crate) fn rubber_start(&mut self, position: Point<Pixels>, additive: bool) {
        let content_y = {
            let state = self.list_scroll.0.borrow();
            let bounds = state.base_handle.bounds();
            let offset = f32::from(state.base_handle.offset().y);
            f32::from(position.y) - f32::from(bounds.top()) - offset
        };
        let anchor_row = self.entry_at(position);
        let started_on_empty = self.below_rows(position.y);
        self.rubber = Some(RubberBand {
            start: position,
            current: position,
            anchor_row,
            content_y,
            base: if additive {
                self.selection.clone()
            } else {
                Arc::new(BTreeSet::new())
            },
            started_on_empty,
        });
    }

    /// Движение с зажатой кнопкой: строит выделение по строкам, которые накрыла рамка.
    pub(crate) fn rubber_update(&mut self, position: Point<Pixels>) {
        // Заём `self.rubber` закрываем сразу: дальше нужен весь `self`
        // (прокрутка и выделение) — одновременно их не одолжить.
        let Some(dragged) = self.rubber.as_mut().map(|band| {
            band.current = position;
            band.dragged()
        }) else {
            return;
        };
        if !dragged {
            return;
        }

        // Тянем за край — прокручиваем список, иначе то, что не влезло в окно,
        // рамкой не выделить.
        self.rubber_auto_scroll(position.y);

        let Some(band) = self.rubber.as_ref() else {
            return;
        };
        let anchor = band.anchor_row;
        let base = band.base.clone();
        let Some(last) = self.entry_at(position) else {
            return;
        };

        self.selection = base;
        let is_grid = self.view_mode.is_grid();
        let cols = self.grid_columns().max(1);
        let total = self.entries.len();
        let selection = Arc::make_mut(&mut self.selection);
        if is_grid {
            let first = anchor.unwrap_or(last);
            for index in grid_selection_indices(first, last, cols, total) {
                selection.insert(index);
            }
        } else {
            let (from, to) = match anchor {
                Some(first) if first <= last => (first, last),
                Some(first) => (last, first),
                None => (last, last),
            };
            for index in from..=to {
                selection.insert(index);
            }
        }
        self.active = Some(last);
    }

    /// Прокрутка списка, когда рамку тянут к верхнему или нижнему краю.
    pub(crate) fn rubber_auto_scroll(&mut self, y: Pixels) {
        /// Насколько близко к краю начинается прокрутка.
        const EDGE: f32 = 14.0;

        let state = self.list_scroll.0.borrow();
        let handle = &state.base_handle;
        let bounds = handle.bounds();
        let offset = handle.offset();
        let max = handle.max_offset();
        let y = f32::from(y);
        let step = if self.view_mode.is_grid() {
            px(theme::tile_height(self.view_mode))
        } else {
            px(theme::row_h())
        };

        let next = if y < f32::from(bounds.top()) + EDGE {
            (offset.y + step).min(px(0.))
        } else if y > f32::from(bounds.bottom()) - EDGE {
            (offset.y - step).max(px(-f32::from(max.y)))
        } else {
            return;
        };
        handle.set_offset(point(offset.x, next));
    }

    /// Кнопка отпущена (в том числе за пределами списка — `mouse_up_out`).
    ///
    /// Возвращает `true`, если это была протяжка: по этому признаку клик по
    /// строке после отпускания не должен схлопывать выделение.
    pub(crate) fn rubber_finish(&mut self) -> bool {
        let dragged = self.rubber.take().is_some_and(|band| band.dragged());
        if dragged {
            // Shift+стрелка после протяжки должен тянуть от последней строки.
            self.anchor = self.active;
        }
        dragged
    }

    /// Точка ниже последней строки — пустое место под списком.
    ///
    /// Клик по нему снимает выделение, как в проводнике и 7-Zip FM.
    pub(crate) fn below_rows(&self, y: Pixels) -> bool {
        if self.entries.is_empty() {
            return true;
        }
        let state = self.list_scroll.0.borrow();
        let top = state.base_handle.bounds().top() + state.base_handle.offset().y;
        let height = if self.view_mode.is_grid() {
            let rows = self.entries.len().div_ceil(self.grid_columns().max(1));
            theme::tile_height(self.view_mode) * rows as f32
        } else {
            theme::row_h() * self.entries.len() as f32
        };
        y >= top + px(height)
    }

    /// Объект под точкой окна.
    ///
    /// В таблице и списке — по вертикали, в сетке значков ещё и по колонке.
    pub(crate) fn entry_at(&self, position: Point<Pixels>) -> Option<usize> {
        if self.entries.is_empty() {
            return None;
        }
        let state = self.list_scroll.0.borrow();
        let bounds = state.base_handle.bounds();
        let top = f32::from(bounds.top() + state.base_handle.offset().y);

        if self.view_mode.is_grid() {
            let columns = self.grid_columns().max(1);
            let tile_w = theme::tile_width(self.view_mode);
            let tile_h = theme::tile_height(self.view_mode);
            let row = ((f32::from(position.y) - top) / tile_h).floor().max(0.0) as usize;
            let column = ((f32::from(position.x) - f32::from(bounds.left())) / tile_w)
                .floor()
                .max(0.0) as usize;
            let index = row * columns + column.min(columns - 1);
            Some(index.min(self.entries.len() - 1))
        } else {
            let index = ((f32::from(position.y) - top) / theme::row_h()).floor();
            Some(index.clamp(0.0, (self.entries.len() - 1) as f32) as usize)
        }
    }

    /// Прямоугольник рамки протяжки в координатах окна.
    ///
    /// Рисуем его в корне окна: там координаты совпадают с оконными, и не нужно
    /// пересчитывать их относительно списка. За границы списка не вылезаем.
    pub(crate) fn rubber_rect(&self) -> Option<(f32, f32, f32, f32)> {
        let band = self.rubber.as_ref().filter(|band| band.dragged())?;
        let state = self.list_scroll.0.borrow();
        let bounds = state.base_handle.bounds();
        // Вертикаль считаем от содержимого: при автопрокрутке начало рамки
        // уезжает вместе со строками, а не остаётся на экране.
        let anchor_y =
            f32::from(bounds.top()) + f32::from(state.base_handle.offset().y) + band.content_y;
        let current_y = f32::from(band.current.y);

        let x0 = f32::from(band.start.x).min(f32::from(band.current.x));
        let x1 = f32::from(band.start.x).max(f32::from(band.current.x));
        let y0 = anchor_y.min(current_y);
        let y1 = anchor_y.max(current_y);

        let left = x0.max(f32::from(bounds.left()));
        let top = y0.max(f32::from(bounds.top()));
        let right = x1.min(f32::from(bounds.right()));
        let bottom = y1.min(f32::from(bounds.bottom()));
        (right > left && bottom > top).then_some((left, top, right - left, bottom - top))
    }
}

impl MainWindow {
    /// Строка, за которую начали тянуть: если она не выделена, выделяем её.
    ///
    /// Выделенный набор не трогаем — иначе Ctrl-выбор схлопнулся бы в одну
    /// строку в момент начала перетаскивания.
    pub(crate) fn select_for_drag(&mut self, panel_index: usize, index: usize) {
        let panel = self.panel_mut(panel_index);
        if !panel.selection.contains(&index) {
            panel.select_only(index);
            panel.active = Some(index);
        }
    }

    /// Grey +/-: диалог маски. В отличие от «Создать папку», работает и внутри
    /// архива: выделить файлы архива для распаковки — основной сценарий.
    pub(crate) fn open_select_mask(
        &mut self,
        deselect: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = if deselect {
            PromptAction::DeselectMask
        } else {
            PromptAction::SelectMask
        };
        // «*» сразу выделено: маска печатается поверх, Enter на «*» выделяет всё.
        let input = cx.new(|cx| InputState::new(window, cx).default_value("*"));
        input.update(cx, |state, cx| state.set_selected_range(0..1, cx));

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

    /// Enter в диалоге маски: применяем к активной панели и закрываем диалог.
    /// Пустая маска — ошибка в самом диалоге, а не молчаливый ноль.
    pub(crate) fn submit_select_mask(&mut self, mut dialog: PromptDialog, mask: &str) {
        if mask.is_empty() {
            dialog.error = Some("Введите маску: например, *.rs".to_string());
            self.modal = Some(Modal::Prompt(dialog));
            return;
        }
        let deselect = matches!(dialog.action, PromptAction::DeselectMask);
        let count = self.active_mut().select_by_mask(mask, deselect);
        self.notice = Some(match (deselect, count) {
            (false, 0) => format!("По маске «{mask}» ничего не найдено"),
            (true, 0) => format!("По маске «{mask}» нечего снимать"),
            (false, _) => format!("Выделено по маске: {count}"),
            (true, _) => format!("Снято выделение по маске: {count}"),
        });
    }

    /// Alt+Grey+/-: выделение по типу текущей строки.
    pub(crate) fn select_same_type(&mut self, deselect: bool) {
        match self.active_mut().select_by_type(deselect) {
            Some(count) => {
                self.notice = Some(if deselect {
                    format!("Снято выделение по типу: {count}")
                } else {
                    format!("Выделено по типу: {count}")
                });
            }
            None => {
                self.notice = Some("Курсор ни на чём не стоит: встаньте на файл".to_string());
            }
        }
    }
}

/// 2D-прямоугольник индексов в сетке значков от `first` до `last`.
pub(crate) fn grid_selection_indices(
    first: usize,
    last: usize,
    cols: usize,
    total: usize,
) -> Vec<usize> {
    if cols == 0 || total == 0 {
        return Vec::new();
    }
    let (r1, c1) = (first / cols, first % cols);
    let (r2, c2) = (last / cols, last % cols);
    let (min_r, max_r) = (r1.min(r2), r1.max(r2));
    let (min_c, max_c) = (c1.min(c2), c1.max(c2));
    let mut indices = Vec::new();
    for r in min_r..=max_r {
        for c in min_c..=max_c {
            let index = r * cols + c;
            if index < total {
                indices.push(index);
            }
        }
    }
    indices
}

#[cfg(test)]
mod tests {
    use super::{Panel, grid_selection_indices};
    use crate::{Arc, Entry, PathBuf};

    fn panel_with(entries: Vec<Entry>) -> Panel {
        let mut panel = Panel::new(PathBuf::from("."));
        panel.entries = Arc::new(entries);
        panel
    }

    fn selected(panel: &Panel) -> Vec<usize> {
        panel.selection.iter().copied().collect()
    }

    #[test]
    fn mask_select_adds_to_existing_and_keeps_cursor() {
        let mut panel = panel_with(vec![
            Entry::dir("docs"),
            Entry::file("main.rs"),
            Entry::file("lib.rs"),
            Entry::file("note.txt"),
        ]);
        panel.select_only(3);
        assert_eq!(panel.select_by_mask("*.rs", false), 2);
        assert_eq!(selected(&panel), vec![1, 2, 3]);
        assert_eq!(panel.active, Some(3));
    }

    #[test]
    fn mask_deselect_removes_only_matching() {
        let mut panel = panel_with(vec![
            Entry::dir("docs"),
            Entry::file("main.rs"),
            Entry::file("lib.rs"),
            Entry::file("note.txt"),
        ]);
        panel.selection = Arc::new((0..4).collect());
        assert_eq!(panel.select_by_mask("*.rs", true), 2);
        assert_eq!(selected(&panel), vec![0, 3]);
    }

    #[test]
    fn mask_accepts_several_patterns_and_matches_dirs() {
        let mut panel = panel_with(vec![
            Entry::dir("docs"),
            Entry::file("main.rs"),
            Entry::file("note.txt"),
        ]);
        assert_eq!(panel.select_by_mask("*.rs docs", false), 2);
        assert_eq!(selected(&panel), vec![0, 1]);
    }

    #[test]
    fn type_selects_same_extension_ignoring_case() {
        let mut panel = panel_with(vec![
            Entry::file("a.rs"),
            Entry::file("b.RS"),
            Entry::file("c.txt"),
            Entry::dir("d.rs"),
            Entry::file("README"),
        ]);
        panel.active = Some(0);
        assert_eq!(panel.select_by_type(false), Some(2));
        assert_eq!(selected(&panel), vec![0, 1]);
    }

    #[test]
    fn type_on_dir_selects_all_dirs() {
        let mut panel = panel_with(vec![Entry::dir("a"), Entry::file("b.txt"), Entry::dir("c")]);
        panel.active = Some(0);
        assert_eq!(panel.select_by_type(false), Some(2));
        assert_eq!(selected(&panel), vec![0, 2]);
    }

    #[test]
    fn type_groups_extensionless_files() {
        let mut panel = panel_with(vec![
            Entry::file("README"),
            Entry::file("LICENSE"),
            Entry::file("a.txt"),
        ]);
        panel.active = Some(0);
        assert_eq!(panel.select_by_type(false), Some(2));
        assert_eq!(selected(&panel), vec![0, 1]);
    }

    #[test]
    fn type_deselect_and_missing_cursor() {
        let mut panel = panel_with(vec![Entry::file("a.rs"), Entry::file("b.rs")]);
        assert_eq!(panel.select_by_type(false), None);
        panel.selection = Arc::new((0..2).collect());
        panel.active = Some(1);
        assert_eq!(panel.select_by_type(true), Some(2));
        assert!(panel.selection.is_empty());
    }

    #[test]
    fn grid_selection_selects_2d_rectangle_not_snake() {
        // Сетка 4 колонки:
        // 0  1  2  3
        // 4  5  6  7
        // 8  9 10 11
        // Рамка от 1 (ряд 0, кол 1) до 6 (ряд 1, кол 2).
        // Должны выбраться: 1, 2 и 5, 6 (прямоугольник).
        // 3 и 4 не должны попасть.
        let indices = grid_selection_indices(1, 6, 4, 12);
        assert_eq!(indices, vec![1, 2, 5, 6]);
    }

    #[test]
    fn grid_selection_reverse_drag() {
        // Тянем снизу вверх справа налево: от 6 до 1.
        let indices = grid_selection_indices(6, 1, 4, 12);
        assert_eq!(indices, vec![1, 2, 5, 6]);
    }
}

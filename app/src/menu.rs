use crate::prelude::*;
/// Верхние меню — как в 7-Zip FM и в макете.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TopMenu {
    File,
    Edit,
    View,
    Favorites,
    Service,
    Help,
}

impl TopMenu {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::File => "Файл",
            Self::Edit => "Правка",
            Self::View => "Вид",
            Self::Favorites => "Избранное",
            Self::Service => "Сервис",
            Self::Help => "Справка",
        }
    }

    pub(crate) fn index(self) -> usize {
        match self {
            Self::File => 0,
            Self::Edit => 1,
            Self::View => 2,
            Self::Favorites => 3,
            Self::Service => 4,
            Self::Help => 5,
        }
    }
}

/// Что делает пункт меню. `None` в описании пункта означает «пока не работает».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuAction {
    Quit,
    Open,
    ExtractArchive,
    CreateArchive,
    SelectAll,
    ClearSelection,
    InvertSelection,
    SelectMask,
    DeselectMask,
    SelectByType,
    DeselectByType,
    Refresh,
    GoUp,
    GoRoot,
    SortName,
    SortType,
    SortSize,
    SortModified,
    ViewLargeIcons,
    ViewSmallIcons,
    ViewList,
    ViewDetails,
    TwoPanels,
    OpenExternal,
    ViewFile,
    EditFile,
    Properties,
    Rename,
    Delete,
    CopyTo,
    MoveTo,
    CreateFolder,
    CreateFile,
    TestArchive,
    Benchmark,
    Theme(ThemePreference),
    Font(FontScale),
    Hash(HashAlgorithm),
    About,
}

/// Пункт меню: подпись, горячая клавиша (справа, как в 7-Zip), действие и,
/// если это подменю, его содержимое.
///
/// Пункт без действия — ещё не реализованный: в меню он стоит с «✕» в колонке
/// маркера и погашен, чтобы было видно, что это не поломка, а неготовое.
pub(crate) struct MenuEntry {
    pub(crate) label: &'static str,
    pub(crate) shortcut: &'static str,
    pub(crate) action: Option<MenuAction>,
    pub(crate) children: &'static [MenuEntry],
    pub(crate) separator: bool,
}

pub(crate) const fn item(
    label: &'static str,
    shortcut: &'static str,
    action: MenuAction,
) -> MenuEntry {
    MenuEntry {
        label,
        shortcut,
        action: Some(action),
        children: &[],
        separator: false,
    }
}

pub(crate) const fn soon(label: &'static str, shortcut: &'static str) -> MenuEntry {
    MenuEntry {
        label,
        shortcut,
        action: None,
        children: &[],
        separator: false,
    }
}

/// Подменю: пункт раскрывается в свой попап (например, «Файл → CRC»).
pub(crate) const fn submenu(label: &'static str, children: &'static [MenuEntry]) -> MenuEntry {
    MenuEntry {
        label,
        shortcut: "",
        action: None,
        children,
        separator: false,
    }
}

const SEPARATOR: MenuEntry = MenuEntry {
    label: "",
    shortcut: "",
    action: None,
    children: &[],
    separator: true,
};

// Массивы держим константами: `&[item(..)]` — временный массив, его нельзя вернуть
// как `&'static`.
//
// Состав меню взят из макета (полный набор 7-Zip FM). Работает пока немного,
// но список не выкидываем: видно, что впереди, и не надо переделывать раскладку.

/// CRC — подменю «Файл → CRC», состав как в 7-Zip FM.
const CRC_SUBMENU: [MenuEntry; 11] = [
    item("CRC-32", "", MenuAction::Hash(HashAlgorithm::Crc32)),
    item("CRC-64", "", MenuAction::Hash(HashAlgorithm::Crc64)),
    item("XXH64", "", MenuAction::Hash(HashAlgorithm::Xxh64)),
    item("MD5", "", MenuAction::Hash(HashAlgorithm::Md5)),
    item("SHA-1", "", MenuAction::Hash(HashAlgorithm::Sha1)),
    item("SHA-256", "", MenuAction::Hash(HashAlgorithm::Sha256)),
    item("SHA-384", "", MenuAction::Hash(HashAlgorithm::Sha384)),
    item("SHA-512", "", MenuAction::Hash(HashAlgorithm::Sha512)),
    item("SHA3-256", "", MenuAction::Hash(HashAlgorithm::Sha3_256)),
    item("BLAKE2sp", "", MenuAction::Hash(HashAlgorithm::Blake2sp)),
    item("*", "", MenuAction::Hash(HashAlgorithm::All)),
];

const FILE_MENU: [MenuEntry; 27] = [
    item("Открыть", "Enter", MenuAction::Open),
    soon("Открыть внутри", "Ctrl+PgDn"),
    soon("Открыть внутри *", ""),
    soon("Открыть внутри #", ""),
    item("Открыть снаружи", "Shift+Enter", MenuAction::OpenExternal),
    item("Просмотр", "F3", MenuAction::ViewFile),
    item("Редактировать", "F4", MenuAction::EditFile),
    SEPARATOR,
    item("Переименовать", "F2", MenuAction::Rename),
    item("Копировать в...", "F5", MenuAction::CopyTo),
    item("Переместить в...", "F6", MenuAction::MoveTo),
    item("Удалить", "Del", MenuAction::Delete),
    SEPARATOR,
    soon("Разбить файл...", ""),
    soon("Объединить файлы...", ""),
    SEPARATOR,
    item("Свойства", "Alt+Enter", MenuAction::Properties),
    soon("Комментарий...", "Ctrl+Z"),
    submenu("CRC", &CRC_SUBMENU),
    SEPARATOR,
    item("Создать Папку", "F7", MenuAction::CreateFolder),
    item("Создать Файл", "Ctrl+N", MenuAction::CreateFile),
    SEPARATOR,
    soon("Ссылка", ""),
    soon("Альтернативные Потоки", ""),
    SEPARATOR,
    item("Выход", "Alt+F4", MenuAction::Quit),
];

/// Контекстное меню списка (ПКМ). Набор — как в «Файл», но без «Выход» и с
/// извлечением: по ПКМ на архиве это основной путь к распаковке, как в 7-Zip FM.
pub(crate) const CONTEXT_MENU: [MenuEntry; 22] = [
    item("Открыть", "Enter", MenuAction::Open),
    item("Извлечь...", "", MenuAction::ExtractArchive),
    item("Проверить архив", "", MenuAction::TestArchive),
    item("Добавить к архиву...", "", MenuAction::CreateArchive),
    item("Открыть снаружи", "Shift+Enter", MenuAction::OpenExternal),
    item("Просмотр", "F3", MenuAction::ViewFile),
    item("Редактировать", "F4", MenuAction::EditFile),
    SEPARATOR,
    item("Переименовать", "F2", MenuAction::Rename),
    item("Копировать в...", "F5", MenuAction::CopyTo),
    item("Переместить в...", "F6", MenuAction::MoveTo),
    item("Удалить", "Del", MenuAction::Delete),
    SEPARATOR,
    item("Свойства", "Alt+Enter", MenuAction::Properties),
    soon("Комментарий...", "Ctrl+Z"),
    submenu("CRC", &CRC_SUBMENU),
    SEPARATOR,
    item("Создать Папку", "F7", MenuAction::CreateFolder),
    item("Создать Файл", "Ctrl+N", MenuAction::CreateFile),
    SEPARATOR,
    soon("Ссылка", ""),
    soon("Альтернативные Потоки", ""),
];

const EDIT_MENU: [MenuEntry; 8] = [
    item("Выделить все", "Shift+[Grey +]", MenuAction::SelectAll),
    item(
        "Убрать выделение",
        "Shift+[Grey -]",
        MenuAction::ClearSelection,
    ),
    item("Обратить выделение", "Grey *", MenuAction::InvertSelection),
    item("Выделить...", "Grey +", MenuAction::SelectMask),
    item("Убрать выделение...", "Grey -", MenuAction::DeselectMask),
    SEPARATOR,
    item("Выделить по типу", "Alt+[Grey+]", MenuAction::SelectByType),
    item(
        "Убрать выделение по типу",
        "Alt+[Grey-]",
        MenuAction::DeselectByType,
    ),
];

/// «Панели инструментов»: у нас панели не настраиваются.
const TOOLBAR_SUBMENU: [MenuEntry; 2] = [soon("Стандартная панель", ""), soon("Панель адреса", "")];

/// «Вид → Тема». Системная стоит первой и по умолчанию: она повторяет
/// оформление ОС и на всех платформах обновляется при его смене.
const THEME_SUBMENU: [MenuEntry; 10] = [
    item("Системная", "", MenuAction::Theme(ThemePreference::System)),
    item("Светлая", "", MenuAction::Theme(ThemePreference::Light)),
    item("Тёмная", "", MenuAction::Theme(ThemePreference::Carbon)),
    item("Блэкаут", "", MenuAction::Theme(ThemePreference::Dark)),
    item("Графит", "", MenuAction::Theme(ThemePreference::Graphite)),
    item("Серебро", "", MenuAction::Theme(ThemePreference::Silver)),
    item("Barbie", "", MenuAction::Theme(ThemePreference::Barbie)),
    item(
        "My Little Pony",
        "",
        MenuAction::Theme(ThemePreference::Pony),
    ),
    item("Гжель", "", MenuAction::Theme(ThemePreference::Gzhel)),
    item("Гуро", "", MenuAction::Theme(ThemePreference::Guro)),
];

/// «Вид → Шрифт»: один кегль на всё окно, средний — по умолчанию.
const FONT_SUBMENU: [MenuEntry; 3] = [
    item("Малый", "", MenuAction::Font(FontScale::Small)),
    item("Средний", "", MenuAction::Font(FontScale::Medium)),
    item("Большой", "", MenuAction::Font(FontScale::Large)),
];

const VIEW_MENU: [MenuEntry; 22] = [
    item("Крупные значки", "Ctrl+1", MenuAction::ViewLargeIcons),
    item("Мелкие значки", "Ctrl+2", MenuAction::ViewSmallIcons),
    item("Список", "Ctrl+3", MenuAction::ViewList),
    item("Таблица", "Ctrl+4", MenuAction::ViewDetails),
    SEPARATOR,
    item("Имя", "Ctrl+F3", MenuAction::SortName),
    item("Тип", "Ctrl+F4", MenuAction::SortType),
    item("Изменён", "Ctrl+F5", MenuAction::SortModified),
    item("Размер", "Ctrl+F6", MenuAction::SortSize),
    soon("Без сортировки", "Ctrl+F7"),
    SEPARATOR,
    soon("Плоский режим", ""),
    item("2 Панели", "F9", MenuAction::TwoPanels),
    submenu("Панели инструментов", &TOOLBAR_SUBMENU),
    item("Открыть корневую папку", "\\", MenuAction::GoRoot),
    item(
        "Переход на один уровень вверх",
        "Backspace",
        MenuAction::GoUp,
    ),
    soon("История папок...", "Alt+F12"),
    item("Обновить", "Ctrl+R", MenuAction::Refresh),
    soon("Автообновление", ""),
    submenu("Тема", &THEME_SUBMENU),
    submenu("Шрифт", &FONT_SUBMENU),
    SEPARATOR,
];

/// Избранное целиком впереди: ячейки Alt+0…Alt+9, как в 7-Zip.
const FAVORITES_SUBMENU: [MenuEntry; 10] = [
    soon("—", "Alt+0"),
    soon("—", "Alt+1"),
    soon("—", "Alt+2"),
    soon("—", "Alt+3"),
    soon("—", "Alt+4"),
    soon("—", "Alt+5"),
    soon("—", "Alt+6"),
    soon("—", "Alt+7"),
    soon("—", "Alt+8"),
    soon("—", "Alt+9"),
];

const FAVORITES_MENU: [MenuEntry; 1] = [submenu(
    "Добавить папку в избранное как",
    &FAVORITES_SUBMENU,
)];

const SERVICE_MENU: [MenuEntry; 5] = [
    item("Проверить архив", "", MenuAction::TestArchive),
    SEPARATOR,
    soon("Настройки...", ""),
    item("Тестирование производительности", "", MenuAction::Benchmark),
    soon("Удалить временные файлы...", ""),
];

const HELP_MENU: [MenuEntry; 2] = [
    soon("Содержание", "F1"),
    item("О программе 7-Zip...", "", MenuAction::About),
];

/// Снимок состояния окна на момент построения попапа.
///
/// Попап живёт в слое кита и нашего состояния не видит, поэтому всё, что ему
/// нужно знать, передаётся значением.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MenuState {
    pub(crate) can_work_with_archive: bool,
    pub(crate) sort_key: SortKey,
    pub(crate) view: ViewMode,
    /// Показана ли вторая панель — галочка у пункта «2 Панели».
    pub(crate) two_panel: bool,
    /// Выбранная тема — галочка в «Вид → Тема».
    pub(crate) theme: ThemePreference,
    /// Выбранный кегль — галочка в «Вид → Шрифт».
    pub(crate) font: FontScale,
    /// Есть выделение — от него зависят «Переименовать» и «Удалить».
    pub(crate) has_selection: bool,
    /// Мы внутри архива: операции с файлами диска недоступны.
    pub(crate) in_archive: bool,
    /// Идёт фоновая операция: файлы не трогаем.
    pub(crate) busy: bool,
}

pub(crate) fn menu_action_enabled(action: MenuAction, state: MenuState) -> bool {
    let file_action = !state.in_archive && !state.busy;
    match action {
        MenuAction::TestArchive | MenuAction::ExtractArchive => state.can_work_with_archive,
        // Бенчмарк не зависит от архива, но не мешаем идущей файловой операции.
        MenuAction::Benchmark => !state.busy,
        MenuAction::Rename | MenuAction::Delete => file_action && state.has_selection,
        MenuAction::CopyTo | MenuAction::MoveTo => file_action && state.has_selection,
        // Просмотр и открытие снаружи работают и внутри архива (извлечение
        // во временную папку) — прятать готовое нельзя.
        MenuAction::OpenExternal | MenuAction::ViewFile => state.has_selection,
        MenuAction::EditFile | MenuAction::Properties => !state.in_archive && state.has_selection,
        MenuAction::CreateFolder | MenuAction::CreateFile | MenuAction::CreateArchive => {
            file_action
        }
        // Курсор на файле тоже считается: без выделения 7-Zip берёт текущий.
        MenuAction::Hash(_) => file_action,
        _ => true,
    }
}

/// Галочка в меню: какая сортировка или вид сейчас выбраны.
pub(crate) fn menu_action_checked(action: MenuAction, state: MenuState) -> bool {
    match action {
        MenuAction::SortName => state.sort_key == SortKey::Name,
        MenuAction::SortType => state.sort_key == SortKey::Type,
        MenuAction::SortSize => state.sort_key == SortKey::Size,
        MenuAction::SortModified => state.sort_key == SortKey::Modified,
        MenuAction::ViewLargeIcons => state.view == ViewMode::LargeIcons,
        MenuAction::ViewSmallIcons => state.view == ViewMode::SmallIcons,
        MenuAction::ViewList => state.view == ViewMode::List,
        MenuAction::ViewDetails => state.view == ViewMode::Details,
        MenuAction::TwoPanels => state.two_panel,
        MenuAction::Theme(preference) => state.theme == preference,
        MenuAction::Font(scale) => state.font == scale,
        _ => false,
    }
}

pub(crate) fn menu_entries(menu: TopMenu) -> &'static [MenuEntry] {
    match menu {
        TopMenu::File => &FILE_MENU,
        TopMenu::Edit => &EDIT_MENU,
        TopMenu::View => &VIEW_MENU,
        TopMenu::Favorites => &FAVORITES_MENU,
        TopMenu::Service => &SERVICE_MENU,
        TopMenu::Help => &HELP_MENU,
    }
}

impl MainWindow {
    /// Доступна ли кнопка в текущем состоянии окна.
    ///
    /// Погашенная кнопка честнее молчащего клика: пока открыт каталог, извлекать
    /// и проверять нечего, а пока открыт архив — нечего архивировать.
    pub(crate) fn tool_enabled(&self, action: ToolAction) -> bool {
        let panel = self.active();
        match action {
            ToolAction::Add => !panel.is_archive,
            ToolAction::Extract | ToolAction::Test => self.action_archive().is_some(),
            ToolAction::Delete => !panel.is_archive && !panel.selection.is_empty(),
            ToolAction::Copy | ToolAction::Move => !panel.is_archive && !panel.selection.is_empty(),
            // У операции без своей кнопки (хеширование) отмена — в окне прогресса.
            ToolAction::None => false,
            // Свойства рисует оболочка Windows: до файлов внутри архива она
            // не достаёт, поэтому там кнопка гаснет.
            ToolAction::Info => !panel.is_archive && !panel.selection.is_empty(),
        }
    }

    /// Архив, с которым работают «Извлечь» и «Проверить».
    ///
    /// Внутри архива это он сам. В каталоге — выделенный файл, если он похож
    /// на архив: открывать его двойным щелчком ради проверки не нужно.
    /// Несколько архивов сразу пока не поддерживаем: это отдельный сценарий
    /// с пачкой результатов, и угадывать, какой из них главный, нельзя.
    pub(crate) fn action_archive(&self) -> Option<PathBuf> {
        let panel = self.active();
        if panel.is_archive {
            return Some(panel.path.clone());
        }

        let mut archives = panel
            .selection
            .iter()
            .filter_map(|index| panel.entries.get(*index))
            .filter(|entry| !entry.is_dir && is_archive_name(&entry.name))
            .map(|entry| panel.path.join(&entry.name));

        let archive = archives.next()?;
        archives.next().is_none().then_some(archive)
    }

    /// Что доступно из верхнего меню прямо сейчас.
    pub(crate) fn menu_state(&self) -> MenuState {
        let panel = self.active();
        MenuState {
            can_work_with_archive: self.job.is_none() && self.action_archive().is_some(),
            sort_key: panel.sort_key,
            view: panel.view_mode,
            two_panel: self.two_panel,
            theme: self.theme_pref,
            font: self.font_scale,
            has_selection: !panel.selection.is_empty(),
            in_archive: panel.is_archive,
            busy: self.job.is_some(),
        }
    }

    /// Нажатие кнопки тулбара. Кнопка идущей операции работает как «Отмена».
    pub(crate) fn run_tool_action(
        &mut self,
        action: ToolAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .job
            .as_ref()
            .is_some_and(|job| job.kind.tool_action() == action)
        {
            self.cancel_job();
            return;
        }

        match action {
            ToolAction::Add => self.open_create_dialog(window, cx),
            ToolAction::Extract => self.open_extract_dialog(window, cx),
            ToolAction::Test => self.test_current(window, cx),
            ToolAction::Delete => self.confirm_delete(window, cx),
            ToolAction::Copy => self.open_transfer(TransferKind::Copy, window, cx),
            ToolAction::Move => self.open_transfer(TransferKind::Move, window, cx),
            ToolAction::Info => self.open_entry_with(ShellVerb::Properties, window, cx),
            ToolAction::None => {}
        }
    }

    pub(crate) fn run_action(
        &mut self,
        action: MenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            MenuAction::Quit => {
                cx.quit();
                return;
            }
            MenuAction::Open => {
                self.open_selected(window, cx);
                self.sync_title(window);
            }
            MenuAction::ExtractArchive => self.open_extract_dialog(window, cx),
            MenuAction::CreateArchive => self.open_create_dialog(window, cx),
            MenuAction::SelectAll => {
                let panel = self.active_mut();
                panel.selection = Arc::new((0..panel.entries.len()).collect());
                panel.active = panel.entries.len().checked_sub(1);
                panel.anchor = Some(0);
            }
            MenuAction::ClearSelection => self.active_mut().clear_selection(),
            MenuAction::InvertSelection => {
                let panel = self.active_mut();
                let count = panel.entries.len();
                panel.selection = Arc::new(
                    (0..count)
                        .filter(|index| !panel.selection.contains(index))
                        .collect(),
                );
            }
            MenuAction::SelectMask => self.open_select_mask(false, window, cx),
            MenuAction::DeselectMask => self.open_select_mask(true, window, cx),
            MenuAction::SelectByType => self.select_same_type(false),
            MenuAction::DeselectByType => self.select_same_type(true),
            MenuAction::Refresh => {
                let index = self.active_panel;
                self.reload_panel(index, cx, move |this, _| {
                    this.notice = Some(format!("Обновлено за {} мс", this.panels[index].load_ms));
                });
            }
            MenuAction::GoUp => {
                self.go_up(self.active_panel, cx);
                self.sync_title(window);
            }
            MenuAction::GoRoot => {
                let index = self.active_panel;
                let is_archive = self.active().is_archive;
                let panel = self.active_mut();
                if is_archive {
                    panel.prefix.clear();
                    panel.clear_selection();
                    panel.rebuild_view();
                } else if let Some(root) = panel.path.ancestors().last() {
                    panel.path = root.to_path_buf();
                    self.reload_panel(index, cx, |_, _| {});
                }
                if is_archive {
                    self.prefetch_icons(index, cx);
                }
                self.sync_title(window);
            }
            MenuAction::SortName => self.sort_by(SortKey::Name),
            MenuAction::SortType => self.sort_by(SortKey::Type),
            MenuAction::SortSize => self.sort_by(SortKey::Size),
            MenuAction::SortModified => self.sort_by(SortKey::Modified),
            MenuAction::ViewLargeIcons => self.active_mut().set_view(ViewMode::LargeIcons),
            MenuAction::ViewSmallIcons => self.active_mut().set_view(ViewMode::SmallIcons),
            MenuAction::ViewList => self.active_mut().set_view(ViewMode::List),
            MenuAction::ViewDetails => self.active_mut().set_view(ViewMode::Details),
            MenuAction::TwoPanels => {
                self.toggle_two_panels(cx);
                self.sync_title(window);
            }
            MenuAction::OpenExternal => self.open_entry_with(ShellVerb::Open, window, cx),
            MenuAction::ViewFile => self.open_entry_with(ShellVerb::Open, window, cx),
            MenuAction::EditFile => self.open_entry_with(ShellVerb::Edit, window, cx),
            MenuAction::Properties => self.open_entry_with(ShellVerb::Properties, window, cx),
            MenuAction::Hash(algorithm) => self.start_hash(algorithm, cx),
            MenuAction::Rename => self.open_rename(window, cx),
            MenuAction::Delete => self.confirm_delete(window, cx),
            MenuAction::CopyTo => self.open_transfer(TransferKind::Copy, window, cx),
            MenuAction::MoveTo => self.open_transfer(TransferKind::Move, window, cx),
            MenuAction::CreateFolder => self.open_prompt(PromptAction::CreateFolder, window, cx),
            MenuAction::CreateFile => self.open_prompt(PromptAction::CreateFile, window, cx),
            MenuAction::TestArchive => self.test_current(window, cx),
            MenuAction::Benchmark => self.open_bench(cx),
            MenuAction::Theme(preference) => {
                self.theme_pref = preference;
                // Предупреждение о битом/неполном themes.json важнее подписи:
                // иначе человек видит «Тема: …», а на экране другая палитра.
                self.notice = theme::apply(preference, Some(window), cx)
                    .or_else(|| Some(format!("Тема: {}", preference.label())));
            }
            MenuAction::Font(scale) => {
                self.font_scale = scale;
                theme::set_font_scale(scale);
                // Кегль живёт в теме, поэтому после смены масштаба её надо
                // применить заново — иначе Root оставит старый rem.
                self.notice = theme::apply(self.theme_pref, Some(window), cx)
                    .or_else(|| Some(format!("Шрифт: {}", scale.label())));
            }
            MenuAction::About => self.modal = Some(Modal::About),
        }
        cx.notify();
    }

    /// Сортировка из меню: выбранная колонка, всегда по возрастанию.
    pub(crate) fn sort_by(&mut self, key: SortKey) {
        self.active_mut().set_sort(key, true);
    }
}

#[cfg(test)]
mod tests {
    // Не `super::*`: он тянет макрос `test` из prelude кита, и `#[test]`
    // уходит в рекурсию. Импортируем только нужные имена.
    use super::{MenuAction, MenuState, VIEW_MENU, menu_action_checked};
    use crate::prelude::{FontScale, SortKey, ThemePreference, ViewMode};

    fn state() -> MenuState {
        MenuState {
            can_work_with_archive: false,
            sort_key: SortKey::Name,
            view: ViewMode::Details,
            two_panel: false,
            theme: ThemePreference::System,
            font: FontScale::Medium,
            has_selection: false,
            in_archive: false,
            busy: false,
        }
    }

    #[test]
    fn view_menu_has_all_theme_choices() {
        let theme = VIEW_MENU
            .iter()
            .find(|entry| entry.label == "Тема")
            .expect("Вид → Тема");
        assert_eq!(theme.children.len(), 10);
        assert_eq!(
            theme.children[0].action,
            Some(MenuAction::Theme(ThemePreference::System))
        );
        assert_eq!(
            theme.children[2].action,
            Some(MenuAction::Theme(ThemePreference::Carbon))
        );
        assert_eq!(
            theme.children[3].action,
            Some(MenuAction::Theme(ThemePreference::Dark))
        );
        assert_eq!(
            theme.children[9].action,
            Some(MenuAction::Theme(ThemePreference::Guro))
        );
    }

    #[test]
    fn view_menu_has_font_sizes() {
        let font = VIEW_MENU
            .iter()
            .find(|entry| entry.label == "Шрифт")
            .expect("Вид → Шрифт");
        assert_eq!(font.children.len(), 3);
        assert_eq!(
            font.children[1].action,
            Some(MenuAction::Font(FontScale::Medium))
        );
        assert!(menu_action_checked(
            MenuAction::Font(FontScale::Medium),
            state()
        ));
        assert!(!menu_action_checked(
            MenuAction::Font(FontScale::Large),
            state()
        ));
    }

    #[test]
    fn theme_choice_is_marked_on_the_chosen_item_only() {
        let state = state();
        assert!(menu_action_checked(
            MenuAction::Theme(ThemePreference::System),
            state
        ));
        assert!(!menu_action_checked(
            MenuAction::Theme(ThemePreference::Dark),
            state
        ));
    }
}

//! Тема окна в одном месте: кегль, гарнитура, цвета и метрики.
//!
//! Цвета не хардкодим, а берём из темы gpui-kit через [`Theme`] — тогда правка
//! темы кита меняет и наше окно. Здесь же собраны размеры: список, шапка,
//! статус-бар, колонки.
//!
//! **Один кегль на всё окно.** [`font_size`] — единственное число, от которого
//! масштабируется интерфейс: от него считается `rem` (база сетки кита) и все
//! метрики ниже. Исключение по договорённости — иконки: буквы в плашках и
//! логотип в «О программе» живут в своём масштабе.
//!
//! Гарнитуру не выбираем: кит держит системную (`Theme::font_family`,
//! `.SystemUIFont`) и сам подставляет установленную замену, если системная
//! недоступна. Свой шрифт сюда не прибиваем — на другой ОС его просто нет.
//!
//! Менять кегль надо через [`FontScale`] и [`apply`]: `Root` из gpui-component
//! на каждой отрисовке делает `window.set_rem_size(theme.font_size)`, поэтому
//! кегль живёт в теме, а не в окне. Подробности — в комментарии к [`init`].

use crate::window::ViewMode;
use gpui_kit::component::{Size, Theme, ThemeConfig, ThemeMode, ThemeRegistry, ThemeSet};
use gpui_kit::*;
use std::sync::atomic::{AtomicU8, Ordering};

/// Базовый кегль окна — **средний, 12 px**; соседние размеры на 1 pt меньше
/// и больше.
///
/// Кегль интерфейса кит задаёт как `text_sm` = 0.875 rem, поэтому [`rem`] —
/// это кегль, делённый на 0.875. Малый даёт ≈10,7 px, средний — 12 px,
/// большой — ≈13,3 px (шаг 1 pt = 1,333 px).
///
/// Дробный `rem` — осознанная плата за точный кегль: высоты, которые считаем
/// сами, дополнительно округляются [`snap`], а текст GPUI рисует с
/// субпиксельным позиционированием DirectWrite. Если нужен строго целый `rem`,
/// поменяй числа в [`FontScale::font_px`] на удобные и пересчитай.
#[repr(u8)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum FontScale {
    Small,
    #[default]
    Medium,
    Large,
}

impl FontScale {
    /// Кегль текста в логических пикселях.
    pub(crate) const fn font_px(self) -> f32 {
        match self {
            Self::Small => 12.0 - 4.0 / 3.0,
            Self::Medium => 12.0,
            Self::Large => 12.0 + 4.0 / 3.0,
        }
    }

    /// База сетки кита (`rem`) для масштаба.
    pub(crate) const fn rem(self) -> f32 {
        self.font_px() / 0.875
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Small => "Малый",
            Self::Medium => "Средний",
            Self::Large => "Большой",
        }
    }

    /// Значение для `--dump-state` и тестов.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    const fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Small,
            2 => Self::Large,
            _ => Self::Medium,
        }
    }
}

/// Выбранный масштаб текста. Он один на процесс: окно у приложения одно, а
/// метрики (высота строки, плитки) нужны и там, где `Context` нет — например,
/// в попадании мышью по списку.
static FONT_SCALE: AtomicU8 = AtomicU8::new(FontScale::Medium as u8);

pub(crate) fn font_scale() -> FontScale {
    FontScale::from_u8(FONT_SCALE.load(Ordering::Relaxed))
}

pub(crate) fn set_font_scale(scale: FontScale) {
    FONT_SCALE.store(scale as u8, Ordering::Relaxed);
}

/// База сетки кита: от неё считаются `px_*`, `gap_*` и `text_*`.
pub(crate) fn rem() -> f32 {
    font_scale().rem()
}

/// Кегль всего текста в приложении, в логических пикселях.
pub(crate) fn font_size() -> f32 {
    font_scale().font_px()
}

/// Единый размерный класс всех элементов управления.
///
/// Кнопки, поля ввода, выпадающие списки, галочки и радио берут размер отсюда:
/// у всех одна высота (Small = `h_6`) и один кегль (`text_sm`). Не задавай
/// размер виджета на месте — иначе соседние поля в одном диалоге разъезжаются
/// по высоте, а «мелкое» поле ещё и печатает другим кеглем.
pub const CONTROL_SIZE: Size = Size::Small;

/// Округление до целого пикселя: метрики ниже считаются от [`rem`], но высоты
/// строк и шапки должны быть целыми, иначе текст садится между пикселями.
fn snap(value: f32) -> f32 {
    (value + 0.5).floor()
}

/// Высота строки списка: в Windows 11 Explorer компактный вид строки
/// имеет шаг ровно 21 px (16 px значок + отступы).
pub(crate) fn row_h() -> f32 {
    snap(rem() * 1.53125)
}

/// Вертикальный отступ меню.
pub(crate) fn menu_pad_y() -> f32 {
    rem() * 0.25
}

/// Шапка таблицы в стиле Windows 11 (28 px).
pub(crate) fn header_h() -> f32 {
    snap(rem() * 2.0)
}

/// Статус-бар в стиле Windows 11 (24 px).
pub(crate) fn status_h() -> f32 {
    snap(rem() * 1.75)
}

/// Ширина полосы прокрутки.
pub(crate) fn scrollbar_w() -> f32 {
    snap(rem() * 1.1)
}

/// Размеры значков Windows 11:
/// - Таблица и обычный список: 16×16 px
/// - Мелкие значки: 16×16 px
/// - Крупные значки: 32×32 px
pub const ICON_DETAILS: f32 = 16.0;
pub const ICON_SMALL: f32 = 16.0;
pub const ICON_LARGE: f32 = 32.0;

/// Клетка значка в режимах «крупные/мелкие значки».
///
/// Размеры выводим из того же [`rem`]: масштаб текста остаётся одним рычагом.
pub(crate) fn tile_width(mode: ViewMode) -> f32 {
    match mode {
        ViewMode::LargeIcons => snap(rem() * 6.5),
        ViewMode::SmallIcons => snap(rem() * 5.5),
        _ => 0.,
    }
}

pub(crate) fn tile_height(mode: ViewMode) -> f32 {
    match mode {
        ViewMode::LargeIcons => snap(rem() * 5.5),
        ViewMode::SmallIcons => snap(rem() * 2.5),
        _ => row_h(),
    }
}

/// Ширины колонок. «Имя» тянется на всё оставшееся место.
/// Это фиксированные ширины, как в 7-Zip FM: их задаёт содержимое колонки,
/// а не кегль, поэтому они не пересчитываются от [`font_size`].
pub const COL_SIZE: f32 = 90.;
pub const COL_MODIFIED: f32 = 150.;
pub const COL_CREATED: f32 = 150.;
pub const COL_COMMENT: f32 = 140.;
pub const COL_DIRS: f32 = 70.;
pub const COL_FILES: f32 = 70.;

/// Ширина подписи в строке диалога.
pub const DIALOG_LABEL_W: f32 = 150.;

/// Палитра окна: имена по смыслу, значения — из темы кита.
#[derive(Debug, Clone, Copy)]
pub struct Colors {
    pub bg: Hsla,
    pub fg: Hsla,
    pub border: Hsla,
    pub accent: Hsla,
    pub muted: Hsla,
    pub green: Hsla,
    pub blue: Hsla,
    pub cyan: Hsla,
    pub red: Hsla,
    pub yellow: Hsla,
    pub white: Hsla,
    pub black: Hsla,
    /// Фон шапки таблицы — семантический токен темы (`list.head`): он и задуман
    /// для списков и виден в обеих темах.
    pub list_head: Hsla,
}

impl Colors {
    pub fn new(theme: &Theme) -> Self {
        Self {
            bg: theme.background,
            fg: theme.foreground,
            border: theme.border,
            accent: theme.accent,
            muted: theme.muted_foreground,
            green: theme.green,
            blue: theme.blue,
            cyan: theme.cyan,
            red: theme.red,
            yellow: theme.yellow,
            white: hsla(0., 0., 1., 1.),
            black: hsla(0., 0., 0., 1.),
            list_head: theme.list_head,
        }
    }

    /// Фон шапки таблицы.
    pub fn header(&self) -> Hsla {
        self.list_head
    }

    /// Подсветка строки под курсором в стиле Windows 11 Fluent: деликатная
    /// полупрозрачность (6%), не перекрывающая текст и значки.
    pub fn hover(&self) -> Hsla {
        self.fg.opacity(0.06)
    }

    /// Фон выделенной строки: мягкий синий оттенок (12%), текст остаётся
    /// контрастным, а строка читается как выбранная плашка в стиле Windows 11.
    pub fn selected(&self) -> Hsla {
        self.blue.opacity(0.12)
    }

    /// Затемнение под модальным диалогом.
    pub fn overlay(&self) -> Hsla {
        self.black.opacity(0.25)
    }

    /// Ошибка в области списка.
    pub fn error(&self) -> Hsla {
        self.red
    }
}

/// Какая тема выбрана в «Вид → Тема».
///
/// `System` — не отдельная палитра, а режим слежения: при старте и при смене
/// системного оформления берём штатную светлую или «Тёмную» (Carbon).
/// Поэтому значение хранится у окна, а палитра приходит из кита или JSON.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ThemePreference {
    /// Как в системе (по умолчанию): светлая днём, «Тёмная» ночью.
    #[default]
    System,
    Light,
    /// Совсем чёрная палитра.
    Dark,
    /// Тёмная серая.
    Graphite,
    /// Светлая серая.
    Silver,
    Barbie,
    Pony,
    /// Почти чёрная с янтарным акцентом — она же «Тёмная» и системная ночь.
    Carbon,
    /// Бело-кобальтовая.
    Gzhel,
    /// Тёмный неон: магента и циан.
    Guro,
}

impl ThemePreference {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::System => "Системная",
            Self::Light => "Светлая",
            Self::Carbon => "Тёмная",
            Self::Dark => "Блэкаут",
            Self::Graphite => "Графит",
            Self::Silver => "Серебро",
            Self::Barbie => "Barbie",
            Self::Pony => "My Little Pony",
            Self::Gzhel => "Гжель",
            Self::Guro => "Гуро",
        }
    }

    /// Значение для `--dump-state` и тестов.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "blackout",
            Self::Graphite => "graphite",
            Self::Silver => "silver",
            Self::Barbie => "barbie",
            Self::Pony => "pony",
            Self::Carbon => "carbon",
            Self::Gzhel => "gzhel",
            Self::Guro => "guro",
        }
    }
}

/// Ставит тему и кегль. Вызывать один раз после `gpui_kit::init`.
///
/// Почему через тему, а не через `window.set_rem_size`: `Root` из gpui-component
/// на каждой отрисовке (`RootPlugin::prepare`) делает
/// `window.set_rem_size(cx.theme().font_size)`. То есть `rem` окна — это всегда
/// `font_size` темы, и наш собственный `set_rem_size` затирается: сборка
/// проходит, а шрифт не меняется. Поэтому кегль живёт в теме.
///
/// Кегль ставим **после** смены режима: `Theme::change` загружает конфиг темы,
/// а он вправе нести свои размеры шрифта. Иначе переключение тёмной темы молча
/// вернуло бы системный кегль.
pub fn init(preference: ThemePreference, scale: FontScale, cx: &mut App) {
    set_font_scale(scale);
    apply(preference, None, cx);
}

/// Переключает палитру и заново применяет наш кегль. Возвращает предупреждение,
/// если внешний `themes.json` битый и применена встроенная копия: ошибку должно
/// быть видно, а галочка темы не должна врать.
///
/// `System` спрашивает оформление у платформы (`window.appearance()`, а без
/// окна — `cx.window_appearance()`): это работает и на Windows, и на macOS, и
/// на Linux через портал оформления. Остальные варианты задают режим явно.
pub(crate) fn apply(
    preference: ThemePreference,
    window: Option<&mut Window>,
    cx: &mut App,
) -> Option<String> {
    let mut warning = None;
    match preference {
        ThemePreference::System => {
            // «Системная» — штатная светлая днём и наша «Тёмная» (Carbon)
            // ночью; чужие светлые/тёмные палитры в реестре кита не оставляем.
            warning = register_system_themes(cx);
            Theme::sync_system_appearance(window, cx);
        }
        ThemePreference::Light => {
            // Своя светлая палитра могла остаться в реестре кита: возвращаем
            // штатную, иначе «Светлая» показала бы, например, «Серебро».
            let stock = ThemeRegistry::global(cx).default_light_theme().clone();
            Theme::update(cx, |theme| theme.light_theme = stock);
            Theme::change(ThemeMode::Light, window, cx);
        }
        // Свои палитры лежат в `themes.json` (рядом с exe или в исходниках),
        // а не в коде: цвет можно поправить, не пересобирая приложение.
        ThemePreference::Dark
        | ThemePreference::Graphite
        | ThemePreference::Silver
        | ThemePreference::Barbie
        | ThemePreference::Pony
        | ThemePreference::Carbon
        | ThemePreference::Gzhel
        | ThemePreference::Guro => {
            apply_custom(preference, &mut warning, cx);
        }
    }
    Theme::update(cx, |theme| {
        theme.font_size = px(rem());
        #[cfg(target_os = "windows")]
        {
            theme.font_family = "Segoe UI Variable Text".into();
        }
    });
    warning
}

/// Читает набор тем: сначала файл рядом с exe/в каталоге, при ошибке —
/// встроенная копия. Второй элемент — предупреждение о битом внешнем файле.
fn load_theme_set() -> (ThemeSet, Option<String>) {
    let (text, external) = theme_json();
    match serde_json::from_str::<ThemeSet>(&text) {
        Ok(set) => (set, None),
        Err(err) => {
            let warning = external
                .then(|| format!("themes.json повреждён ({err}); применена встроенная тема"));
            let set = serde_json::from_str(include_str!("../themes.json"))
                .expect("встроенный themes.json корректен");
            (set, warning)
        }
    }
}

/// Регистрирует штатную светлую и «Тёмную» (Carbon) как пару для `System`.
///
/// Цвета не применяем: это сделает `Theme::sync_system_appearance`, выбрав
/// светлую или тёмную половину по оформлению ОС.
fn register_system_themes(cx: &mut App) -> Option<String> {
    let (carbon, warning) = load_theme(ThemePreference::Carbon);
    let (stock_light, stock_dark) = {
        let registry = ThemeRegistry::global(cx);
        (
            registry.default_light_theme().clone(),
            registry.default_dark_theme().clone(),
        )
    };
    let carbon = carbon.unwrap_or(stock_dark);
    Theme::update(cx, |theme| {
        theme.light_theme = stock_light;
        theme.dark_theme = carbon;
    });
    warning
}

/// Ищет тему в внешнем наборе, а если её там нет — во встроенном.
///
/// Внешний `themes.json` может быть старее приложения и не знать новых
/// палитр; тогда берём встроенную копию и предупреждаем.
fn load_theme(preference: ThemePreference) -> (Option<std::rc::Rc<ThemeConfig>>, Option<String>) {
    let (set, parse_warning) = load_theme_set();
    if let Some(config) = find_theme(&set, preference) {
        return (Some(config), parse_warning);
    }
    let embedded = serde_json::from_str::<ThemeSet>(include_str!("../themes.json"))
        .expect("встроенный themes.json корректен");
    if let Some(config) = find_theme(&embedded, preference) {
        let warning = parse_warning.or_else(|| {
            Some(format!(
                "В themes.json нет темы «{}»; применена встроенная",
                preference.label()
            ))
        });
        return (Some(config), warning);
    }
    let warning =
        parse_warning.or_else(|| Some(format!("В themes.json нет темы «{}»", preference.label())));
    (None, warning)
}

/// Применяет свою палитру из внешнего файла. Если файл битый или в нём нет
/// нужной темы — берём встроенную копию, а про внешний файл предупреждаем:
/// иначе галочка в меню показывала бы тему, которой на экране нет.
fn apply_custom(preference: ThemePreference, warning: &mut Option<String>, cx: &mut App) {
    let (config, theme_warning) = load_theme(preference);
    if let Some(text) = theme_warning {
        *warning = Some(text);
    }
    if let Some(config) = config {
        Theme::update(cx, |theme| theme.apply_config(&config));
    }
}

fn find_theme(set: &ThemeSet, preference: ThemePreference) -> Option<std::rc::Rc<ThemeConfig>> {
    set.themes
        .iter()
        .find(|theme| theme.name == preference.as_str())
        .cloned()
        .map(std::rc::Rc::new)
}

/// Файл рядом с exe главнее встроенного: так цвет правится без пересборки.
/// Второй элемент — внешний ли файл прочитан (битый внешний тоже возвращаем:
/// разбор и fallback делает [`apply_custom`], чтобы показать ошибку).
fn theme_json() -> (String, bool) {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
        && let Ok(text) = std::fs::read_to_string(dir.join("themes.json"))
    {
        return (text, true);
    }
    if let Ok(text) = std::fs::read_to_string("themes.json") {
        return (text, true);
    }
    (include_str!("../themes.json").to_string(), false)
}

#[cfg(test)]
mod tests {
    use super::{FontScale, ThemePreference};
    use gpui_kit::component::{Theme, ThemeMode, ThemeSet};
    use std::rc::Rc;

    #[test]
    fn system_and_medium_are_the_defaults() {
        assert_eq!(ThemePreference::default(), ThemePreference::System);
        assert_eq!(FontScale::default(), FontScale::Medium);
    }

    #[test]
    fn medium_is_twelve_pixels_and_neighbours_step_one_point() {
        let small = FontScale::Small.font_px();
        let medium = FontScale::Medium.font_px();
        let large = FontScale::Large.font_px();
        assert!((medium - 12.).abs() < 0.001, "база 12 px: {medium}");
        assert!((medium - small - 4. / 3.).abs() < 0.05, "малый −1 pt");
        assert!((large - medium - 4. / 3.).abs() < 0.05, "большой +1 pt");
    }

    #[test]
    fn menu_labels_and_dump_names_are_stable() {
        assert_eq!(ThemePreference::System.label(), "Системная");
        assert_eq!(ThemePreference::Carbon.label(), "Тёмная");
        assert_eq!(ThemePreference::Dark.label(), "Блэкаут");
        assert_eq!(ThemePreference::Dark.as_str(), "blackout");
        assert_eq!(ThemePreference::Graphite.label(), "Графит");
        assert_eq!(ThemePreference::Silver.label(), "Серебро");
        assert_eq!(ThemePreference::Guro.as_str(), "guro");
        assert_eq!(ThemePreference::Pony.as_str(), "pony");
        assert_eq!(FontScale::Small.label(), "Малый");
        assert_eq!(FontScale::Large.as_str(), "large");
    }

    #[test]
    fn human_readable_theme_file_has_all_custom_themes() {
        let set: ThemeSet = serde_json::from_str(include_str!("../themes.json"))
            .expect("app/themes.json должен быть корректным JSON");
        for name in [
            "carbon", "blackout", "graphite", "silver", "barbie", "pony", "gzhel", "guro",
        ] {
            let theme = set
                .themes
                .iter()
                .find(|theme| theme.name == name)
                .unwrap_or_else(|| panic!("в themes.json нет темы {name}"));
            assert!(
                theme.colors.background.is_some(),
                "у темы {name} не задан background"
            );
        }
    }

    #[test]
    fn theme_file_colors_reach_the_kit_theme() {
        let set: ThemeSet =
            serde_json::from_str(include_str!("../themes.json")).expect("themes.json");
        let config = set
            .themes
            .iter()
            .find(|theme| theme.name == "graphite")
            .cloned()
            .expect("graphite");
        let mut theme = Theme::default();
        theme.apply_config(&Rc::new(config));
        assert_eq!(theme.mode, ThemeMode::Dark);
        assert_eq!(theme.background, gpui_kit::rgb(0x2b2d30).into());
    }
}

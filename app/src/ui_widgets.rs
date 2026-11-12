use crate::prelude::*;

// Значки списка в стиле Windows 11 Fluent: двухтоновые папки с вырезом под бумагу,
// архивы с молнией и аккуратные документы со скошенным уголком (16×16).
const FOLDER_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none"><path d="M1.5 3.5A1.5 1.5 0 0 1 3 2h2.8c.4 0 .78.16 1.06.44l.7.7c.28.28.66.44 1.06.44H13A1.5 1.5 0 0 1 14.5 5v7A1.5 1.5 0 0 1 13 13.5H3A1.5 1.5 0 0 1 1.5 12V3.5Z" fill="currentColor" fill-opacity="0.45"/><rect x="3.5" y="4" width="7" height="3" rx="0.5" fill="#ffffff" fill-opacity="0.7"/><path d="M1 6.5A1.5 1.5 0 0 1 2.5 5h11A1.5 1.5 0 0 1 15 6.5v5.5a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V6.5Z" fill="currentColor"/></svg>"##;
const ARCHIVE_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none"><path d="M1.5 3.5A1.5 1.5 0 0 1 3 2h2.8c.4 0 .78.16 1.06.44l.7.7c.28.28.66.44 1.06.44H13A1.5 1.5 0 0 1 14.5 5v7A1.5 1.5 0 0 1 13 13.5H3A1.5 1.5 0 0 1 1.5 12V3.5Z" fill="currentColor" fill-opacity="0.45"/><path d="M1 6.5A1.5 1.5 0 0 1 2.5 5h11A1.5 1.5 0 0 1 15 6.5v5.5a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V6.5Z" fill="currentColor"/><path d="M8 5v8.5M6.8 6.5h2.4M6.8 8h2.4M6.8 9.5h2.4M6.8 11h2.4" stroke="#ffffff" stroke-width="0.9" stroke-linecap="round"/></svg>"##;
const FILE_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none"><path d="M3.5 1.5A1.5 1.5 0 0 0 2 3v10a1.5 1.5 0 0 0 1.5 1.5h9a1.5 1.5 0 0 0 1.5-1.5V5L10.5 1.5H3.5Z" fill="currentColor" fill-opacity="0.12" stroke="currentColor" stroke-width="1.1" stroke-linejoin="round"/><path d="M10 1.5V5h3.5" stroke="currentColor" stroke-width="1.1" stroke-linejoin="round"/><path d="M4.5 7.5h5M4.5 9.5h7M4.5 11.5h6" stroke="currentColor" stroke-width="0.9" stroke-linecap="round" stroke-opacity="0.5"/></svg>"##;

/// Единый размер для всех интерактивных элементов.
///
/// Трейт намеренно висит на `Sizable`, а не на конкретных типах: кнопка, поле,
/// галочка, радио, число и выпадающий список получают один класс размера
/// (`theme::CONTROL_SIZE`) и, значит, одну высоту и один кегль. Не зови
/// `xsmall()/medium()` на месте — это и есть рассинхрон, который мы убираем.
pub(crate) trait ControlSized: Sizable + Sized {
    fn control_size(self) -> Self {
        self.with_size(theme::CONTROL_SIZE)
    }
}

impl<T: Sizable> ControlSized for T {}

/// Поле ввода в едином размере. Высота и кегль — из [`ControlSized`];
/// `focus_bordered(false)` оставляем по умолчанию: в диалогах рамку рисует
/// контейнер, иначе кит кладёт поверх вторую.
pub(crate) fn control_input(state: &Entity<InputState>) -> Input {
    Input::new(state)
        .control_size()
        .text_size(px(theme::font_size()))
        .focus_bordered(false)
}

/// Единственная кнопка диалогов: один вариант, один размер, один кегль.
///
/// Через неё идут подтверждение, отмена, «…», крестик и погашенные «✕/Y».
/// Не добавляй отдельной кнопке диалога `.primary()`, `.ghost()` или свои
/// `px_*`/`py_*`: рядом это сразу видно. Панель меню и тулбар — отдельное
/// исключение: они остаются `ghost`, как в 7-Zip FM.
pub(crate) fn ui_button(id: impl Into<ElementId> + 'static) -> Button {
    Button::new(id).control_size().text_sm()
}

/// Значок строки: системный, если он уже загружен, иначе встроенный SVG.
///
/// Системный значок приходит картинкой, а не SVG, поэтому у него нет
/// `text_color`; размер задаём явно, чтобы он совпадал с прежней плашкой.
pub(crate) fn entry_icon(
    icon: Option<EntryIcon>,
    is_dir: bool,
    is_archive: bool,
    size: f32,
    c: &Colors,
) -> AnyElement {
    if let Some(icon) = icon {
        let image = match icon {
            EntryIcon::Image(image) => img(image),
            EntryIcon::File(path) => img(path),
        };
        return image.w(px(size)).h(px(size)).flex_none().into_any_element();
    }
    let (svg, color) = if is_dir {
        (FOLDER_SVG, c.yellow)
    } else if is_archive {
        (ARCHIVE_SVG, c.yellow)
    } else {
        (FILE_SVG, c.muted)
    };
    Icon::default()
        .data(svg)
        .size(px(size))
        .text_color(color)
        .into_any_element()
}

pub(crate) fn dialog_button(
    id: impl Into<ElementId> + 'static,
    label: impl Into<SharedString> + 'static,
) -> Button {
    ui_button(id).label(label)
}

/// Крестик закрытия: одна кнопка на все диалоги и дерево папок.
pub(crate) fn close_button(id: &'static str) -> Button {
    ui_button(id).label("×").tooltip("Закрыть (Esc)")
}

/// Затемнение окна под модальным диалогом.
pub(crate) fn dialog_layer(c: &Colors, card: impl IntoElement) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(c.overlay())
        .child(card)
}

/// Каркас диалога: заголовок с крестиком, тело, подвал.
pub(crate) fn dialog_card(
    width: f32,
    title: impl IntoElement,
    close: impl IntoElement,
    body: impl IntoElement,
    footer: impl IntoElement,
    c: &Colors,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .w(px(width))
        .bg(c.bg)
        .border_1()
        .border_color(c.border)
        .rounded_md()
        .shadow_lg()
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(c.border)
                .child(div().text_sm().child(title))
                .child(close),
        )
        .child(body)
        .child(footer)
}

impl MainWindow {
    /// Строка диалога: подпись слева, элемент справа.
    pub(crate) fn dialog_row(
        &self,
        label: &'static str,
        field: impl IntoElement,
    ) -> impl IntoElement {
        h_flex()
            .items_center()
            .gap_2()
            .child(div().w(px(DIALOG_LABEL_W)).text_sm().child(label))
            .child(div().flex_1().child(field))
    }

    /// Пункт, которого у нас пока нет: «✕» и погашенная подпись — как в меню.
    pub(crate) fn soon_row(label: &'static str, c: &Colors) -> impl IntoElement {
        Self::marked_row("✕", label, c)
    }

    /// Пункт, который на этой платформе сделать нельзя: «Y» вместо «✕».
    ///
    /// Разница важна: «✕» — сделаем, «Y» — не сделаем в принципе (нужен COM,
    /// `unsafe` или файл, которого мы не поставляем).
    pub(crate) fn impossible_row(label: &'static str, c: &Colors) -> impl IntoElement {
        Self::marked_row("Y", label, c)
    }

    pub(crate) fn marked_row(
        marker: &'static str,
        label: &'static str,
        c: &Colors,
    ) -> impl IntoElement {
        h_flex()
            .items_center()
            .gap_2()
            .opacity(0.55)
            .child(
                div()
                    .w(px(14.))
                    .flex_none()
                    .text_sm()
                    .text_color(c.muted)
                    .child(marker),
            )
            .child(div().text_sm().child(label))
    }

    /// Погашенная кнопка нереализованного действия.
    pub(crate) fn soon_button(label: &'static str) -> impl IntoElement {
        Self::marked_button(label)
    }

    /// Погашенная кнопка невозможного действия («Y» в подписи).
    pub(crate) fn impossible_button(label: &'static str) -> impl IntoElement {
        Self::marked_button(label)
    }

    pub(crate) fn marked_button(label: &'static str) -> impl IntoElement {
        // Та же кнопка, что и живая, только disabled: высота, отступы, кегль и
        // вариант те же. Раньше это был свой div с другими метриками.
        ui_button(label).label(label).disabled(true)
    }

    /// Крестик в заголовке диалога.
    pub(crate) fn dialog_close(
        &self,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        close_button(id).on_click(cx.listener(|this, _event, window, cx| {
            this.close_modal();
            this.folder_picker = None;
            window.focus(&this.focus, cx);
            cx.notify();
        }))
    }
}

/// Политика совпадений — одна таблица для извлечения и переноса.
///
/// Раньше одинаковый цикл с «Заменить/Пропустить/Переименовать» был скопирован
/// в два диалога; теперь подписи и порядок задаются здесь.
pub(crate) const OVERWRITE_MODES: [(Overwrite, &str); 3] = [
    (Overwrite::All, "Заменить"),
    (Overwrite::Skip, "Пропустить"),
    (Overwrite::Rename, "Переименовать"),
];

/// «Пути к файлам» в диалоге извлечения: `7z x` или `7z e`.
pub(crate) const PATH_MODES: [&str; 2] = ["Полные пути", "Без путей"];
pub(crate) const PATH_FLAT: &str = "Без путей";

pub(crate) fn overwrite_index(value: Overwrite) -> usize {
    OVERWRITE_MODES
        .iter()
        .position(|(mode, _)| *mode == value)
        .unwrap_or(0)
}

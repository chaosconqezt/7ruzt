use crate::prelude::*;
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::base::Selectable as _;
/// Строка пункта меню: маркер, подпись, горячая клавиша.
///
/// Маркер один на все случаи: `✓` — включённый режим (например, текущая
/// сортировка), `✕` — пункт ещё не реализован. Так неготовое видно сразу и не
/// путается с «сейчас недоступно» (такие пункты просто погашены).
pub(crate) fn menu_row(
    marker: &'static str,
    label: &'static str,
    shortcut: &'static str,
    c: Colors,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .min_w(px(300.))
        .items_center()
        .gap_3()
        .child(
            div()
                .w(px(14.))
                .flex_none()
                .text_sm()
                .text_color(c.muted)
                .child(marker),
        )
        .child(div().flex_1().child(label))
        .child(div().text_sm().text_color(c.muted).child(shortcut))
}

/// Пункты верхнего меню для `dropdown_menu`. Обработчики идут через `WeakEntity`:
/// попап живёт в слое кита и прямого доступа к нашему состоянию не имеет.
pub(crate) fn build_popup(
    menu: TopMenu,
    popup: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    weak: WeakEntity<MainWindow>,
    state: MenuState,
) -> PopupMenu {
    push_entries(popup, menu_entries(menu), window, cx, &weak, state)
}

/// Наполняет попап пунктами. Подменю — тот же список пунктов в своём попапе,
/// поэтому функция рекурсивная.
pub(crate) fn push_entries(
    mut popup: PopupMenu,
    entries: &'static [MenuEntry],
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    weak: &WeakEntity<MainWindow>,
    state: MenuState,
) -> PopupMenu {
    for entry in entries {
        if entry.separator {
            popup = popup.separator();
            continue;
        }

        if !entry.children.is_empty() {
            // Подменю целиком из неготовых пунктов помечаем «✕» прямо в подписи:
            // в строке подменю колонки маркера нет.
            let all_pending = entry
                .children
                .iter()
                .all(|child| child.action.is_none() && child.children.is_empty());
            let label = if all_pending {
                format!("✕ {}", entry.label)
            } else {
                entry.label.to_string()
            };
            let children = entry.children;
            let weak = weak.clone();
            popup = popup.submenu(label, window, cx, move |sub, window, cx| {
                push_entries(sub, children, window, cx, &weak, state)
            });
            continue;
        }

        let (marker, ready) = match entry.action {
            None => ("✕", false),
            Some(action) if !menu_action_enabled(action, state) => ("", false),
            Some(action) if menu_action_checked(action, state) => ("✓", true),
            Some(_) => ("", true),
        };

        let label = entry.label;
        let shortcut = entry.shortcut;
        let mut item = PopupMenuItem::element(move |_window, cx| {
            menu_row(marker, label, shortcut, Colors::new(cx.theme()))
        });

        if !ready {
            item = item.disabled(true);
        } else if let Some(action) = entry.action {
            let weak = weak.clone();
            item = item.on_click(move |_event, window, cx| {
                let _ = weak.update(cx, |this, cx| this.run_action(action, window, cx));
            });
        }

        popup = popup.item(item);
    }
    popup
}

impl MainWindow {
    /// Создать попап меню заново: в нём зафиксированы галочки и доступность.
    fn build_menu_popup(&mut self, menu: TopMenu, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let state = self.menu_state();
        let popup = PopupMenu::build(window, cx, move |popup, window, cx| {
            build_popup(menu, popup, window, cx, weak, state)
        });
        popup.focus_handle(cx).focus(window, cx);
        self.menu_subscription = Some(cx.subscribe_in(
            &popup,
            window,
            |this, _, _: &DismissEvent, window, cx| this.close_menu(window, cx),
        ));
        self.menu_popup = Some(popup);
    }

    /// Закрыть меню. Фокус возвращаем списку, только если он остался в попапе:
    /// при выборе пункта фокус уже мог уйти в открывшийся диалог.
    fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let popup_focused = self
            .menu_popup
            .as_ref()
            .is_some_and(|popup| popup.read(cx).focus_handle(cx).contains_focused(window, cx));
        self.open_menu = None;
        self.menu_popup = None;
        self.menu_subscription = None;
        if popup_focused {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    fn toggle_menu(&mut self, menu: TopMenu, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_menu == Some(menu) {
            self.close_menu(window, cx);
            return;
        }
        self.open_menu = Some(menu);
        // Старый попап и его подписка больше не нужны.
        self.menu_popup = None;
        self.menu_subscription = None;
        self.build_menu_popup(menu, window, cx);
        cx.notify();
    }

    /// Наведение на соседний пункт при уже открытом меню переключает попап —
    /// как в Windows, без повторного клика.
    fn menu_hovered(&mut self, menu: TopMenu, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_menu.is_some() && self.open_menu != Some(menu) {
            self.toggle_menu(menu, window, cx);
        }
    }

    /// Верхнее меню. `Button` — только триггер, попап ведём сами: встроенный
    /// `dropdown_menu` не умеет переключаться по наведению при уже открытом меню.
    pub(crate) fn menu_bar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let c = Colors::new(cx.theme());

        // `--menu-view` (и любой внешний код) может выставить `open_menu`
        // до первого кадра. Попап тогда создаём здесь же.
        if let Some(menu) = self.open_menu
            && self.menu_popup.is_none()
        {
            self.build_menu_popup(menu, window, cx);
        }
        let open = self.open_menu;
        let popup = self.menu_popup.clone();

        let mut bar = h_flex()
            .items_center()
            .px_2()
            // Межстрочный отступ меню — из темы: на 15% плотнее обычного.
            .py(px(theme::menu_pad_y()))
            .gap_1()
            .border_b_1()
            .border_color(c.border);

        for menu in [
            TopMenu::File,
            TopMenu::Edit,
            TopMenu::View,
            TopMenu::Favorites,
            TopMenu::Service,
            TopMenu::Help,
        ] {
            let is_open = open == Some(menu);
            let trigger = Button::new(("menu", menu.index()))
                .label(menu.title())
                .ghost()
                .control_size()
                .text_sm()
                .disabled(
                    self.modal_open()
                        || window.has_active_dialog(cx)
                        || self.job.as_ref().is_some_and(|job| !job.hidden),
                )
                .open(is_open)
                .on_click(cx.listener(move |this, _event, window, cx| {
                    cx.stop_propagation();
                    this.toggle_menu(menu, window, cx);
                }));

            let mut item = div()
                .id(("menu-item", menu.index()))
                .relative()
                .child(trigger)
                .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                    if *hovered {
                        this.menu_hovered(menu, window, cx);
                    }
                }));

            if is_open && let Some(popup) = popup.clone() {
                let popup = popup.into_any_element();
                item = item.child(deferred(
                    anchored()
                        .anchor(Anchor::TopLeft)
                        .snap_to_window_with_margin(px(8.))
                        .child(div().size_full().occlude().top_1().child(popup)),
                ));
            }

            bar = bar.child(item);
        }
        bar
    }
}

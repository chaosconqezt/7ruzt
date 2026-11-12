//! Главное окно — клон 7-Zip File Manager.
//!
//! Здесь только точка входа: разбор аргументов, окно, запуск. Логика — в
//! `window.rs`, отрисовка — в `ui*.rs`, диагностика — в `diag.rs`. Модули
//! плоские: всё нужное лежит рядом, без вложенных каталогов.
//!
//! `windows_subsystem = "windows"`: без этого рядом с GUI-окном висит консоль,
//! и она же перехватывает `MainWindowHandle` при автоматическом скриншоте.

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod archive;
mod bench;
mod diag;
mod dialogs;
mod files;
mod fsutil;
mod icons;
mod job;
mod keys;
mod launch;
mod menu;
mod nav;
mod panel;
mod picker;
mod prelude;
mod select;
mod shell;
mod text;
mod theme;
mod transfer;
mod ui;
mod ui_about;
mod ui_chrome;
mod ui_dialog_bench;
mod ui_dialog_create;
mod ui_dialog_picker;
mod ui_dialog_progress;
mod ui_dialog_simple;
mod ui_list;
mod ui_menu;
mod ui_widgets;
mod window;

pub(crate) use std::collections::BTreeSet;
pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::sync::Arc;
pub(crate) use std::sync::atomic::{AtomicBool, Ordering};
pub(crate) use std::time::{Duration, Instant};

pub(crate) use archiver_core::{
    BenchOutcome, BenchReport, BenchValue, ConflictPolicy, CreateRequest, Entry, ExtractRequest,
    HashAlgorithm, HashReport, HashRequest, Level, Listing, Locator, Method, Overwrite, PathMode,
    Progress, SortKey, TestRequest, TestStats, check_name, child_prefix, copy_paths, copy_to,
    create, create_empty_file, create_folder, delete_paths, extract, file_extension, group_digits,
    hash_files, human_size, is_archive_name, list_archive, list_dir, mask_matches, move_paths,
    move_to, parent_prefix, parse_bench_output, rename_in, run_benchmark, sort_entries,
    test_archive, view_at,
};

// Трейты — по именам, а не `as _`: модули берут их через `prelude`,
// а анонимные импорты глобом не подхватываются.
pub(crate) use gpui_kit::assets::IconName;
pub(crate) use gpui_kit::base::{h_flex, v_flex};
pub(crate) use gpui_kit::component::Sizable;
pub(crate) use gpui_kit::component::WindowExt;
pub(crate) use gpui_kit::component::button::{Button, ButtonVariants};
pub(crate) use gpui_kit::component::checkbox::Checkbox;
pub(crate) use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants};
pub(crate) use gpui_kit::component::input::{Input, InputEvent, InputState, NumberInput};
pub(crate) use gpui_kit::component::list::ListItem;
pub(crate) use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
pub(crate) use gpui_kit::component::progress::Progress as ProgressBar;
pub(crate) use gpui_kit::component::radio::{Radio, RadioGroup};
pub(crate) use gpui_kit::component::scroll::{Scrollbar, ScrollbarMode};
pub(crate) use gpui_kit::component::searchable_list::SearchableVec;
pub(crate) use gpui_kit::component::select::{Select, SelectEvent, SelectState};
pub(crate) use gpui_kit::component::skeleton::Skeleton;
pub(crate) use gpui_kit::component::spinner::Spinner;
pub(crate) use gpui_kit::component::status_bar::StatusBar;
pub(crate) use gpui_kit::component::toolbar::Toolbar;
pub(crate) use gpui_kit::component::tooltip::Tooltip;
pub(crate) use gpui_kit::component::tree::{TreeEvent, TreeItem, TreeState, tree};
pub(crate) use gpui_kit::component::{ActiveTheme, Disableable, Icon, IndexPath};
pub(crate) use gpui_kit::prelude::FluentBuilder;
pub(crate) use gpui_kit::*;

pub(crate) use crate::theme::{Colors, DIALOG_LABEL_W, FontScale, ThemePreference};

fn main() {
    let launch = launch::Launch::from_env();
    let path = launch.path.clone();
    let prefix = launch.prefix.clone();
    let preset = launch.preset.clone();
    let address_override = launch.address.clone();
    let auto_address = address_override.is_some();
    let auto_extract = launch.auto_extract;
    let auto_extract_dialog = launch.auto_extract_dialog;
    let auto_menu = launch.auto_menu;
    let title_bench = launch.title_bench;
    let auto_add = launch.auto_add;
    let auto_submit = launch.auto_submit;
    let auto_test = launch.auto_test;
    let auto_hash = launch.auto_hash;
    let auto_bench = launch.auto_bench;
    let theme_pref = launch.theme;
    let font_scale = launch.font;
    let auto_two_panel = launch.two_panel;
    let right_panel = launch.right.clone();
    let dump_state_target = launch.dump_state.clone();
    let test_keys = launch.test_keys.clone();
    let quit_after = launch.quit_after;

    let title = launch.title();
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            ..Default::default()
        }),
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(10.), px(10.)),
            size(px(1000.), px(640.)),
        ))),
        ..Default::default()
    };

    let app = gpui_kit::application().with_assets(gpui_kit::assets::AllAssets);
    app.run(move |cx| {
        gpui_kit::init(cx);
        gpui_kit::component::init(cx);
        // Tab в gpui-base занят переходом по фокусу. Наш биндинг в контексте
        // окна перекрывает его: в 7-Zip FM Tab переключает панели.
        cx.bind_keys([
            KeyBinding::new("tab", window::SwitchPanel, Some("Archiver")),
            KeyBinding::new("shift-tab", window::SwitchPanel, Some("Archiver")),
            // Пока адрес правится, Tab дополняет введённый путь, а не
            // переключает панели. Контекст живёт только на время правки.
            KeyBinding::new("tab", window::Complete, Some("AddressSuggestions")),
        ]);
        // Кегль и гарнитура живут в теме: Root на каждой отрисовке ставит
        // окну rem = theme.font_size, поэтому менять надо именно тему.
        theme::init(theme_pref, font_scale, cx);
        let path = path.clone();
        let prefix = prefix.clone();
        let (window_handle, view) = gpui_kit::open_window(options, cx, move |window, cx| {
            let path = path.clone();
            let prefix = prefix.clone();
            let view = {
                // Окно нужно конструктору для подписки на смену системного
                // оформления; берём его повторно, чтобы оно осталось доступно
                // ниже (фокус списка, автоматические действия).
                let window_ref = &mut *window;
                cx.new(move |cx| {
                    window::MainWindow::with_prefix(
                        path.clone(),
                        prefix,
                        theme_pref,
                        font_scale,
                        window_ref,
                        cx,
                    )
                })
            };

            // Сразу отдаём фокус списку: стрелки должны работать без предварительного клика.
            let focus = view.read(cx).focus.clone();
            window.focus(&focus, cx);
            view
        })
        .expect("не удалось открыть окно");

        let automated = !test_keys.is_empty()
            || dump_state_target.is_some()
            || quit_after.is_some()
            || preset.is_some()
            || auto_two_panel
            || auto_extract
            || title_bench
            || auto_menu
            || auto_extract_dialog
            || auto_add
            || auto_test
            || auto_hash
            || auto_bench
            || auto_address;
        if automated {
            let view_test = view.clone();
            cx.spawn(async move |cx| {
                // Даём окну смонтировать Root и WindowState перед запуском действий
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;

                // Стартовая загрузка теперь асинхронная: ждём её, иначе пресет
                // выделения и автодействия увидят пустой список.
                for _ in 0..200 {
                    let ready = cx.update(|cx| {
                        let panel = &view_test.read(cx).panels[0];
                        panel.loaded || panel.error.is_some()
                    });
                    if ready {
                        break;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(25))
                        .await;
                }

                let _ = window_handle.update(cx, |_root, window, cx| {
                    // Выделение из `--select=` ставим до всех автоматических действий:
                    // им пользуются и `--add`, и просто скриншот выделенной строки.
                    if let Some(indices) = &preset {
                        let indices = indices.clone();
                        view_test.update(cx, |this, cx| {
                            let panel = this.active_mut();
                            panel.selection = Arc::new(
                                indices
                                    .iter()
                                    .copied()
                                    .filter(|index| *index < panel.entries.len())
                                    .collect(),
                            );
                            panel.active = panel.selection.iter().next_back().copied();
                            panel.anchor = panel.selection.iter().next().copied();
                            cx.notify();
                        });
                    }

                    if let Some(text) = address_override.clone() {
                        view_test.update(cx, |this, cx| {
                            this.edit_address(0, window, cx);
                            if let Some(edit) = this.address_edit.as_ref() {
                                let input = edit.input.clone();
                                input.update(cx, |state, cx| {
                                    state.set_value(text.clone(), window, cx);
                                });
                            }
                            this.refresh_address_suggestions(cx);
                            cx.notify();
                        });
                    }

                    if auto_two_panel {
                        let right_panel = right_panel.clone();
                        view_test.update(cx, move |this, cx| {
                            if let Some(path) = right_panel {
                                this.show_second_panel(path, cx);
                            } else {
                                this.toggle_two_panels(cx);
                            }
                        });
                    }

                    if auto_extract {
                        view_test.update(cx, |this, cx| {
                            this.open_extract_dialog(window, cx);
                            this.submit_extract(cx);
                        });
                    }

                    if title_bench {
                        // Сколько на этой машине стоит SetWindowText. Раньше мы звали его
                        // на каждое нажатие клавиши — если он дорогой, это и был фриз.
                        let started = Instant::now();
                        for index in 0..20 {
                            window.set_window_title(&format!("title-bench {index}"));
                        }
                        let per_call = started.elapsed().as_secs_f64() * 1000.0 / 20.0;
                        let _ = std::fs::write(
                            "titlebench.log",
                            format!("SetWindowText: {per_call:.3} ms per call\n"),
                        );
                    }

                    if auto_menu {
                        view_test.update(cx, |this, cx| {
                            this.open_menu = Some(menu::TopMenu::View);
                            cx.notify();
                        });
                    }

                    if auto_extract_dialog {
                        view_test.update(cx, |this, cx| {
                            this.open_extract_dialog(window, cx);
                            cx.notify();
                        });
                    }

                    if auto_add {
                        view_test.update(cx, |this, cx| {
                            if preset.is_none() {
                                let panel = this.active_mut();
                                panel.selection = Arc::new((0..panel.entries.len()).collect());
                                panel.active = panel.entries.len().checked_sub(1);
                                panel.anchor = Some(0);
                            }
                            this.open_create_dialog(window, cx);
                            if auto_submit {
                                this.submit_create(cx);
                            }
                            cx.notify();
                        });
                    }

                    if auto_test {
                        view_test.update(cx, |this, cx| {
                            // Тот же путь, что и «Сервис → Проверить архив»: шифрованный
                            // архив покажет диалог пароля, обычный проверится сразу.
                            this.test_current(window, cx);
                            cx.notify();
                        });
                    }

                    if auto_hash {
                        view_test.update(cx, |this, cx| {
                            this.start_hash(HashAlgorithm::Sha256, cx);
                            cx.notify();
                        });
                    }

                    if auto_bench {
                        view_test.update(cx, |this, cx| {
                            // Тот же путь, что и «Сервис → Тестирование производительности».
                            this.open_bench(cx);
                            cx.notify();
                        });
                    }

                    view_test.update(cx, |this, cx| {
                        for key in &test_keys {
                            let (key, shift, control, alt) = launch::parse_test_key(key);
                            this.handle_key(key, shift, control, alt, window, cx);
                            this.sync_title(window);
                            cx.notify();
                        }

                        if let Some(target) = &dump_state_target {
                            this.write_dump_state(target, cx);
                        }
                    });
                });

                if let Some(delay_ms) = quit_after {
                    if delay_ms > 0 {
                        cx.background_executor()
                            .timer(Duration::from_millis(delay_ms))
                            .await;
                    }
                    // Повторный дамп перед выходом: фоновые операции к этому
                    // моменту завершились, и видно итог, а не старт.
                    if let Some(target) = &dump_state_target {
                        cx.update(|cx| view_test.read(cx).write_dump_state(target, cx));
                    }
                    cx.update(|cx| cx.quit());
                }
            })
            .detach();
        }
    });
}

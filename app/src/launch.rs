//! Разбор аргументов командной строки.
//!
//! Отдельный модуль, чтобы это можно было покрыть тестами: окно и процесс 7z
//! для проверки флагов не нужны.
//!
//! Флаги автоматизации (`--extract`, `--add`, `--select=…`) включаются только
//! при `ARCHIVER_TEST_FLAGS=1`: в продуктовом бинаре случайный флаг не должен
//! ничего запускать. `scripts/shot.ps1` выставляет переменную сам.

use crate::prelude::*;

/// Что и как открыть при старте.
pub(crate) struct Launch {
    /// Каталог или архив; по умолчанию — текущий каталог.
    pub(crate) path: PathBuf,
    /// Необязательный подкаталог внутри архива (`docs/img`).
    pub(crate) prefix: Option<String>,
    /// Индексы для `--select=0,2`.
    pub(crate) preset: Option<Vec<usize>>,
    /// Начальное значение поля адреса (`--address=C:\Wi`) — для проверки
    /// автодополнения без ручного набора.
    pub(crate) address: Option<String>,
    pub(crate) auto_extract: bool,
    pub(crate) auto_extract_dialog: bool,
    pub(crate) auto_menu: bool,
    pub(crate) title_bench: bool,
    pub(crate) auto_add: bool,
    pub(crate) auto_submit: bool,
    pub(crate) auto_test: bool,
    /// Сразу посчитать SHA-256 выделенного (`--hash-now`).
    pub(crate) auto_hash: bool,
    /// Сразу открыть тестирование производительности (`--bench`).
    pub(crate) auto_bench: bool,
    /// Тема на старте (`--theme=dark`); по умолчанию — системная.
    pub(crate) theme: ThemePreference,
    /// Масштаб текста на старте (`--font=large`); по умолчанию — средний.
    pub(crate) font: FontScale,
    /// Показать вторую панель (F9) сразу при запуске.
    pub(crate) two_panel: bool,
    /// Путь для второй панели при запуске (`--right=…`).
    pub(crate) right: Option<PathBuf>,
    /// Куда записать JSON-дамп состояния окна.
    pub(crate) dump_state: Option<PathBuf>,
    /// Клавиши для автоматического нажатия в окне (`--press=f5,enter`).
    pub(crate) test_keys: Vec<String>,
    /// Завершить приложение через N мс (`--quit-after=500` или `--quit`).
    pub(crate) quit_after: Option<u64>,
}

impl Launch {
    /// `test_flags` передаётся явно: тесты не должны зависеть от окружения.
    pub(crate) fn parse(args: impl IntoIterator<Item = String>, test_flags: bool) -> Self {
        let mut positional: Vec<String> = Vec::new();
        let mut preset = None;
        let mut address = None;
        let (mut auto_extract, mut auto_extract_dialog, mut auto_menu) = (false, false, false);
        let (mut title_bench, mut auto_add, mut auto_submit, mut auto_test) =
            (false, false, false, false);
        let mut auto_hash = false;
        let mut auto_bench = false;
        let mut theme = ThemePreference::System;
        let mut font = FontScale::Medium;
        let mut two_panel = false;
        let mut right = None;
        let mut dump_state = None;
        let mut test_keys = Vec::new();
        let mut quit_after = None;

        for arg in args {
            if !test_flags {
                // Без разрешения единственный аргумент — путь; «--» считаем опечаткой.
                if !arg.starts_with("--") {
                    positional.push(arg);
                }
                continue;
            }

            match arg.as_str() {
                "--extract" => auto_extract = true,
                "--extract-dialog" => auto_extract_dialog = true,
                "--menu-view" => auto_menu = true,
                "--title-bench" => title_bench = true,
                "--add" => auto_add = true,
                "--create-now" => {
                    auto_add = true;
                    auto_submit = true;
                }
                "--test-now" => auto_test = true,
                "--hash-now" => auto_hash = true,
                "--bench" => auto_bench = true,
                "--two-panel" => two_panel = true,
                "--dump-state" => dump_state = Some(PathBuf::from("target/app-state.json")),
                "--quit" => quit_after = Some(0),
                _ => {
                    if let Some(value) = arg.strip_prefix("--right=") {
                        right = Some(PathBuf::from(value));
                        two_panel = true;
                    } else if let Some(value) = arg.strip_prefix("--theme=") {
                        theme = match value.trim().to_lowercase().as_str() {
                            "light" => ThemePreference::Light,
                            "dark" | "black" => ThemePreference::Carbon,
                            "blackout" | "black-out" => ThemePreference::Dark,
                            "gray" | "grey" | "graphite" => ThemePreference::Graphite,
                            "silver" => ThemePreference::Silver,
                            "barbie" => ThemePreference::Barbie,
                            "pony" | "mlp" => ThemePreference::Pony,
                            "carbon" => ThemePreference::Carbon,
                            "gzhel" => ThemePreference::Gzhel,
                            "guro" => ThemePreference::Guro,
                            _ => ThemePreference::System,
                        };
                    } else if let Some(value) = arg.strip_prefix("--font=") {
                        font = match value.trim().to_lowercase().as_str() {
                            "small" | "s" | "малый" => FontScale::Small,
                            "medium" | "m" | "средний" => FontScale::Medium,
                            "large" | "l" | "большой" => FontScale::Large,
                            _ => FontScale::Medium,
                        };
                    } else if let Some(value) = arg.strip_prefix("--address=") {
                        address = Some(value.to_string());
                    } else if let Some(value) = arg.strip_prefix("--dump-state=") {
                        dump_state = Some(PathBuf::from(value));
                    } else if let Some(value) = arg
                        .strip_prefix("--press=")
                        .or_else(|| arg.strip_prefix("--test-keys="))
                    {
                        test_keys = value
                            .split(',')
                            .map(|s| s.trim().to_lowercase())
                            .filter(|s| !s.is_empty())
                            .collect();
                    } else if let Some(value) = arg.strip_prefix("--quit-after=") {
                        quit_after = value.trim().parse::<u64>().ok();
                    } else if let Some(value) = arg.strip_prefix("--select=") {
                        preset = Some(
                            value
                                .split(',')
                                .filter_map(|part| part.trim().parse::<usize>().ok())
                                .collect(),
                        );
                    } else {
                        positional.push(arg);
                    }
                }
            }
        }

        let mut positional = positional.into_iter();
        let path = positional
            .next()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        Self {
            path,
            prefix: positional.next(),
            preset,
            address,
            auto_extract,
            auto_extract_dialog,
            auto_menu,
            title_bench,
            auto_add,
            auto_submit,
            auto_test,
            auto_hash,
            auto_bench,
            theme,
            font,
            two_panel,
            right,
            dump_state,
            test_keys,
            quit_after,
        }
    }

    pub(crate) fn from_env() -> Self {
        Self::parse(
            std::env::args().skip(1),
            std::env::var_os("ARCHIVER_TEST_FLAGS").is_some_and(|value| value == "1"),
        )
    }

    /// Заголовок окна: путь плюс подкаталог архива, если он задан.
    pub(crate) fn title(&self) -> String {
        match &self.prefix {
            Some(prefix) => {
                let sep = if cfg!(windows) { "\\" } else { "/" };
                format!("{}{sep}{}", self.path.display(), prefix.replace('/', sep))
            }
            None => self.path.display().to_string(),
        }
    }
}

/// Разбирает синтетическое нажатие из `--press=`: «ctrl+a», «shift+enter»,
/// «alt+up». Голый «+» — тоже клавиша (диалог маски), а не разделитель.
pub(crate) fn parse_test_key(raw: &str) -> (&str, bool, bool, bool) {
    if raw == "+" {
        return ("+", false, false, false);
    }
    let mut shift = false;
    let mut control = false;
    let mut alt = false;
    let mut key = raw;
    while let Some((prefix, rest)) = key.split_once('+') {
        match prefix {
            "ctrl" => control = true,
            "shift" => shift = true,
            "alt" => alt = true,
            _ => break,
        }
        key = rest;
    }
    (key, shift, control, alt)
}

#[cfg(all(test, windows))]
mod tests {
    use super::{Launch, parse_test_key};

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn test_key_modifiers_parse() {
        assert_eq!(parse_test_key("ctrl+a"), ("a", false, true, false));
        assert_eq!(parse_test_key("shift+enter"), ("enter", true, false, false));
        assert_eq!(parse_test_key("alt+up"), ("up", false, false, true));
        assert_eq!(parse_test_key("ctrl+shift+f3"), ("f3", true, true, false));
        assert_eq!(parse_test_key("+"), ("+", false, false, false));
        assert_eq!(parse_test_key("f5"), ("f5", false, false, false));
    }

    #[test]
    fn select_parses_indices() {
        let launch = Launch::parse(args(&["--select=0, 2,abc,5"]), true);
        assert_eq!(launch.preset, Some(vec![0, 2, 5]));
    }

    #[test]
    fn create_now_implies_add_and_submit() {
        let launch = Launch::parse(args(&["--create-now"]), true);
        assert!(launch.auto_add && launch.auto_submit);
    }

    #[test]
    fn hash_now_parses() {
        let launch = Launch::parse(args(&["--hash-now"]), true);
        assert!(launch.auto_hash);
    }

    #[test]
    fn bench_flag_parses() {
        let launch = Launch::parse(args(&["--bench"]), true);
        assert!(launch.auto_bench);
    }

    #[test]
    fn theme_flag_parses_and_defaults_to_system() {
        let dark = Launch::parse(args(&["--theme=dark"]), true);
        assert_eq!(dark.theme.as_str(), "carbon");
        let light = Launch::parse(args(&["--theme=LIGHT"]), true);
        assert_eq!(light.theme.as_str(), "light");
        let pony = Launch::parse(args(&["--theme=pony"]), true);
        assert_eq!(pony.theme.as_str(), "pony");
        let guro = Launch::parse(args(&["--theme=guro"]), true);
        assert_eq!(guro.theme.as_str(), "guro");
        let blackout = Launch::parse(args(&["--theme=blackout"]), true);
        assert_eq!(blackout.theme.as_str(), "blackout");
        let unknown = Launch::parse(args(&["--theme=xp"]), true);
        assert_eq!(unknown.theme.as_str(), "system");
        let default = Launch::parse(args(&[]), true);
        assert_eq!(default.theme.as_str(), "system");
    }

    #[test]
    fn font_flag_parses_and_defaults_to_medium() {
        let small = Launch::parse(args(&["--font=small"]), true);
        assert_eq!(small.font.as_str(), "small");
        let medium = Launch::parse(args(&["--font=medium"]), true);
        assert_eq!(medium.font.as_str(), "medium");
        let large = Launch::parse(args(&["--font=LARGE"]), true);
        assert_eq!(large.font.as_str(), "large");
        let unknown = Launch::parse(args(&["--font=huge"]), true);
        assert_eq!(unknown.font.as_str(), "medium");
        let default = Launch::parse(args(&[]), true);
        assert_eq!(default.font.as_str(), "medium");
    }

    #[test]
    fn address_override_parses() {
        let launch = Launch::parse(args(&["--address=C:\\Wi"]), true);
        assert_eq!(launch.address.as_deref(), Some("C:\\Wi"));
        let default = Launch::parse(args(&[]), true);
        assert!(default.address.is_none());
    }

    #[test]
    fn flags_are_ignored_without_permission() {
        let launch = Launch::parse(args(&["--add", "--select=1", "C:\\tmp"]), false);
        assert!(!launch.auto_add);
        assert_eq!(launch.preset, None);
        assert_eq!(launch.path, std::path::PathBuf::from("C:\\tmp"));
    }

    #[test]
    fn two_panel_flag_opens_second_panel() {
        let launch = Launch::parse(args(&["--two-panel"]), true);
        assert!(launch.two_panel);
        assert!(launch.right.is_none());
    }

    #[test]
    fn dump_state_and_press_and_quit_flags() {
        let launch = Launch::parse(
            args(&[
                "--dump-state=target/test.json",
                "--press=f5,enter",
                "--quit-after=300",
            ]),
            true,
        );
        assert_eq!(
            launch.dump_state,
            Some(std::path::PathBuf::from("target/test.json"))
        );
        assert_eq!(launch.test_keys, vec!["f5", "enter"]);
        assert_eq!(launch.quit_after, Some(300));
    }

    #[test]
    fn right_path_implies_two_panels() {
        let launch = Launch::parse(args(&["--right=C:\\tmp\\docs"]), true);
        assert!(launch.two_panel);
        assert_eq!(
            launch.right,
            Some(std::path::PathBuf::from("C:\\tmp\\docs"))
        );
    }

    #[test]
    fn second_positional_is_archive_prefix() {
        let launch = Launch::parse(args(&["C:\\tmp\\a.7z", "docs/img"]), true);
        assert_eq!(launch.prefix.as_deref(), Some("docs/img"));
        assert_eq!(launch.title(), "C:\\tmp\\a.7z\\docs\\img");
    }
}

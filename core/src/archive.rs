//! Сворачивание плоского списка путей внутри архива в «вид из корня».
//!
//! `7z l -slt` отдаёт плоский перечень: `docs/readme.txt`. 7-Zip File Manager показывает
//! в корне один каталог `docs`, а внутрь пускает по двойному клику. Без сворачивания
//! список выглядит как дерево, из которого выдрали ветки.
//!
//! В архиве может быть и явная запись каталога (`Path = docs`, `Folder = +`), и
//! одновременно дочерние пути. Обе приходят одной пачкой, поэтому одинаковые имена
//! сливаются, иначе в корне появляются два «docs».

use std::collections::HashMap;

use crate::entry::Entry;

/// Оставляет только верхний уровень, недостающие каталоги создаёт сам.
pub fn root_view(entries: Vec<Entry>) -> Vec<Entry> {
    view_at(&entries, "")
}

/// Показывает один уровень архива под префиксом `prefix` (`""`, `"docs/"`, `"docs/img/"`).
///
/// Внутри префикса всё, что глубже, сворачивается в каталоги — как в 7-Zip FM.
pub fn view_at(entries: &[Entry], prefix: &str) -> Vec<Entry> {
    let trimmed = prefix.trim_end_matches('/');
    let prefix_with_slash = if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}/")
    };

    let mut view: Vec<Entry> = Vec::new();
    // Индекс «имя → позиция в view»: линейный поиск на каждую запись давал
    // квадратичную свёртку на плоских архивах с десятками тысяч файлов.
    let mut index: HashMap<String, usize> = HashMap::new();

    for entry in entries {
        let rest = if prefix_with_slash.is_empty() {
            entry.name.as_str()
        } else {
            match entry.name.strip_prefix(&prefix_with_slash) {
                Some(rest) => rest,
                None => continue,
            }
        };

        // Сам каталог префикса (`docs` при prefix = `docs/`) в этом уровне не показываем.
        if rest.is_empty() {
            continue;
        }

        match rest.split_once('/') {
            None => {
                let mut item = entry.clone();
                item.name = rest.to_string();
                merge_into(&mut view, &mut index, item);
            }
            Some((head, _rest)) => {
                // Дату каталога из даты вложенного файла НЕ выводим: это было бы враньём.
                if !index.contains_key(head) {
                    index.insert(head.to_string(), view.len());
                    view.push(Entry::dir(head));
                }
            }
        }
    }

    crate::entry::sort_entries(&mut view, crate::entry::SortKey::Name, true);
    view
}

/// Префикс каталога внутри архива → префикс родителя.
/// `None` означает «выше некуда», то есть мы в корне архива.
pub fn parent_prefix(prefix: &str) -> Option<String> {
    let trimmed = prefix.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    Some(match trimmed.rsplit_once('/') {
        Some((parent, _)) => format!("{parent}/"),
        None => String::new(),
    })
}

/// Префикс каталога при входе в `name` из `prefix`.
pub fn child_prefix(prefix: &str, name: &str) -> String {
    let base = prefix.trim_end_matches('/');
    if base.is_empty() {
        format!("{name}/")
    } else {
        format!("{base}/{name}/")
    }
}

/// Кладёт запись верхнего уровня, заменяя одноимённую, но не теряя даты,
/// если у новой записи их нет (а у прежней — есть).
fn merge_into(top: &mut Vec<Entry>, index: &mut HashMap<String, usize>, entry: Entry) {
    let Some(&position) = index.get(&entry.name) else {
        index.insert(entry.name.clone(), top.len());
        top.push(entry);
        return;
    };

    let known = &top[position];
    let modified = entry.modified.clone().or_else(|| known.modified.clone());
    let created = entry.created.clone().or_else(|| known.created.clone());
    let is_dir = entry.is_dir || known.is_dir;

    let mut merged = entry;
    merged.modified = modified;
    merged.created = created;
    merged.is_dir = is_dir;
    top[position] = merged;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.name.as_str()).collect()
    }

    #[test]
    fn collapses_nested_paths_into_one_dir() {
        let entries = vec![
            Entry::file("docs/readme.txt"),
            Entry::file("docs/img/logo.png"),
            Entry::file("a.bin"),
        ];
        let root = root_view(entries);
        assert_eq!(names(&root), ["docs", "a.bin"]);
        assert!(root[0].is_dir);
        assert!(!root[1].is_dir);
    }

    #[test]
    fn keeps_flat_names_untouched() {
        let entries = vec![Entry::file("a.bin"), Entry::dir("docs")];
        let root = root_view(entries);
        assert_eq!(names(&root), ["docs", "a.bin"]);
    }

    #[test]
    fn explicit_dir_merges_with_synthesised_one() {
        let entries = vec![Entry::file("docs/readme.txt"), {
            let mut dir = Entry::dir("docs");
            dir.modified = Some("2026-01-01 00:00".to_string());
            dir
        }];
        let root = root_view(entries);
        let dirs: Vec<&Entry> = root.iter().filter(|entry| entry.is_dir).collect();
        assert_eq!(dirs.len(), 1, "каталог не должен задвоиться");
        assert_eq!(dirs[0].modified.as_deref(), Some("2026-01-01 00:00"));
    }

    #[test]
    fn explicit_dir_first_is_not_duplicated_by_children() {
        let entries = vec![
            Entry::dir("docs"),
            Entry::file("docs/readme.txt"),
            Entry::file("docs/img/logo.png"),
        ];
        let root = root_view(entries);
        assert_eq!(names(&root), ["docs"]);
    }

    #[test]
    fn synthesised_dir_has_no_dates() {
        let entries = vec![{
            let mut child = Entry::file("docs/readme.txt");
            child.modified = Some("2026-11-06 14:01".to_string());
            child
        }];
        let root = root_view(entries);
        assert!(root[0].is_dir);
        assert!(root[0].modified.is_none());
    }

    #[test]
    fn view_inside_prefix_shows_next_level() {
        let entries = vec![
            Entry::dir("docs"),
            Entry::file("docs/readme.txt"),
            Entry::file("docs/img/logo.png"),
            Entry::file("a.bin"),
        ];

        let docs = view_at(&entries, "docs/");
        assert_eq!(names(&docs), ["img", "readme.txt"]);
        assert!(docs[0].is_dir);

        let img = view_at(&entries, "docs/img/");
        assert_eq!(names(&img), ["logo.png"]);
        assert!(!img[0].is_dir);
    }

    #[test]
    fn unknown_prefix_gives_empty_view() {
        let entries = vec![Entry::file("a.bin")];
        assert!(view_at(&entries, "nope/").is_empty());
    }

    #[test]
    fn large_flat_archive_stays_linear_and_unique() {
        // Плоский архив: каждый файл — запись верхнего уровня, плюс повтор
        // явным каталогом. Индекс по имени не должен задваивать записи.
        let mut entries: Vec<Entry> = (0..20_000)
            .map(|index| Entry::file(format!("file{index:05}.txt")))
            .collect();
        entries.push(Entry::dir("file00000.txt"));
        let root = root_view(entries);
        assert_eq!(root.len(), 20_000);
        assert!(
            root.iter()
                .find(|e| e.name == "file00000.txt")
                .unwrap()
                .is_dir
        );
    }

    #[test]
    fn prefix_without_trailing_slash_works_too() {
        let entries = vec![Entry::file("docs/readme.txt")];
        assert_eq!(names(&view_at(&entries, "docs")), ["readme.txt"]);
    }

    #[test]
    fn parent_prefix_walks_up() {
        assert_eq!(parent_prefix("docs/img/").as_deref(), Some("docs/"));
        assert_eq!(parent_prefix("docs/").as_deref(), Some(""));
        assert_eq!(parent_prefix(""), None);
        assert_eq!(parent_prefix("docs"), Some(String::new()));
    }

    #[test]
    fn child_prefix_joins_both_ways() {
        assert_eq!(child_prefix("", "docs"), "docs/");
        assert_eq!(child_prefix("docs/", "img"), "docs/img/");
        assert_eq!(child_prefix("docs", "img"), "docs/img/");
    }

    #[test]
    fn navigation_round_trip_returns_to_start() {
        let mut prefix = child_prefix("", "docs");
        prefix = child_prefix(&prefix, "img");
        assert_eq!(prefix, "docs/img/");

        prefix = parent_prefix(&prefix).unwrap();
        assert_eq!(prefix, "docs/");

        prefix = parent_prefix(&prefix).unwrap();
        assert_eq!(prefix, "");
        assert_eq!(parent_prefix(&prefix), None);
    }
}

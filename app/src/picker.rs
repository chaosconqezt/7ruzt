use crate::prelude::*;
/// Кому вернуть выбранную папку.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PickerTarget {
    Transfer,
    Extract,
}

/// Дерево каталогов для выбора папки (как «Обзор папок» в 7-Zip).
///
/// Узлы и раскрытие ведёт кит (`TreeState`), мы храним только то, чего в ките
/// нет: какие каталоги уже прочитаны с диска и куда вернуть выбор.
pub(crate) struct FolderPicker {
    pub(crate) target: PickerTarget,
    /// Состояние дерева кита: выделение, прокрутка, мышь — его.
    pub(crate) tree: Entity<TreeState>,
    /// Какие узлы развёрнуты. Дети читаются только для них — иначе обход
    /// всего диска на каждое открытие.
    pub(crate) expanded: BTreeSet<PathBuf>,
    /// Поколение пересборки: устаревший фоновый результат не применяется.
    generation: u64,
    /// Что выделить после первой загрузки (стартовый каталог).
    pending_select: Option<SharedString>,
}

/// Корни дерева папок — диски (Windows) или корень файловой системы (Unix).
#[cfg(windows)]
pub(crate) fn drive_roots() -> Vec<PathBuf> {
    (b'C'..=b'Z')
        .map(|letter| format!("{}:\\", letter as char))
        .filter(|path| Path::new(path).is_dir())
        .map(PathBuf::from)
        .collect()
}

#[cfg(not(windows))]
pub(crate) fn drive_roots() -> Vec<PathBuf> {
    vec![PathBuf::from("/")]
}

pub(crate) fn folder_has_children(path: &Path) -> bool {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .any(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        })
        .unwrap_or(false)
}

pub(crate) fn folder_children(path: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
                .map(|entry| entry.path())
                .collect()
        })
        .unwrap_or_default();
    dirs.sort_by_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    });
    dirs
}

pub(crate) fn folder_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Идентификатор узла — путь. Так дерево и файловая система говорят на одном
/// языке: выбранный узел сразу превращается в `PathBuf` без словаря.
pub(crate) fn folder_id(path: &Path) -> SharedString {
    SharedString::from(path.to_string_lossy().into_owned())
}

/// Узел дерева: у развёрнутого — настоящие дети, у свёрнутого — заглушка.
///
/// Кит считает папкой только узел с детьми (`TreeItem::is_folder`), а читать
/// весь диск на открытии нельзя. Поэтому у свёрнутой папки лежит один погашенный
/// ребёнок «…»: он даёт стрелку и раскрытие, а при `Expanded` заменяется детьми.
///
/// Scan-представление отдельно от `TreeItem`: китовый узел содержит `Rc`
/// и не пересекает потоки, а обход диска должен идти в фоне.
struct FolderNode {
    path: PathBuf,
    expanded: bool,
    children: Vec<FolderNode>,
    has_children: bool,
}

fn scan_folder(path: &Path, expanded: &BTreeSet<PathBuf>) -> FolderNode {
    if expanded.contains(path) {
        FolderNode {
            path: path.to_path_buf(),
            expanded: true,
            children: folder_children(path)
                .into_iter()
                .map(|child| scan_folder(&child, expanded))
                .collect(),
            has_children: false,
        }
    } else {
        FolderNode {
            path: path.to_path_buf(),
            expanded: false,
            children: Vec::new(),
            has_children: folder_has_children(path),
        }
    }
}

/// Корни дерева и всё, что развёрнуто под ними. Блокирующая: звать из фона.
fn scan_tree(expanded: &BTreeSet<PathBuf>) -> Vec<FolderNode> {
    drive_roots()
        .into_iter()
        .map(|root| scan_folder(&root, expanded))
        .collect()
}

fn node_to_item(node: &FolderNode) -> TreeItem {
    let item = TreeItem::new(folder_id(&node.path), folder_label(&node.path));
    if node.expanded {
        item.expanded(true)
            .children(node.children.iter().map(node_to_item))
    } else if node.has_children {
        item.child(
            TreeItem::new(
                SharedString::from(format!("{}::placeholder", folder_id(&node.path))),
                "…",
            )
            .disabled(true),
        )
    } else {
        item
    }
}

impl MainWindow {
    /// «…»: открыть дерево выбора папки для диалога копирования/извлечения.
    pub(crate) fn open_picker(
        &mut self,
        target: PickerTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Фокус забираем у поля ввода под диалогом: иначе набор текста уходил
        // бы в невидимое поле, пока открыто дерево.
        window.focus(&self.focus, cx);
        let start = {
            let value = match (target, &self.modal) {
                (PickerTarget::Transfer, Some(Modal::Transfer(dialog))) => {
                    Some(dialog.destination.read(cx).value().to_string())
                }
                (PickerTarget::Extract, Some(Modal::Extract(dialog))) => {
                    Some(dialog.destination.read(cx).value().to_string())
                }
                _ => None,
            };
            value
                .map(PathBuf::from)
                .filter(|path| path.is_dir())
                .unwrap_or_else(|| self.active().path.clone())
        };

        // Разворачиваем цепочку до стартового каталога, чтобы он был виден
        // сразу: иначе дерево открывается свёрнутым на диске.
        let mut expanded = std::collections::BTreeSet::new();
        for ancestor in start.ancestors() {
            expanded.insert(ancestor.to_path_buf());
        }

        let start_id = folder_id(&start);
        // Узлы читаются в фоне: опрос дисков и read_dir не морозят окно.
        let tree = cx.new(|cx| TreeState::new(cx));

        // Раскрытие и сворачивание ведёт кит; нам остаётся прочитать детей.
        // Пересборку откладываем: событие приходит из `toggle_expand`, где
        // `TreeState` уже занят, и обновлять его в этот момент нельзя.
        let weak = cx.entity().downgrade();
        cx.subscribe(&tree, move |_this, _, event: &TreeEvent, cx| {
            let (id, expand) = match event {
                TreeEvent::Expanded(id) => (id.to_string(), true),
                TreeEvent::Collapsed(id) => (id.to_string(), false),
            };
            let weak = weak.clone();
            cx.defer(move |cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.picker_set_expanded(&id, expand, cx);
                    cx.notify();
                });
            });
        })
        .detach();

        self.folder_picker = Some(FolderPicker {
            target,
            tree,
            expanded,
            generation: 0,
            pending_select: Some(start_id),
        });
        self.rebuild_picker_items(cx);
    }

    /// Пересборка узлов дерева в фоне. Результат применяется, только если
    /// дерево ещё то же самое и поколение не устарело (быстрые toggle'ы).
    fn rebuild_picker_items(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = self.folder_picker.as_mut() else {
            return;
        };
        picker.generation = picker.generation.wrapping_add(1);
        let generation = picker.generation;
        let expanded = picker.expanded.clone();
        let tree = picker.tree.clone();
        cx.spawn(async move |this, cx| {
            let nodes = cx
                .background_executor()
                .spawn(async move { scan_tree(&expanded) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(picker) = this.folder_picker.as_mut() else {
                    return;
                };
                if picker.generation != generation || picker.tree != tree {
                    return;
                }
                let tree = picker.tree.clone();
                let select_id = picker
                    .pending_select
                    .take()
                    .or_else(|| tree.read(cx).selected_item().map(|item| item.id.clone()));
                let items: Vec<TreeItem> = nodes.iter().map(node_to_item).collect();
                tree.update(cx, |state, cx| {
                    state.set_items(items, cx);
                    if let Some(id) = select_id
                        && let Some(index) = state.index_of(&id)
                    {
                        state.set_selected_index(Some(index), cx);
                        state.scroll_to_item(index, ScrollStrategy::Nearest);
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Дерево сообщило, что узел раскрыт/свёрнут: обновляем набор и перечитываем
    /// детей. Выделение и прокрутку сохраняем по id — иначе они сбросятся.
    pub(crate) fn picker_set_expanded(&mut self, id: &str, expand: bool, cx: &mut Context<Self>) {
        let Some(picker) = self.folder_picker.as_mut() else {
            return;
        };
        let path = PathBuf::from(id);
        if expand {
            picker.expanded.insert(path);
        } else {
            picker.expanded.remove(&path);
        }
        self.rebuild_picker_items(cx);
    }

    /// Стрелки в дереве. Клавиатуру слушает корень окна, поэтому двигаем
    /// выделение сами; мышь и раскрытие — на ките.
    pub(crate) fn picker_step(&mut self, up: bool, cx: &mut Context<Self>) {
        let Some(picker) = self.folder_picker.as_ref() else {
            return;
        };
        let tree = picker.tree.clone();
        tree.update(cx, |state, cx| {
            let mut len = 0;
            while state.entry(len).is_some() {
                len += 1;
            }
            if len == 0 {
                return;
            }
            let next = match (state.selected_index(), up) {
                (Some(index), true) => index.saturating_sub(1),
                (Some(index), false) => (index + 1).min(len - 1),
                (None, _) => 0,
            };
            state.set_selected_index(Some(next), cx);
            state.scroll_to_item(next, ScrollStrategy::Nearest);
        });
    }

    /// «OK» в дереве: подставляем выбранный путь в поле диалога.
    /// Если ничего не выделено, дерево остаётся открытым — закрывать нечего.
    pub(crate) fn picker_ok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.folder_picker.as_ref() else {
            return;
        };
        let Some(path) = picker
            .tree
            .read(cx)
            .selected_item()
            .map(|item| PathBuf::from(item.id.as_ref()))
        else {
            return;
        };
        let target = picker.target;
        self.folder_picker = None;
        let text = path.display().to_string();

        match target {
            PickerTarget::Transfer => {
                if let Some(Modal::Transfer(dialog)) = &mut self.modal {
                    dialog
                        .destination
                        .update(cx, |state, cx| state.set_value(text, window, cx));
                    dialog.error = None;
                }
            }
            PickerTarget::Extract => {
                if let Some(Modal::Extract(dialog)) = &mut self.modal {
                    dialog
                        .destination
                        .update(cx, |state, cx| state.set_value(text, window, cx));
                }
            }
        }
    }
}

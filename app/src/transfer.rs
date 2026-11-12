use crate::prelude::*;
/// Копирование или перемещение выделенного в выбранный каталог.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferKind {
    Copy,
    Move,
}

impl TransferKind {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Copy => "Копировать в…",
            Self::Move => "Переместить в…",
        }
    }

    pub(crate) fn button(self) -> &'static str {
        match self {
            Self::Copy => "Копировать",
            Self::Move => "Переместить",
        }
    }

    pub(crate) fn job(self) -> JobKind {
        match self {
            Self::Copy => JobKind::Copy,
            Self::Move => JobKind::Move,
        }
    }
}

/// Куда именно уедет выделение.
pub(crate) enum TransferTarget {
    /// Каталог: копируем внутрь (существующий или только что созданный).
    IntoDir(PathBuf),
    /// Точный путь: один объект копируется как новый файл/папка с этим именем.
    AsPath(PathBuf),
}

/// Политика диалога извлечения → политика копирования (одинаковые по смыслу).
pub(crate) fn conflict_policy(overwrite: Overwrite) -> ConflictPolicy {
    match overwrite {
        Overwrite::All => ConflictPolicy::Replace,
        Overwrite::Skip => ConflictPolicy::Skip,
        Overwrite::Rename => ConflictPolicy::Rename,
    }
}

/// Что делать с путём назначения — как `cp` и 7-Zip:
///
/// * существующий каталог → копируем внутрь;
/// * путь со слэшем на конце или несколько объектов → создаём каталог и копируем внутрь;
/// * один объект в несуществующий путь → копируем как новый файл/папку с этим именем.
pub(crate) fn plan_transfer(
    sources: &[PathBuf],
    destination: &str,
) -> Result<TransferTarget, String> {
    let raw = destination.trim();
    if raw.is_empty() {
        return Err("Куда копировать — пусто".to_string());
    }
    let path = PathBuf::from(raw);
    let trailing_separator = raw.ends_with('\\') || raw.ends_with('/');

    if path.is_dir() {
        return Ok(TransferTarget::IntoDir(path));
    }
    if path.exists() {
        return if sources.len() == 1 {
            Ok(TransferTarget::AsPath(path))
        } else {
            Err("Нужен каталог: выделено несколько объектов".to_string())
        };
    }

    if trailing_separator || sources.len() > 1 {
        std::fs::create_dir_all(&path)
            .map_err(|err| format!("Не удалось создать каталог: {}", fs_error(&err)))?;
        return Ok(TransferTarget::IntoDir(path));
    }
    Ok(TransferTarget::AsPath(path))
}

fn validate_archive_move(destination: &Path, sources: &[PathBuf]) -> Result<(), String> {
    let destination = if destination.exists() {
        destination.canonicalize()
    } else {
        destination
            .parent()
            .unwrap_or(Path::new("."))
            .canonicalize()
            .map(|parent| parent.join(destination.file_name().unwrap_or_default()))
    }
    .map_err(|error| format!("Не удалось проверить путь архива: {}", fs_error(&error)))?;
    for source in sources {
        let source = source
            .canonicalize()
            .map_err(|error| format!("Не удалось проверить исходный путь: {}", fs_error(&error)))?;
        if destination.starts_with(&source) {
            return Err(
                "Нельзя переместить папку в архив внутри неё или архив в самого себя".to_string(),
            );
        }
    }
    Ok(())
}

impl MainWindow {
    /// F5/F6: копировать или переместить выделенное в выбранный каталог.
    pub(crate) fn open_transfer(
        &mut self,
        kind: TransferKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.job.is_some() {
            self.notice = Some("Дождитесь окончания текущей операции".to_string());
            return;
        }
        if self.active().is_archive {
            self.notice =
                Some("Внутри архива операции с файлами недоступны: сначала извлеките".to_string());
            return;
        }

        let panel = self.active();
        let sources: Vec<PathBuf> = panel
            .selection
            .iter()
            .filter_map(|index| panel.entries.get(*index))
            .map(|entry| panel.path.join(&entry.name))
            .collect();
        if sources.is_empty() {
            self.notice = Some("Ничего не выделено".to_string());
            return;
        }

        // В двухпанельном режиме «Куда» по умолчанию — вторая панель: ради
        // этого F5/F6 и нажимают. Если там архив, копировать некуда — остаёмся
        // в текущем каталоге.
        let other = 1 - self.active_panel;
        let default_dir = if self.two_panel && !self.panels[other].is_archive {
            self.panels[other].path.clone()
        } else {
            panel.path.clone()
        };

        let destination = cx
            .new(|cx| InputState::new(window, cx).default_value(default_dir.display().to_string()));

        cx.subscribe_in(
            &destination,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_transfer(cx);
                    if this.modal.is_none() {
                        window.focus(&this.focus, cx);
                    }
                }
            },
        )
        .detach();

        let focus = destination.read(cx).focus_handle(cx);
        window.focus(&focus, cx);

        self.notice = None;
        self.modal = Some(Modal::Transfer(TransferDialog {
            kind,
            sources,
            destination,
            // Не перезаписывать — безопасный вариант по умолчанию.
            overwrite: Overwrite::Skip,
            error: None,
        }));
    }

    pub(crate) fn submit_transfer(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::Transfer(mut dialog)) = self.modal.take() else {
            return;
        };

        let destination = dialog.destination.read(cx).value().trim().to_string();
        let sources = dialog.sources.clone();
        let kind = dialog.kind;
        let overwrite = dialog.overwrite;
        // Планирование — дисковый IO (is_dir/exists/create_dir_all): на сетевых
        // путях может блокировать, поэтому в фон. Диалог на это время закрыт;
        // при ошибке планирования вернётся с красной строкой.
        cx.spawn(async move |this, cx| {
            let plan_sources = sources.clone();
            let planned = cx
                .background_executor()
                .spawn(async move { plan_transfer(&plan_sources, &destination) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match planned {
                    Ok(target) => {
                        this.start_transfer(kind, sources, target, conflict_policy(overwrite), cx);
                        if let Some(job) = &mut this.job {
                            job.retry_dialog = Some(Modal::Transfer(dialog));
                        }
                    }
                    Err(err) => {
                        dialog.error = Some(err);
                        this.modal = Some(Modal::Transfer(dialog));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Добавление файлов в существующий или новый архив.
    /// При `move_files == true` (зажат Shift) исходные файлы удаляются после успешного добавления.
    pub(crate) fn add_to_archive(
        &mut self,
        archive_path: PathBuf,
        mut sources: Vec<PathBuf>,
        move_files: bool,
        cx: &mut Context<Self>,
    ) {
        if self.job.is_some() {
            self.notice = Some("Дождитесь окончания текущей операции".to_string());
            return;
        }

        if move_files && let Err(error) = validate_archive_move(&archive_path, &sources) {
            self.notice = Some(error);
            return;
        }
        // Сравнение строк обходится относительным путём или алиасом регистра:
        // нормализуем обе стороны (канонизация, для нового архива — абсолютный путь).
        let normalized = |path: &Path| {
            std::fs::canonicalize(path).unwrap_or_else(|_| {
                std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
            })
        };
        let archive_normalized = normalized(&archive_path);
        sources.retain(|source| normalized(source) != archive_normalized);
        if sources.is_empty() {
            self.notice = Some("Нельзя упаковать архив сам в себя".to_string());
            return;
        }

        let request = CreateRequest::new(&archive_path, sources.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let target_display = archive_path.clone();

        let receiver = spawn_job(move |send| {
            let tool = resolve_tool()?;
            let exe = tool.info.exe.clone();
            let utf8 = tool.info.version.supports_utf8_console();
            let (bytes, files) = scan_totals(&request.sources);
            send(JobMessage::Totals { bytes, files });
            let mut progress = |event: Progress| send(JobMessage::Progress(event));

            match create(&exe, &request, utf8, &flag, &mut progress) {
                Ok(summary) => {
                    let cancelled = summary.cancelled || flag.load(Ordering::Relaxed);
                    let mut warnings = summary.warnings;
                    if move_files && !cancelled {
                        if summary.code == 0 {
                            let deleted = delete_paths(&request.sources);
                            for (path, error) in deleted.errors {
                                if !warnings.is_empty() {
                                    warnings.push('\n');
                                }
                                warnings.push_str(&format!(
                                    "Архив создан, но не удалось удалить {}: {}",
                                    path.display(),
                                    fs_error(&error)
                                ));
                            }
                        } else {
                            if !warnings.is_empty() {
                                warnings.push('\n');
                            }
                            warnings.push_str(
                                "Исходные файлы сохранены: упаковка завершилась с предупреждениями",
                            );
                        }
                    }
                    Ok(JobOutcome {
                        cancelled,
                        warnings,
                    })
                }
                Err(err) => Err(err.to_string()),
            }
        });

        self.ui_log(&format!(
            "add to archive={} sources={} move={move_files}",
            archive_path.display(),
            sources.len(),
        ));
        self.notice = None;
        self.start_job(JobKind::Create, target_display, cancel, receiver, cx);
    }

    /// Файлы, перетащенные из другой панели.
    ///
    /// Цель зависит от точки отпускания:
    /// * строка архива или открытая панель архива — файлы добавляются в архив
    ///   (с Shift — перемещаются в архив, то есть удаляются из источника);
    /// * строка папки — копируются внутрь неё (с Shift — перемещаются);
    /// * пустое место панели — в текущий каталог панели.
    pub(crate) fn drop_files_on_panel(
        &mut self,
        files: &[(PathBuf, bool)],
        panel_index: usize,
        position: Point<Pixels>,
        move_files: bool,
        cx: &mut Context<Self>,
    ) {
        if self.job.is_some() {
            self.notice = Some("Дождитесь окончания текущей операции".to_string());
            return;
        }

        if files.is_empty() {
            return;
        }

        let panel = self.panel(panel_index);

        let over_list = {
            let state = panel.list_scroll.0.borrow();
            let bounds = state.base_handle.bounds();
            let x = f32::from(position.x);
            x >= f32::from(bounds.left()) && x <= f32::from(bounds.right())
        };
        let over_row = over_list && !panel.below_rows(position.y);
        let target_entry = over_row
            .then(|| panel.entry_at(position))
            .flatten()
            .and_then(|index| panel.entries.get(index));

        let sources: Vec<PathBuf> = files.iter().map(|(path, _)| path.clone()).collect();
        let target_spec = target_entry.map(|e| (e.name.as_str(), e.is_dir));

        let action = match resolve_drop(
            panel.is_archive,
            &panel.path,
            target_spec,
            &sources,
            move_files,
        ) {
            Ok(action) => action,
            Err(DropError::AlreadyInTargetFolder) => {
                self.notice = Some("Объекты уже в этой папке".to_string());
                return;
            }
            Err(DropError::CannotArchiveSelf) => {
                self.notice = Some("Нельзя упаковать архив сам в себя".to_string());
                return;
            }
        };

        match action {
            DropAction::CopyInto(target) => {
                self.ui_log(&format!(
                    "drop copy files={} -> {}",
                    sources.len(),
                    target.display()
                ));
                self.start_transfer(
                    TransferKind::Copy,
                    sources,
                    TransferTarget::IntoDir(target),
                    ConflictPolicy::Skip,
                    cx,
                );
            }
            DropAction::MoveInto(target) => {
                self.ui_log(&format!(
                    "drop move files={} -> {}",
                    sources.len(),
                    target.display()
                ));
                self.start_transfer(
                    TransferKind::Move,
                    sources,
                    TransferTarget::IntoDir(target),
                    ConflictPolicy::Skip,
                    cx,
                );
            }
            DropAction::AddToArchive {
                archive_path,
                move_files,
            } => {
                self.add_to_archive(archive_path, sources, move_files, cx);
            }
        }
    }

    /// Записи архива, перетащенные из панели архива на папку.
    ///
    /// Распаковываем **только** выбранные записи: их имена уходят в 7z после
    /// `--`, поэтому сброс одного файла не вытаскивает весь архив. Цель — как
    /// у файлов: строка папки или текущий каталог панели. На архивный файл не
    /// сбрасываем: это не папка, а добавлять записи в архив — другая операция.
    pub(crate) fn drop_archive_entries(
        &mut self,
        archive: &Path,
        entries: &[String],
        encrypted: bool,
        panel_index: usize,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self.job.is_some() {
            self.notice = Some("Дождитесь окончания текущей операции".to_string());
            return;
        }
        if entries.is_empty() {
            self.notice = Some("Ничего не выделено".to_string());
            return;
        }
        if encrypted {
            self.notice = Some(
                "Архив зашифрован: распакуйте через «Извлечь», чтобы ввести пароль".to_string(),
            );
            return;
        }

        let target_dir = {
            let panel = self.panel(panel_index);
            if panel.is_archive {
                self.notice =
                    Some("Записи архива распаковываются в папку, а не в другой архив".to_string());
                return;
            }
            let over_list = {
                let state = panel.list_scroll.0.borrow();
                let bounds = state.base_handle.bounds();
                let x = f32::from(position.x);
                x >= f32::from(bounds.left()) && x <= f32::from(bounds.right())
            };
            let over_row = over_list && !panel.below_rows(position.y);
            let target_entry = over_row
                .then(|| panel.entry_at(position))
                .flatten()
                .and_then(|index| panel.entries.get(index));
            match target_entry {
                Some(entry) if entry.is_dir => panel.path.join(&entry.name),
                _ => panel.path.clone(),
            }
        };

        let mut request = ExtractRequest::new(archive, &target_dir);
        request.entries = entries.to_vec();
        // Сброс мышью не должен молча стирать существующие файлы.
        request.overwrite = Overwrite::Skip;

        self.ui_log(&format!(
            "drop archive entries={} -> {}",
            entries.len(),
            target_dir.display()
        ));
        self.start_extract(request, cx);
    }

    /// Запускает копирование/перемещение в фоне: у больших каталогов это минуты.
    pub(crate) fn start_transfer(
        &mut self,
        kind: TransferKind,
        sources: Vec<PathBuf>,
        target: TransferTarget,
        policy: ConflictPolicy,
        cx: &mut Context<Self>,
    ) {
        if self.job.is_some() {
            return;
        }

        let job_target = match &target {
            TransferTarget::IntoDir(path) | TransferTarget::AsPath(path) => path.clone(),
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let receiver = spawn_job(move |send| {
            let (bytes, files) = scan_totals(&sources);
            send(JobMessage::Totals { bytes, files });
            let mut progress = |event: Progress| send(JobMessage::Progress(event));

            match &target {
                TransferTarget::IntoDir(dir) => {
                    let summary = match kind {
                        TransferKind::Copy => {
                            copy_paths(&sources, dir, policy, &flag, &mut progress)
                        }
                        TransferKind::Move => {
                            move_paths(&sources, dir, policy, &flag, &mut progress)
                        }
                    };
                    if summary.cancelled {
                        return Ok(JobOutcome {
                            cancelled: true,
                            warnings: String::new(),
                        });
                    }
                    if let Some((path, err)) = summary.errors.first() {
                        return Err(format!("«{}»: {}", name_of(path), fs_error(err)));
                    }
                    Ok(JobOutcome {
                        cancelled: false,
                        warnings: if summary.skipped > 0 {
                            format!("пропущено: {}", summary.skipped)
                        } else {
                            String::new()
                        },
                    })
                }
                TransferTarget::AsPath(destination) => {
                    let Some(source) = sources.first() else {
                        return Err("ничего не выделено".to_string());
                    };
                    let result = match kind {
                        TransferKind::Copy => {
                            copy_to(source, destination, policy, &flag, &mut progress)
                        }
                        TransferKind::Move => {
                            move_to(source, destination, policy, &flag, &mut progress)
                        }
                    };
                    match result {
                        Ok(Some(_)) => Ok(JobOutcome {
                            cancelled: false,
                            warnings: String::new(),
                        }),
                        Ok(None) => Ok(JobOutcome {
                            cancelled: false,
                            warnings: "пропущено: 1".to_string(),
                        }),
                        Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {
                            Ok(JobOutcome {
                                cancelled: true,
                                warnings: String::new(),
                            })
                        }
                        Err(err) => Err(format!("«{}»: {}", name_of(source), fs_error(&err))),
                    }
                }
            }
        });

        self.notice = None;
        self.start_job(kind.job(), job_target, cancel, receiver, cx);
    }
}

/// Какое действие выполнять при перетаскивании.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DropAction {
    /// Копировать в каталог
    CopyInto(PathBuf),
    /// Переместить в каталог
    MoveInto(PathBuf),
    /// Добавить файлы в архив (с `move_files: true` — удалить исходники после добавления)
    AddToArchive {
        archive_path: PathBuf,
        move_files: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DropError {
    AlreadyInTargetFolder,
    CannotArchiveSelf,
}

/// Определение целевого действия при сбросе файлов на панель.
pub(crate) fn resolve_drop(
    is_panel_archive: bool,
    panel_path: &Path,
    target_entry: Option<(&str, bool)>, // (name, is_dir)
    sources: &[PathBuf],
    shift: bool,
) -> Result<DropAction, DropError> {
    if is_panel_archive {
        let archive_path = panel_path.to_path_buf();
        if sources.iter().any(|s| s == &archive_path) {
            return Err(DropError::CannotArchiveSelf);
        }
        return Ok(DropAction::AddToArchive {
            archive_path,
            move_files: shift,
        });
    }

    match target_entry {
        Some((name, false)) if is_archive_name(name) => {
            let archive_path = panel_path.join(name);
            if sources.iter().any(|s| s == &archive_path) {
                return Err(DropError::CannotArchiveSelf);
            }
            Ok(DropAction::AddToArchive {
                archive_path,
                move_files: shift,
            })
        }
        Some((name, true)) => {
            let target_dir = panel_path.join(name);
            if sources.iter().all(|path| parent_is(path, &target_dir)) {
                return Err(DropError::AlreadyInTargetFolder);
            }
            if shift {
                Ok(DropAction::MoveInto(target_dir))
            } else {
                Ok(DropAction::CopyInto(target_dir))
            }
        }
        _ => {
            let target_dir = panel_path.to_path_buf();
            if sources.iter().all(|path| parent_is(path, &target_dir)) {
                return Err(DropError::AlreadyInTargetFolder);
            }
            if shift {
                Ok(DropAction::MoveInto(target_dir))
            } else {
                Ok(DropAction::CopyInto(target_dir))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DropAction, DropError, resolve_drop, validate_archive_move};
    use std::path::{Path, PathBuf};

    #[test]
    fn archive_move_rejects_self_and_destination_inside_source() {
        let directory = crate::fsutil::temp_open_dir().unwrap();
        let source = directory.join("source");
        std::fs::create_dir(&source).unwrap();
        let archive = directory.join("out.7z");
        std::fs::write(&archive, b"archive").unwrap();
        assert!(validate_archive_move(&archive, std::slice::from_ref(&archive)).is_err());
        assert!(
            validate_archive_move(&source.join("new.7z"), std::slice::from_ref(&source)).is_err()
        );
        assert!(validate_archive_move(&archive, std::slice::from_ref(&source)).is_ok());
        assert!(
            validate_archive_move(&directory.join("new.7z"), std::slice::from_ref(&source)).is_ok()
        );
        assert_eq!(std::fs::read(&archive).unwrap(), b"archive");
        assert!(source.is_dir());
        std::fs::remove_file(archive).unwrap();
        std::fs::remove_dir(source).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn drop_on_folder_copies_without_shift() {
        let panel_path = Path::new("C:\\work");
        let sources = vec![PathBuf::from("C:\\other\\file.txt")];
        let action =
            resolve_drop(false, panel_path, Some(("docs", true)), &sources, false).unwrap();
        assert_eq!(
            action,
            DropAction::CopyInto(PathBuf::from("C:\\work\\docs"))
        );
    }

    #[test]
    fn drop_on_folder_moves_with_shift() {
        let panel_path = Path::new("C:\\work");
        let sources = vec![PathBuf::from("C:\\other\\file.txt")];
        let action = resolve_drop(false, panel_path, Some(("docs", true)), &sources, true).unwrap();
        assert_eq!(
            action,
            DropAction::MoveInto(PathBuf::from("C:\\work\\docs"))
        );
    }

    #[test]
    fn drop_on_archive_adds_without_shift() {
        let panel_path = Path::new("C:\\work");
        let sources = vec![PathBuf::from("C:\\other\\file.txt")];
        let action = resolve_drop(
            false,
            panel_path,
            Some(("backup.7z", false)),
            &sources,
            false,
        )
        .unwrap();
        assert_eq!(
            action,
            DropAction::AddToArchive {
                archive_path: PathBuf::from("C:\\work\\backup.7z"),
                move_files: false,
            }
        );
    }

    #[test]
    fn drop_on_archive_moves_with_shift() {
        let panel_path = Path::new("C:\\work");
        let sources = vec![PathBuf::from("C:\\other\\file.txt")];
        let action =
            resolve_drop(false, panel_path, Some(("data.zip", false)), &sources, true).unwrap();
        assert_eq!(
            action,
            DropAction::AddToArchive {
                archive_path: PathBuf::from("C:\\work\\data.zip"),
                move_files: true,
            }
        );
    }

    #[test]
    fn drop_inside_archive_adds_files() {
        let panel_path = Path::new("C:\\work\\archive.7z");
        let sources = vec![PathBuf::from("C:\\other\\file.txt")];
        let action = resolve_drop(true, panel_path, None, &sources, false).unwrap();
        assert_eq!(
            action,
            DropAction::AddToArchive {
                archive_path: PathBuf::from("C:\\work\\archive.7z"),
                move_files: false,
            }
        );
    }

    #[test]
    fn drop_archive_onto_itself_is_refused() {
        let panel_path = Path::new("C:\\work");
        let self_archive = PathBuf::from("C:\\work\\archive.7z");
        let sources = vec![self_archive.clone()];
        let err = resolve_drop(
            false,
            panel_path,
            Some(("archive.7z", false)),
            &sources,
            false,
        )
        .unwrap_err();
        assert_eq!(err, DropError::CannotArchiveSelf);
    }

    #[test]
    fn drop_already_in_folder_is_refused() {
        let panel_path = Path::new("C:\\work");
        let sources = vec![PathBuf::from("C:\\work\\file.txt")];
        let err = resolve_drop(false, panel_path, None, &sources, false).unwrap_err();
        assert_eq!(err, DropError::AlreadyInTargetFolder);
    }
}

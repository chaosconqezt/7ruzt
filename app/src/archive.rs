use crate::prelude::*;
impl MainWindow {
    /// Открывает диалог извлечения с разумными значениями по умолчанию.
    pub(crate) fn open_extract_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.job.is_some() {
            self.notice = Some("Дождитесь окончания текущей операции".to_string());
            return;
        }
        let Some(archive) = self.action_archive() else {
            self.notice = Some("Откройте архив или выделите файл архива".to_string());
            return;
        };

        // Куда по умолчанию: во второй панели — туда (для этого режим и нужен),
        // иначе рядом с архивом. Подпапку с именем архива 7-Zip предлагает
        // галочкой (в скриншоте — «rox\»), а не зашивает в путь.
        let stem = archive
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unpacked".to_string());
        let other = 1 - self.active_panel;
        let two_panel_target = self.two_panel && !self.panels[other].is_archive;
        let base = if two_panel_target {
            self.panels[other].path.clone()
        } else {
            archive
                .parent()
                .map(|path| path.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        };
        // Поле показывает базовую папку, а галочка добавляет к ней подпапку —
        // как в 7-Zip: на скриншоте путь без «rox\», хотя галочка стоит.
        let subfolder = !two_panel_target;

        // Что именно распаковываем — фиксируем сейчас, а не при подтверждении:
        // выделение за время открытого диалога не должно менять содержимое.
        let entries = if self.active().is_archive {
            self.active().selected_paths()
        } else {
            Vec::new()
        };

        let destination =
            cx.new(|cx| InputState::new(window, cx).default_value(base.display().to_string()));
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("пароль")
                .masked(true)
        });

        let paths_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(PATH_MODES.to_vec()),
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        let overwrite_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(
                    OVERWRITE_MODES
                        .iter()
                        .map(|(_, label)| *label)
                        .collect::<Vec<_>>(),
                ),
                Some(IndexPath::new(overwrite_index(Overwrite::Skip))),
                window,
                cx,
            )
        });

        cx.subscribe(
            &paths_select,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(value)) = event
                    && let Some(Modal::Extract(dialog)) = &mut this.modal
                {
                    dialog.path_mode = if *value == PATH_FLAT {
                        PathMode::Flat
                    } else {
                        PathMode::Full
                    };
                }
                cx.notify();
            },
        )
        .detach();
        cx.subscribe(
            &overwrite_select,
            |this, _, event: &SelectEvent<SearchableVec<&'static str>>, cx| {
                if let SelectEvent::Confirm(Some(value)) = event
                    && let Some(Modal::Extract(dialog)) = &mut this.modal
                    && let Some((mode, _)) =
                        OVERWRITE_MODES.iter().find(|(_, label)| label == value)
                {
                    dialog.overwrite = *mode;
                }
                cx.notify();
            },
        )
        .detach();

        for input in [&destination, &password] {
            cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_extract(cx);
                    if this.modal.is_none() {
                        window.focus(&this.focus, cx);
                    }
                }
            })
            .detach();
        }

        let focus = destination.read(cx).focus_handle(cx);
        window.focus(&focus, cx);

        self.notice = None;
        self.modal = Some(Modal::Extract(ExtractDialog {
            error: None,
            archive,
            entries,
            destination,
            password,
            // Не перезаписывать — безопасный вариант по умолчанию: клик не должен стирать файлы.
            overwrite: Overwrite::Skip,
            path_mode: PathMode::Full,
            subfolder,
            subfolder_name: stem,
            paths_select,
            overwrite_select,
            show_password: false,
        }));
    }

    pub(crate) fn submit_extract(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::Extract(mut dialog)) = self.modal.take() else {
            return;
        };

        let destination = dialog.destination.read(cx).value().to_string();
        let password = dialog.password.read(cx).value().to_string();

        if destination.trim().is_empty() {
            dialog.error = Some("Куда распаковывать — пусто".to_string());
            self.modal = Some(Modal::Extract(dialog));
            cx.notify();
            return;
        }

        // Итоговый путь = поле + подпапка, если галочка стоит. Поле остаётся
        // базовой папкой — так 7-Zip показывает его в диалоге.
        let base = PathBuf::from(destination.trim());
        let target = if dialog.subfolder {
            base.join(&dialog.subfolder_name)
        } else {
            base
        };

        let mut request = ExtractRequest::new(&dialog.archive, target);
        request.entries = dialog.entries.clone();
        request.password = (!password.is_empty()).then_some(password);
        request.overwrite = dialog.overwrite;
        request.path_mode = dialog.path_mode;
        self.start_extract(request, cx);
        if let Some(job) = &mut self.job {
            job.retry_dialog = Some(Modal::Extract(dialog));
        } else {
            dialog.error = self.notice.clone();
            self.modal = Some(Modal::Extract(dialog));
        }
        cx.notify();
    }

    /// Запускает распаковку архива в указанное место.
    ///
    /// Работа идёт в отдельном потоке: 7z на гигабайтах живёт минутами, и морозить
    /// окно на это время нельзя. UI только читает канал опросом.
    pub(crate) fn start_extract(&mut self, request: ExtractRequest, cx: &mut Context<Self>) {
        if self.job.is_some() {
            return;
        }

        // Имена записей приходят из чужого архива: traversal и запрещённые
        // имена должны быть отсечены на любом пути извлечения, не только при
        // одиночном открытии. «prefix*» — наша маска «всё в подпапке»:
        // проверяем её путь без хвостовой звёздочки.
        if let Some(bad) = request.entries.iter().find(|name| {
            let path = name.strip_suffix('*').unwrap_or(name).trim_end_matches('/');
            !path.is_empty() && !entry_path_is_safe(path)
        }) {
            self.notice = Some(format!(
                "Небезопасное имя записи «{bad}»: извлечение отменено"
            ));
            return;
        }

        let target = request.destination.clone();

        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let (total_bytes, total_files) = self.archive_totals();
        let receiver = spawn_job(move |send| {
            // Поиск 7z (`7z i`) тоже в фоне: первый запуск не морозит окно.
            let tool = resolve_tool()?;
            let exe = tool.info.exe.clone();
            let utf8 = tool.info.version.supports_utf8_console();
            send(JobMessage::Totals {
                bytes: total_bytes,
                files: total_files,
            });
            let mut progress = |event: Progress| send(JobMessage::Progress(event));
            match extract(&exe, &request, utf8, &flag, &mut progress) {
                Ok(summary) => Ok(JobOutcome {
                    cancelled: summary.cancelled,
                    warnings: summary.warnings,
                }),
                Err(err) => Err(err.to_string()),
            }
        });

        self.notice = None;
        self.start_job(JobKind::Extract, target, cancel, receiver, cx);
    }

    /// «Проверить архив»: `7z t`. Обычный архив проверяется сразу, шифрованный —
    /// через диалог пароля: без него 7z пытается читать пароль со stdin, а там `nul`.
    pub(crate) fn test_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.job.is_some() {
            self.notice = Some("Дождитесь окончания текущей операции".to_string());
            return;
        }
        let Some(archive) = self.action_archive() else {
            self.notice = Some("Откройте архив или выделите файл архива".to_string());
            return;
        };

        // Внутри архива шифрование уже известно из листинга; для файла в папке
        // спрашиваем листинг — это чтение заголовка, а не всего архива, и оно
        // уходит в фон: запуск 7z на UI-потоке морозил бы окно.
        let known = {
            let panel = self.active();
            (panel.is_archive && archive == panel.path).then_some(panel.encrypted)
        };
        match known {
            Some(encrypted) => self.test_after_probe(archive, encrypted, window, cx),
            None => {
                let probe_path = archive.clone();
                cx.spawn(async move |this, cx| {
                    let encrypted = cx
                        .background_executor()
                        .spawn(async move { probe_encrypted(&probe_path) })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.with_window(cx, move |this, window, cx| {
                            this.test_after_probe(archive, encrypted, window, cx);
                        });
                    });
                })
                .detach();
            }
        }
    }

    /// Продолжение «Проверить» после того, как шифрование стало известно.
    fn test_after_probe(
        &mut self,
        archive: PathBuf,
        encrypted: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.job.is_some() {
            self.notice = Some("Дождитесь окончания текущей операции".to_string());
            return;
        }
        if encrypted {
            self.open_test_dialog(archive, window, cx);
        } else {
            let entries = if self.active().is_archive {
                self.active().selected_paths()
            } else {
                Vec::new()
            };
            self.start_test(archive, entries, None, cx);
        }
    }

    pub(crate) fn open_test_dialog(
        &mut self,
        archive: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("пароль")
                .masked(true)
        });

        cx.subscribe(&password, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.submit_test(cx);
            }
        })
        .detach();

        let focus = password.read(cx).focus_handle(cx);
        window.focus(&focus, cx);

        self.notice = None;
        self.modal = Some(Modal::Test(TestDialog { archive, password }));
    }

    pub(crate) fn submit_test(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::Test(dialog)) = self.modal.take() else {
            return;
        };

        let password = dialog.password.read(cx).value().to_string();
        let entries = if self.active().is_archive {
            self.active().selected_paths()
        } else {
            Vec::new()
        };
        self.start_test(
            dialog.archive,
            entries,
            if password.is_empty() {
                None
            } else {
                Some(password)
            },
            cx,
        );
    }

    /// Проверка целостности архива в фоне: `7z t` пересчитывает CRC всех файлов.
    pub(crate) fn start_test(
        &mut self,
        archive: PathBuf,
        entries: Vec<String>,
        password: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.job.is_some() {
            return;
        }

        let mut request = TestRequest::new(&archive);
        request.password = password;
        // Несколько выделенных элементов — проверяем только их, как 7-Zip FM.
        request.entries = entries;

        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let (total_bytes, total_files) = self.archive_totals();
        let receiver = spawn_job(move |send| {
            let tool = resolve_tool()?;
            let exe = tool.info.exe.clone();
            let utf8 = tool.info.version.supports_utf8_console();
            send(JobMessage::Totals {
                bytes: total_bytes,
                files: total_files,
            });
            let mut progress = |event: Progress| send(JobMessage::Progress(event));
            match test_archive(&exe, &request, utf8, &flag, &mut progress) {
                Ok(summary) => {
                    send(JobMessage::TestStats(summary.stats));
                    Ok(JobOutcome {
                        cancelled: summary.cancelled,
                        warnings: summary.warnings,
                    })
                }
                Err(err) => Err(err.to_string()),
            }
        });

        self.notice = None;
        self.start_job(JobKind::Test, archive, cancel, receiver, cx);
    }

    pub(crate) fn open_create_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.job.is_some() {
            self.notice = Some("Дождитесь окончания текущей операции".to_string());
            return;
        }
        if self.active().is_archive {
            self.notice = Some("Создание архива доступно из каталога".to_string());
            return;
        }
        if self.active().selection.is_empty() {
            self.notice = Some("Выберите файлы или папки для архива".to_string());
            return;
        }

        // Имя по умолчанию — как в 7-Zip: один выделенный объект даёт своё имя
        // (папка — целиком, файл — без расширения), несколько — имя каталога.
        // Иначе архив из выделенной папки назывался бы именем родительской.
        let (base, sources) = {
            let panel = self.active();
            let current_dir = panel
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
            let single = (panel.selection.len() == 1)
                .then(|| {
                    panel
                        .selection
                        .iter()
                        .next()
                        .and_then(|index| panel.entries.get(*index))
                })
                .flatten();

            let base = match single {
                Some(entry) if entry.is_dir => entry.name.clone(),
                Some(entry) => Path::new(&entry.name)
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .filter(|stem| !stem.is_empty())
                    .unwrap_or_else(|| entry.name.clone()),
                None => current_dir
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| "archive".to_string()),
            };

            let sources: Vec<PathBuf> = panel
                .selection
                .iter()
                .filter_map(|index| panel.entries.get(*index))
                .map(|entry| panel.path.join(&entry.name))
                .collect();

            (base, sources)
        };

        let name = cx.new(|cx| InputState::new(window, cx).default_value(format!("{base}.7z")));
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("пароль")
                .masked(true)
        });
        let repeat = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("повторите пароль")
                .masked(true)
        });
        let dictionary =
            cx.new(|cx| InputState::new(window, cx).placeholder("авто (например 64m)"));
        let threads = cx.new(|cx| InputState::new(window, cx).placeholder("все ядра"));
        let volume = cx.new(|cx| InputState::new(window, cx).placeholder("100m"));

        // Enter в любом поле подтверждает создание — как в диалогах 7-Zip.
        for input in [&name, &password, &repeat, &dictionary, &threads, &volume] {
            cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_create(cx);
                    if this.modal.is_none() {
                        window.focus(&this.focus, cx);
                    }
                }
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
        }

        let focus = name.read(cx).focus_handle(cx);
        window.focus(&focus, cx);

        self.ui_log(&format!(
            "add-dialog open selection={} sources={}",
            self.active().selection.len(),
            sources.len()
        ));
        self.notice = None;
        self.modal = Some(Modal::Create(CreateDialog {
            directory: self.active().path.clone(),
            name,
            password,
            repeat,
            error: None,
            show_password: false,
            dictionary,
            threads,
            volume,
            solid: true,
            full_paths: false,
            compress_open: false,
            encrypt_names: false,
            level: Level::Normal,
            method: Method::Lzma2,
            sources,
        }));
    }

    pub(crate) fn submit_create(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::Create(mut dialog)) = self.modal.take() else {
            return;
        };

        let name = dialog.name.read(cx).value().to_string();
        let password = dialog.password.read(cx).value().to_string();
        let repeat = dialog.repeat.read(cx).value().to_string();
        let volume = dialog.volume.read(cx).value().to_string();
        let dictionary = dialog.dictionary.read(cx).value().trim().to_string();
        let threads = dialog.threads.read(cx).value().trim().to_string();

        let name = name.trim().to_string();
        if name.is_empty() {
            dialog.error = Some("Имя архива пустое".to_string());
            self.modal = Some(Modal::Create(dialog));
            return;
        }

        // 7-Zip требует, чтобы повтор совпадал: опечатка в пароле — это
        // недоступный архив, а не мелкое неудобство.
        if password != repeat {
            dialog.error = Some("Пароль и повтор не совпадают".to_string());
            self.modal = Some(Modal::Create(dialog));
            return;
        }

        let threads = if threads.is_empty() {
            None
        } else {
            match threads.parse::<u32>() {
                Ok(value) if value > 0 => Some(value),
                _ => {
                    dialog.error = Some("Число потоков — целое число больше нуля".to_string());
                    self.modal = Some(Modal::Create(dialog));
                    return;
                }
            }
        };

        if dialog.encrypt_names && password.is_empty() {
            dialog.error = Some("Для шифрования имён введите пароль".to_string());
            self.modal = Some(Modal::Create(dialog));
            cx.notify();
            return;
        }

        // Берём снимок, сделанный при открытии диалога, а не текущее выделение.
        let sources = dialog.sources.clone();
        if sources.is_empty() {
            dialog.error = Some("Ничего не выбрано".to_string());
            self.modal = Some(Modal::Create(dialog));
            return;
        }

        let destination = dialog.directory.join(&name);
        self.ui_log(&format!(
            "create archive={} sources=[{}]",
            destination.display(),
            sources
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        let mut request = CreateRequest::new(&destination, sources);
        request.level = dialog.level;
        request.method = dialog.method;
        request.dictionary = (!dictionary.is_empty()).then_some(dictionary);
        request.threads = threads;
        request.solid = dialog.solid;
        request.full_paths = dialog.full_paths;
        request.compress_open = dialog.compress_open;
        request.encrypt_names = dialog.encrypt_names;
        request.password = if password.is_empty() {
            None
        } else {
            Some(password)
        };
        request.volume = if volume.trim().is_empty() {
            None
        } else {
            Some(volume.trim().to_string())
        };
        if let Err(error) = request.validate() {
            dialog.error = Some(error.to_string());
            self.modal = Some(Modal::Create(dialog));
            cx.notify();
            return;
        }

        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let receiver = spawn_job(move |send| {
            let tool = resolve_tool()?;
            let exe = tool.info.exe.clone();
            let utf8 = tool.info.version.supports_utf8_console();
            // Объём считаем здесь же, в фоне: обход большого дерева на UI-потоке
            // заморозил бы окно, а 7z всё равно пройдёт по тем же каталогам.
            let (bytes, files) = scan_totals(&request.sources);
            send(JobMessage::Totals { bytes, files });
            let mut progress = |event: Progress| send(JobMessage::Progress(event));
            match create(&exe, &request, utf8, &flag, &mut progress) {
                Ok(summary) => Ok(JobOutcome {
                    cancelled: summary.cancelled,
                    warnings: summary.warnings,
                }),
                Err(err) => Err(err.to_string()),
            }
        });

        self.start_job(JobKind::Create, destination, cancel, receiver, cx);
        if let Some(job) = &mut self.job {
            job.retry_dialog = Some(Modal::Create(dialog));
        }
        cx.notify();
    }

    /// Enter/F3 на файле внутри архива: извлечение во временную папку и открытие
    /// ассоциированной программой, как в 7-Zip FM. Шифрованный архив сначала
    /// спрашивает пароль — без него 7z полез бы читать его со stdin.
    pub(crate) fn open_from_archive(
        &mut self,
        entry: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.job.is_some() {
            self.notice = Some("Дождитесь окончания текущей операции".to_string());
            return;
        }
        // Имя записи приходит из чужого файла: `../` и ему подобные не должны
        // уводить извлечение за пределы временной папки.
        if !entry_path_is_safe(&entry) {
            self.notice = Some(format!("Небезопасное имя в архиве: «{entry}»"));
            return;
        }
        let archive = self.active().path.clone();
        if self.active().encrypted {
            self.open_password_dialog(archive, entry, window, cx);
        } else {
            self.start_open_extract(archive, entry, None, cx);
        }
    }

    pub(crate) fn open_password_dialog(
        &mut self,
        archive: PathBuf,
        entry: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("пароль")
                .masked(true)
        });

        cx.subscribe(&password, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.submit_open_password(cx);
            }
        })
        .detach();

        let focus = password.read(cx).focus_handle(cx);
        window.focus(&focus, cx);

        self.notice = None;
        self.modal = Some(Modal::Open(OpenDialog {
            archive,
            entry,
            password,
        }));
    }

    pub(crate) fn submit_open_password(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::Open(dialog)) = self.modal.take() else {
            return;
        };
        let password = dialog.password.read(cx).value().to_string();
        self.start_open_extract(
            dialog.archive,
            dialog.entry,
            (!password.is_empty()).then_some(password),
            cx,
        );
    }

    /// Извлекает одну запись архива во временную папку; по готовности
    /// `finish_job` откроет её оболочкой (`Job::open_after`).
    pub(crate) fn start_open_extract(
        &mut self,
        archive: PathBuf,
        entry: String,
        password: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.job.is_some() {
            return;
        }

        let dir = match temp_open_dir() {
            Ok(dir) => dir,
            Err(err) => {
                self.notice = Some(format!(
                    "Не удалось создать временную папку: {}",
                    fs_error(&err)
                ));
                return;
            }
        };
        // Папку убираем при закрытии окна даже если извлечение упадёт.
        self.temp_dirs.push(dir.clone());
        let open_path = dir.join(&entry);

        let (total_bytes, total_files) =
            if self.active().is_archive && self.active().path == archive {
                self.totals_for(std::slice::from_ref(&entry))
            } else {
                (0, 0)
            };

        let mut request = ExtractRequest::new(&archive, &dir);
        request.entries = vec![entry];
        request.password = password;

        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let receiver = spawn_job(move |send| {
            let tool = resolve_tool()?;
            let exe = tool.info.exe.clone();
            let utf8 = tool.info.version.supports_utf8_console();
            send(JobMessage::Totals {
                bytes: total_bytes,
                files: total_files,
            });
            let mut progress = |event: Progress| send(JobMessage::Progress(event));
            match extract(&exe, &request, utf8, &flag, &mut progress) {
                Ok(summary) => Ok(JobOutcome {
                    cancelled: summary.cancelled,
                    warnings: summary.warnings,
                }),
                Err(err) => Err(err.to_string()),
            }
        });

        self.notice = None;
        self.start_job(JobKind::Open, archive, cancel, receiver, cx);
        if let Some(job) = &mut self.job {
            job.open_after = vec![open_path];
        }
    }

    /// «Файл → CRC»: контрольные суммы выделенных файлов (`7z h`) в фоне.
    ///
    /// Внутри архива не работаем: `7z h` считает файлы на диске, для содержимого
    /// архива пришлось бы сначала извлекать — это отдельное решение.
    pub(crate) fn start_hash(&mut self, algorithm: HashAlgorithm, cx: &mut Context<Self>) {
        if self.job.is_some() {
            self.notice = Some("Дождитесь окончания текущей операции".to_string());
            return;
        }
        let paths: Vec<PathBuf> = {
            let panel = self.active();
            if panel.is_archive {
                self.notice = Some("Контрольные суммы доступны для файлов в каталоге".to_string());
                return;
            }
            // Нет выделения — считаем файл под курсором, как 7-Zip FM.
            let indices: Vec<usize> = if panel.selection.is_empty() {
                panel.active.into_iter().collect()
            } else {
                panel.selection.iter().copied().collect()
            };
            indices
                .iter()
                .filter_map(|index| panel.entries.get(*index))
                .map(|entry| panel.path.join(&entry.name))
                .collect()
        };
        if paths.is_empty() {
            self.notice = Some("Выделите файлы для подсчёта сумм".to_string());
            return;
        }

        let request = HashRequest::new(algorithm, paths);
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let receiver = spawn_job(move |send| {
            let tool = resolve_tool()?;
            let exe = tool.info.exe.clone();
            let utf8 = tool.info.version.supports_utf8_console();
            let (bytes, files) = scan_totals(&request.paths);
            send(JobMessage::Totals { bytes, files });
            let mut progress = |event: Progress| send(JobMessage::Progress(event));
            match hash_files(&exe, &request, utf8, &flag, &mut progress) {
                Ok(summary) => {
                    send(JobMessage::Hashes(summary.report));
                    Ok(JobOutcome {
                        cancelled: summary.cancelled,
                        warnings: summary.warnings,
                    })
                }
                Err(err) => Err(err.to_string()),
            }
        });

        self.notice = None;
        let target = self.active().path.clone();
        self.start_job(JobKind::Hash, target, cancel, receiver, cx);
    }
}

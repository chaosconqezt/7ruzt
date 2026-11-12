use crate::prelude::*;

// Tab: переключение панели. Действие своё, потому что встроенное в gpui-base
// `Tab` переводит фокус по элементам, а в 7-Zip FM Tab меняет активную панель.
actions!(archiver, [SwitchPanel]);
actions!(address, [Complete]);

/// Что делает кнопка.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolAction {
    None,
    Add,
    Extract,
    Test,
    Delete,
    Copy,
    Move,
    /// Свойства выделенного объекта — как Alt+Enter.
    Info,
}

/// Открытый модальный диалог.
///
/// Диалоги взаимоисключающие, поэтому одно поле, а не семь `Option`: раньше
/// список «какой диалог открыт» приходилось повторять в `render`, `list` и
/// `handle_key`, и новый диалог легко было забыть в одной из копий.
///
/// Дерево выбора папки (`folder_picker`) сюда не входит: оно открывается
/// **поверх** диалога извлечения или переноса и живёт отдельным слоем.
/// Переименование в строке и окно прогресса — тоже не модалки в этом смысле.
pub(crate) enum Modal {
    Create(CreateDialog),
    Extract(ExtractDialog),
    Test(TestDialog),
    /// Итог проверки: маленькое окно поверх закрывшегося прогресса.
    TestResult(TestResultDialog),
    Open(OpenDialog),
    Hash(HashDialog),
    Prompt(PromptDialog),
    Transfer(TransferDialog),
    /// Тестирование производительности (`7z b`): живой вывод консоли.
    Bench(BenchDialog),
    Notice(String),
    About,
}

/// Как показан список.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewMode {
    LargeIcons,
    SmallIcons,
    List,
    Details,
}

impl ViewMode {
    /// Сетка (несколько колонок) или одна колонка на объект.
    pub(crate) fn is_grid(self) -> bool {
        matches!(self, Self::LargeIcons | Self::SmallIcons)
    }
}

pub(crate) struct MainWindow {
    /// Панели списка. В обычном режиме видна и активна первая; вторая (F9)
    /// создаётся лениво и живёт, даже когда её скрыли, — как в 7-Zip FM.
    pub(crate) panels: [Panel; 2],
    /// Какую панель слушают клавиши и кнопки: 0 — левая, 1 — правая.
    pub(crate) active_panel: usize,
    /// Показана ли вторая панель (F9).
    pub(crate) two_panel: bool,
    /// Ширина окна на текущем кадре: от неё зависит, какие колонки влезают.
    pub(crate) window_width: f32,
    /// Фокус на списке: без него клавиатура не доходит до окна.
    pub(crate) focus: FocusHandle,
    /// Идущая фоновая операция (распаковка).
    pub(crate) job: Option<Job>,
    /// Завершённая проверка: окно прогресса остаётся под окном итогов,
    /// как в 7-Zip, пока его не закроют. Идущей операцией не считается.
    pub(crate) done_job: Option<Job>,
    /// Задача опроса фонового потока: если её выронить, опрос остановится.
    pub(crate) job_task: Option<Task<()>>,
    /// Последнее сообщение для статус-бара: «готово», «отменено», ошибка.
    pub(crate) notice: Option<String>,
    /// Открытый модальный диалог: создание, извлечение, проверка и прочее.
    pub(crate) modal: Option<Modal>,
    pub(crate) address_edit: Option<AddressEdit>,
    /// Идёт переименование прямо в строке списка.
    pub(crate) rename: Option<RenameEdit>,
    /// Панель, в которой переименование только что завершилось по потере
    /// фокуса. Клик, который его вызвал, относится к прежнему списку и не
    /// должен менять выделение — но только в этой панели.
    pub(crate) rename_finished: Option<usize>,
    /// Открыто дерево выбора папки.
    pub(crate) folder_picker: Option<FolderPicker>,
    /// Раскрытое меню и его попап. Попап создаётся заново при каждом открытии:
    /// в нём зафиксировано состояние окна (галочки, доступность пунктов).
    pub(crate) open_menu: Option<TopMenu>,
    pub(crate) menu_popup: Option<Entity<PopupMenu>>,
    /// Подписка на закрытие попапа: без неё `open_menu` остался бы висеть.
    pub(crate) menu_subscription: Option<Subscription>,
    /// Заголовок, который уже стоит в окне — чтобы не звать SetWindowText зря.
    pub(crate) title_cache: String,
    /// Точка отсчёта для меток времени в диагностическом логе.
    pub(crate) clock: Instant,
    /// Когда логировали прошлое нажатие — чтобы видеть паузы между событиями.
    pub(crate) last_key_log: Option<Instant>,
    /// Счётчики кадра и отклика ввода.
    pub(crate) meter: FrameMeter,
    /// Временные папки просмотра файлов из архива: убираются при закрытии окна.
    pub(crate) temp_dirs: Vec<PathBuf>,
    /// Системные значки, общие на обе панели. `Rc`, потому что списки
    /// виртуализированы и строят строки в замыкании без доступа к `self`.
    pub(crate) icons: std::rc::Rc<IconCache>,
    /// Выбранная тема. `System` означает «следить за оформлением ОС».
    pub(crate) theme_pref: ThemePreference,
    /// Выбранный масштаб текста (Малый/Средний/Большой).
    pub(crate) font_scale: FontScale,
    /// Подписка на смену системного оформления. Если её выронить, `System`
    /// перестанет замечать переключение светлой/тёмной темы в ОС.
    pub(crate) theme_subscription: Option<Subscription>,
}

impl MainWindow {
    /// `prefix` — открыть архив сразу на подкаталоге (`docs/img`); для каталога игнорируется.
    pub(crate) fn with_prefix(
        path: PathBuf,
        prefix: Option<String>,
        theme_pref: ThemePreference,
        font_scale: FontScale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Вторая панель появится по F9: её стартовый путь зависит от того,
        // что открыто сейчас, и вычисляется в [`Self::toggle_two_panels`].
        let second = Panel::new(path.clone());

        let mut this = Self {
            panels: [Panel::open(path, prefix), second],
            active_panel: 0,
            two_panel: false,
            window_width: 0.,
            focus: cx.focus_handle(),
            job: None,
            done_job: None,
            job_task: None,
            notice: None,
            modal: None,
            address_edit: None,
            rename: None,
            rename_finished: None,
            folder_picker: None,
            open_menu: None,
            menu_popup: None,
            menu_subscription: None,
            title_cache: String::new(),
            clock: Instant::now(),
            last_key_log: None,
            meter: FrameMeter::new(),
            // Пробники системных значков чистятся вместе с остальным temp.
            temp_dirs: vec![probe_dir()],
            icons: std::rc::Rc::new(IconCache::new()),
            theme_pref,
            font_scale,
            theme_subscription: None,
        };
        // Уже есть окно — уточняем системную тему по `window.appearance()`:
        // на Linux оформление платформы до открытия окна может быть ещё
        // неизвестно, и старт получился бы светлым при тёмной ОС.
        if this.theme_pref == ThemePreference::System {
            theme::apply(ThemePreference::System, Some(&mut *window), cx);
        }
        // «Системная» тема должна замечать переключение оформления в ОС, пока
        // окно открыто. Если человек выбрал явно светлую/тёмную — не трогаем.
        this.theme_subscription = Some(cx.observe_window_appearance(window, |this, window, cx| {
            if this.theme_pref == ThemePreference::System {
                theme::apply(ThemePreference::System, Some(window), cx);
                cx.notify();
            }
        }));
        // Стартовое чтение — в фоне: запуск 7z на большом архиве не должен
        // морозить окно до первого кадра.
        this.reload_panel(0, cx, |_, _| {});
        this
    }

    /// Активная панель: её слушают клавиши, тулбар и меню.
    pub(crate) fn active(&self) -> &Panel {
        &self.panels[self.active_panel]
    }

    pub(crate) fn active_mut(&mut self) -> &mut Panel {
        &mut self.panels[self.active_panel]
    }

    pub(crate) fn panel(&self, index: usize) -> &Panel {
        &self.panels[index]
    }

    pub(crate) fn panel_mut(&mut self, index: usize) -> &mut Panel {
        &mut self.panels[index]
    }

    /// Сколько панелей видно сейчас.
    pub(crate) fn panel_count(&self) -> usize {
        if self.two_panel { 2 } else { 1 }
    }

    /// Сделать панель активной (без перерисовки: это дело вызывающего).
    pub(crate) fn activate_panel(&mut self, index: usize) {
        self.active_panel = if self.two_panel { index.min(1) } else { 0 };
    }

    /// F9: показать/скрыть вторую панель.
    ///
    /// При включении вторая панель впервые открывается рядом с текущей: если
    /// активна папка — в той же папке, если архив — в папке, где он лежит.
    /// Иначе из архива некуда распаковать, а ради этого режим и включают.
    /// При выключении активной остаётся левая панель, как в 7-Zip FM.
    pub(crate) fn toggle_two_panels(&mut self, cx: &mut Context<Self>) {
        if self.two_panel {
            self.two_panel = false;
            self.active_panel = 0;
        } else {
            self.two_panel = true;
            self.active_panel = 0;
            if !self.panels[1].loaded {
                let start = if self.panels[0].is_archive {
                    self.panels[0]
                        .path
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| self.panels[0].path.clone())
                } else {
                    self.panels[0].path.clone()
                };
                self.panels[1] = Panel::open(start, None);
                self.reload_panel(1, cx, |_, _| {});
            }
        }
        cx.notify();
    }

    /// Открыть вторую панель сразу на заданном пути (тестовый `--right=`).
    pub(crate) fn show_second_panel(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.two_panel = true;
        self.active_panel = 0;
        self.panels[1] = Panel::open(path, None);
        self.reload_panel(1, cx, |_, _| {});
        cx.notify();
    }

    /// Alt+Up: открыть текущую папку в другой панели.
    pub(crate) fn mirror_in_other_panel(&mut self, cx: &mut Context<Self>) {
        if !self.two_panel {
            return;
        }
        let other = 1 - self.active_panel;
        let (path, prefix, is_archive) = {
            let panel = self.active();
            (panel.path.clone(), panel.prefix.clone(), panel.is_archive)
        };
        self.panels[other].path = path;
        self.reload_panel(other, cx, move |this, cx| {
            let panel = &mut this.panels[other];
            if is_archive && panel.is_archive {
                panel.prefix = prefix;
                panel.rebuild_view();
            }
            panel.scroll_to_top();
            this.with_window(cx, |this, window, _| this.sync_title(window));
        });
    }

    /// Диагностический лог — только когда задан ARCHIVER_UI_LOG.
    /// Пишется рядом с exe, чтобы человек мог просто прислать файл.
    pub(crate) fn ui_log(&self, line: &str) {
        diag::log(self.clock, line);
    }

    /// Открыт диалог или дерево папок: список не строим, клавиши принадлежат слою.
    pub(crate) fn modal_open(&self) -> bool {
        self.modal.is_some() || self.folder_picker.is_some()
    }

    /// Закрыть открытый диалог. Дерево папок закрывается своим крестиком
    /// или Escape в [`keys`]: оно лежит поверх диалога и живёт отдельно.
    pub(crate) fn close_modal(&mut self) {
        // Бенчмарк держит фоновый 7z: при закрытии гасим процесс, иначе он
        // дожил бы до конца теста вхолостую. Задача опроса дропается с диалогом.
        if let Some(Modal::Bench(dialog)) = &self.modal {
            dialog.cancel.store(true, Ordering::Relaxed);
        }
        self.modal = None;
    }

    /// Enter в диалоге подтверждает то, что в нём открыто.
    pub(crate) fn confirm_modal(&mut self, cx: &mut Context<Self>) {
        match self.modal.as_ref() {
            Some(Modal::Create(_)) => self.submit_create(cx),
            Some(Modal::Extract(_)) => self.submit_extract(cx),
            Some(Modal::Test(_)) => self.submit_test(cx),
            Some(Modal::TestResult(_)) => self.close_modal(),
            Some(Modal::Open(_)) => self.submit_open_password(cx),
            Some(Modal::Hash(_)) => self.close_modal(),
            Some(Modal::Prompt(_)) => self.submit_prompt(cx),
            Some(Modal::Transfer(_)) => self.submit_transfer(cx),
            // У бенчмарка Enter ничего не подтверждает: прогон идёт сам.
            Some(Modal::Bench(_)) => {}
            Some(Modal::About | Modal::Notice(_)) => self.close_modal(),
            None => {}
        }
    }

    /// Машиночитаемый дамп состояния окна для автоматизированного тестирования.
    pub(crate) fn dump_state(&self, cx: &App) -> String {
        let modal = match &self.modal {
            Some(Modal::Create(_)) => "\"create\"",
            Some(Modal::Extract(_)) => "\"extract\"",
            Some(Modal::Test(_)) => "\"test\"",
            Some(Modal::TestResult(_)) => "\"test-result\"",
            Some(Modal::Open(_)) => "\"open\"",
            Some(Modal::Hash(_)) => "\"hash\"",
            Some(Modal::Prompt(_)) => "\"prompt\"",
            Some(Modal::Transfer(_)) => "\"transfer\"",
            Some(Modal::Bench(_)) => "\"bench\"",
            Some(Modal::About) => "\"about\"",
            Some(Modal::Notice(_)) => "\"notice\"",
            None => "null",
        };
        let notice = match &self.notice {
            Some(n) => json_str(n),
            None => "null".to_string(),
        };
        let job = match &self.job {
            Some(j) => format!(
                "{{\"kind\":{},\"percent\":{},\"item\":{},\"total_files\":{},\"seen_files\":{}}}",
                json_str(j.verb()),
                j.percent,
                json_str(&j.item),
                j.total_files,
                j.seen_files
            ),
            None => "null".to_string(),
        };
        let panel0 = self.panels[0].dump_json();
        let panel1 = self.panels[1].dump_json();
        let rename_selection = match &self.rename {
            Some(edit) => {
                let range = edit.input.read(cx).selected_range();
                format!("[{},{}]", range.start, range.end)
            }
            None => "null".to_string(),
        };
        let address_suggestions = self
            .address_edit
            .as_ref()
            .map(|edit| edit.suggestions.len())
            .unwrap_or(0);
        let address_ghost = self
            .address_edit
            .as_ref()
            .and_then(|edit| completion_ghost(&edit.input.read(cx).value(), &edit.suggestions))
            .map(|ghost| json_str(&ghost))
            .unwrap_or_else(|| "null".to_string());
        let address_value = match &self.address_edit {
            Some(edit) => json_str(&edit.input.read(cx).value()),
            None => "null".to_string(),
        };

        format!(
            "{{\"active_panel\":{},\"two_panel\":{},\"theme\":{},\"font\":{},\"modal\":{},\"has_folder_picker\":{},\"has_rename\":{},\"rename_selection\":{},\"address_edit\":{},\"address_suggestions\":{},\"address_ghost\":{},\"address_value\":{},\"notice\":{},\"job\":{},\"done_progress\":{},\"panels\":[{},{}]}}",
            self.active_panel,
            self.two_panel,
            json_str(self.theme_pref.as_str()),
            json_str(self.font_scale.as_str()),
            modal,
            self.folder_picker.is_some(),
            self.rename.is_some(),
            rename_selection,
            self.address_edit.is_some(),
            address_suggestions,
            address_ghost,
            address_value,
            notice,
            job,
            self.done_job.is_some(),
            panel0,
            panel1
        )
    }

    pub(crate) fn write_dump_state(&self, target: &Path, cx: &App) {
        let json = self.dump_state(cx);
        let _ = std::fs::write(target, json);
    }
}

impl Drop for MainWindow {
    fn drop(&mut self) {
        // Внешняя программа могла оставить файл открытым — тогда папка
        // переживёт окно; это лучше, чем ломать просмотрщик удалением.
        for dir in &self.temp_dirs {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

impl Panel {
    pub(crate) fn dump_json(&self) -> String {
        let path = json_str(&self.path.display().to_string());
        let prefix = json_str(&self.prefix);
        let kind = match &self.kind {
            Some(k) => json_str(k),
            None => "null".to_string(),
        };
        let error = match &self.error {
            Some(e) => json_str(e),
            None => "null".to_string(),
        };
        let sel: Vec<String> = self.selection.iter().map(|i| i.to_string()).collect();
        let active = match self.active {
            Some(a) => a.to_string(),
            None => "null".to_string(),
        };
        let first_names: Vec<String> = self
            .entries
            .iter()
            .take(10)
            .map(|e| json_str(&e.name))
            .collect();

        format!(
            "{{\"path\":{},\"prefix\":{},\"is_archive\":{},\"kind\":{},\"encrypted\":{},\"entries_count\":{},\"selection\":[{}],\"active\":{},\"first_entries\":[{}],\"error\":{},\"loading\":{}}}",
            path,
            prefix,
            self.is_archive,
            kind,
            self.encrypted,
            self.entries.len(),
            sel.join(","),
            active,
            first_names.join(","),
            error,
            self.loading
        )
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

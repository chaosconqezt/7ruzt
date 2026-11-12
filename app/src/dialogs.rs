use crate::prelude::*;
/// Что делает однострочный диалог: имя новой папки/файла или маска выделения.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PromptAction {
    CreateFolder,
    CreateFile,
    SelectMask,
    DeselectMask,
}

impl PromptAction {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::CreateFolder => "Новая папка",
            Self::CreateFile => "Новый файл",
            Self::SelectMask => "Выделить",
            Self::DeselectMask => "Убрать выделение",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::CreateFolder => "Имя папки:",
            Self::CreateFile => "Имя файла:",
            Self::SelectMask | Self::DeselectMask => "Маска:",
        }
    }

    pub(crate) fn button(self) -> &'static str {
        match self {
            Self::CreateFolder | Self::CreateFile => "Создать",
            Self::SelectMask => "Выделить",
            Self::DeselectMask => "Убрать",
        }
    }
}

/// Однострочный диалог с полем ввода — «Создать папку/файл» и маска выделения.
pub(crate) struct PromptDialog {
    pub(crate) action: PromptAction,
    pub(crate) input: Entity<InputState>,
    /// Ошибка показывается в самом диалоге, а не только в статус-баре:
    /// «имя занято» внизу окна легко не заметить.
    pub(crate) error: Option<String>,
}

/// Переименование прямо в строке списка (F2, как в проводнике).
///
/// Отдельного окна нет: строка на месте меняет подпись на поле ввода.
pub(crate) struct RenameEdit {
    /// В какой панели правим имя: вторая панель — отдельный список.
    pub(crate) panel: usize,
    /// Индекс строки в списке — привязка к видимому уровню, не к имени.
    pub(crate) index: usize,
    /// Имя до правки: по нему ищем объект на диске.
    pub(crate) original: String,
    pub(crate) input: Entity<InputState>,
}

/// Диалог «Копировать в…» / «Переместить в…».
pub(crate) struct TransferDialog {
    pub(crate) kind: TransferKind,
    /// Снимок объектов на момент открытия — выделение за диалогом не меняется.
    pub(crate) sources: Vec<PathBuf>,
    pub(crate) destination: Entity<InputState>,
    pub(crate) overwrite: Overwrite,
    pub(crate) error: Option<String>,
}

/// Состояние диалога создания архива.
///
/// Поля ввода — штатные `InputState` из gpui-kit. Самописное поле на `on_key_down`
/// было ошибкой: оно обходило нормальный путь ввода и давало задержку в кадр.
pub(crate) struct CreateDialog {
    pub(crate) directory: PathBuf,
    pub(crate) name: Entity<InputState>,
    pub(crate) password: Entity<InputState>,
    /// Ошибка в самом диалоге: имя пустое/занято, пароли не совпали и т. п.
    pub(crate) error: Option<String>,
    /// Повтор пароля: 7-Zip не даёт создать архив, если строки расходятся.
    pub(crate) repeat: Entity<InputState>,
    /// Показывать пароль открытым текстом (переключает маску у обоих полей).
    pub(crate) show_password: bool,
    /// Размер словаря (`-md`): пусто — как решит 7z.
    pub(crate) dictionary: Entity<InputState>,
    /// Число потоков (`-mmt`): пусто — все ядра.
    pub(crate) threads: Entity<InputState>,
    pub(crate) volume: Entity<InputState>,
    pub(crate) solid: bool,
    /// `-spf`: хранить полные пути.
    pub(crate) full_paths: bool,
    /// `-ssw`: сжимать файлы, открытые на запись.
    pub(crate) compress_open: bool,
    pub(crate) encrypt_names: bool,
    pub(crate) level: Level,
    pub(crate) method: Method,
    /// Что именно архивируем — зафиксировано в момент открытия диалога.
    /// Иначе любое изменение выделения (стрелка, клик мимо) тихо меняет содержимое архива.
    pub(crate) sources: Vec<PathBuf>,
}

/// Список вариантов в выпадающем списке диалога извлечения.
pub(crate) type ExtractSelect = Entity<SelectState<SearchableVec<&'static str>>>;

/// Диалог «Извлечь»: как в 7-Zip FM — куда, подпапка с именем архива,
/// структура путей, перезапись и пароль.
pub(crate) struct ExtractDialog {
    pub(crate) error: Option<String>,
    /// Архив, который распакуем: открытый или выделенный в списке.
    pub(crate) archive: PathBuf,
    /// Пусто — весь архив; иначе только эти элементы внутри него.
    pub(crate) entries: Vec<String>,
    pub(crate) destination: Entity<InputState>,
    pub(crate) password: Entity<InputState>,
    pub(crate) overwrite: Overwrite,
    pub(crate) path_mode: PathMode,
    /// Отдельная подпапка с именем архива, как галочка «rox\» в 7-Zip.
    pub(crate) subfolder: bool,
    pub(crate) subfolder_name: String,
    /// Варианты «Полные пути / Без путей» и режим перезаписи.
    pub(crate) paths_select: ExtractSelect,
    pub(crate) overwrite_select: ExtractSelect,
    pub(crate) show_password: bool,
}

/// Диалог проверки архива: только пароль. Обычный архив проверяется сразу,
/// диалог нужен лишь когда содержимое зашифровано (`Listing::encrypted`).
pub(crate) struct TestDialog {
    /// Что проверяем — открытый архив или файл, выделенный в списке.
    pub(crate) archive: PathBuf,
    pub(crate) password: Entity<InputState>,
}

/// Итог проверки как в 7-Zip: счётчики и «Ошибок не найдено» либо текст ошибки.
pub(crate) struct TestResultDialog {
    pub(crate) stats: TestStats,
    /// Замечания 7z (код 1): проверка прошла, но не для всего.
    pub(crate) warnings: String,
    /// Проверка не пройдена или 7z не запустился.
    pub(crate) error: Option<String>,
}

/// Диалог пароля при открытии файла из шифрованного архива (Enter/F3).
pub(crate) struct OpenDialog {
    pub(crate) archive: PathBuf,
    /// Путь записи внутри архива — что извлечь во временную папку и открыть.
    pub(crate) entry: String,
    pub(crate) password: Entity<InputState>,
}

/// Отчёт контрольных сумм: список файлов сверху, суммы выбранного снизу.
pub(crate) struct HashDialog {
    pub(crate) report: HashReport,
    /// Индекс файла в `report.files`; `files.len()` — строка «Все данные».
    pub(crate) selected: usize,
    /// Прокрутка виртуализованного списка файлов.
    pub(crate) scroll: UniformListScrollHandle,
}

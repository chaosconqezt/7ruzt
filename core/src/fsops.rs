//! Файловые операции над каталогом: создать, переименовать, удалить.
//!
//! Операции без 7z, включая копирование и перемещение с прогрессом,
//! безопасной заменой существующих объектов и отменой.
//!
//! Всё, что можно проверить без окна, живёт здесь: правила имён Windows и
//! поведение на коллизиях — источник самых неприятных ошибок в файловом
//! менеджере (переименование «в себя», имена `CON`, точка в конце).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::run::Progress;

/// Почему имя не годится для файла или папки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameError {
    Empty,
    /// `.` и `..` — не имя, а переход по дереву.
    Dots,
    /// Символ, запрещённый в Windows: `< > : " / \ | ? *` и управляющие.
    Forbidden(char),
    /// Пробел или точка в конце: Windows молча отрежет, и имя разойдётся с введённым.
    Trailing,
    /// Зарезервированное имя Windows (`CON`, `PRN`, `COM1`…) — даже с расширением.
    Reserved,
}

impl std::fmt::Display for NameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "имя пустое"),
            Self::Dots => write!(f, "«.» и «..» — не имя"),
            Self::Forbidden(ch) => write!(f, "символ «{ch}» нельзя использовать в имени"),
            Self::Trailing => write!(f, "имя не может заканчиваться пробелом или точкой"),
            Self::Reserved => write!(f, "это имя зарезервировано Windows"),
        }
    }
}

impl std::error::Error for NameError {}

/// Зарезервированные имена Windows: файл `CON.txt` тоже нельзя создать.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Проверяет имя, которое ввёл человек.
///
/// Проверяем строже, чем разрешает POSIX: проект целиком про Windows-поведение
/// (7-Zip FM), а имена с `:` или хвостовой точкой на Windows создают файл,
/// который потом нельзя ни открыть, ни удалить проводником.
pub fn check_name(name: &str) -> Result<(), NameError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name == "." || name == ".." {
        return Err(NameError::Dots);
    }
    if let Some(ch) = name
        .chars()
        .find(|ch| matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || *ch < ' ')
    {
        return Err(NameError::Forbidden(ch));
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return Err(NameError::Trailing);
    }

    // До первой точки: `CON.txt` — тоже зарезервированное имя.
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return Err(NameError::Reserved);
    }
    Ok(())
}

/// Создаёт папку `parent/name`.
///
/// `create_dir` (не `create_dir_all`): вложенные пути запрещены проверкой имени,
/// а лишняя «умность» здесь только маскировала бы опечатку.
pub fn create_folder(parent: &Path, name: &str) -> io::Result<PathBuf> {
    let path = parent.join(name.trim());
    std::fs::create_dir(&path)?;
    Ok(path)
}

/// Создаёт пустой файл `parent/name`.
///
/// `create_new`, а не `create`: существующий файл не обнуляем молча.
pub fn create_empty_file(parent: &Path, name: &str) -> io::Result<PathBuf> {
    let path = parent.join(name.trim());
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    Ok(path)
}

/// Переименовывает `parent/from` в `parent/to`.
///
/// Отдельно обрабатывает смену регистра (`photo.jpg` → `Photo.jpg`): на Windows
/// проверка `exists()` считает такой путь существующим — файл-то тот же, — и
/// наивная проверка коллизии запрещала бы смену регистра. На unix файловая
/// система регистрозависимая: `File.txt` и `file.txt` — разные объекты, и
/// поблажка привела бы к молчаливой перезаписи чужого файла.
pub fn rename_in(parent: &Path, from: &str, to: &str) -> io::Result<PathBuf> {
    let from = from.trim();
    let to = to.trim();
    if from == to {
        return Ok(parent.join(from));
    }

    let target = parent.join(to);
    let same_name = cfg!(windows) && from.to_lowercase() == to.to_lowercase();
    if !same_name && target.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("«{to}» уже существует"),
        ));
    }

    std::fs::rename(parent.join(from), &target)?;
    Ok(target)
}

/// Итог удаления: сколько получилось и на чём споткнулись.
#[derive(Debug, Default)]
pub struct DeleteSummary {
    pub deleted: usize,
    pub errors: Vec<(PathBuf, io::Error)>,
}

/// Удаляет пути безвозвратно (корзина — отдельная, платформенная операция).
///
/// Каталог удаляется целиком; ссылка — только сама, без захода в цель, иначе
/// «удалить ярлык» стирало бы содержимое другого каталога.
pub fn delete_paths(paths: &[PathBuf]) -> DeleteSummary {
    let mut summary = DeleteSummary::default();
    for path in paths {
        match remove_one(path) {
            Ok(()) => summary.deleted += 1,
            Err(err) => summary.errors.push((path.clone(), err)),
        }
    }
    summary
}

fn remove_one(path: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        // В Windows ссылка на каталог (даже битая) удаляется через remove_dir;
        // в Unix любая ссылка — через remove_file. Цель никогда не трогаем.
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileTypeExt as _;
            if meta.file_type().is_symlink_dir() {
                return std::fs::remove_dir(path);
            }
        }
        return std::fs::remove_file(path);
    }
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Копирует один объект ровно в `destination` (как `cp src dst`):
/// существующий объект заменяется/пропускается/переименовывается по политике.
///
/// Возвращает итоговый путь; `None` — пропуск, в том числе копирование в себя.
pub fn copy_to(
    source: &Path,
    destination: &Path,
    policy: ConflictPolicy,
    cancelled: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
) -> io::Result<Option<PathBuf>> {
    transfer_to(source, destination, policy, cancelled, on_progress, false)
}

/// Как [`copy_to`], но источник удаляется после успешной записи.
pub fn move_to(
    source: &Path,
    destination: &Path,
    policy: ConflictPolicy,
    cancelled: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
) -> io::Result<Option<PathBuf>> {
    transfer_to(source, destination, policy, cancelled, on_progress, true)
}

fn transfer_to(
    source: &Path,
    destination: &Path,
    policy: ConflictPolicy,
    cancelled: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
    remove_after: bool,
) -> io::Result<Option<PathBuf>> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(interrupted());
    }

    let Some(target) = resolve_target(source, destination, policy)? else {
        return Ok(None);
    };
    let total = path_size(source);
    let mut done = 0;
    let written = copy_entry(
        source,
        &target,
        policy,
        cancelled,
        &mut done,
        total,
        on_progress,
    )?;

    if remove_after
        && let Some(target) = &written
        && let Err(err) = remove_one(source)
    {
        // Копия уже записана: ошибка удаления не должна выглядеть как
        // провал всего переноса — иначе операцию повторят и задвоят файл.
        return Err(io::Error::new(
            err.kind(),
            format!(
                "копия записана в «{}», но источник не удалён: {err}",
                target.display()
            ),
        ));
    }
    Ok(written)
}

/// Что делать, если в каталоге назначения уже есть объект с таким именем.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    Replace,
    Skip,
    Rename,
}

/// Итог копирования/перемещения.
#[derive(Debug, Default)]
pub struct TransferSummary {
    /// Сколько корневых объектов скопировано/перенесено.
    pub copied: usize,
    /// Сколько пропущено из-за политики «не перезаписывать».
    pub skipped: usize,
    pub bytes: u64,
    pub errors: Vec<(PathBuf, io::Error)>,
    pub cancelled: bool,
}

/// Копирует источники в каталог `target_dir`.
///
/// Прогресс — теми же событиями, что у 7z: `Item` на корневой объект и
/// `Percent` по скопированным байтам, поэтому окно операции не различает
/// «копирую» и «распаковываю».
pub fn copy_paths(
    sources: &[PathBuf],
    target_dir: &Path,
    policy: ConflictPolicy,
    cancelled: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
) -> TransferSummary {
    transfer(sources, target_dir, policy, cancelled, on_progress, false)
}

/// Перемещает источники: копирует и удаляет исходник только после успеха.
pub fn move_paths(
    sources: &[PathBuf],
    target_dir: &Path,
    policy: ConflictPolicy,
    cancelled: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
) -> TransferSummary {
    transfer(sources, target_dir, policy, cancelled, on_progress, true)
}

fn transfer(
    sources: &[PathBuf],
    target_dir: &Path,
    policy: ConflictPolicy,
    cancelled: &AtomicBool,
    on_progress: &mut dyn FnMut(Progress),
    remove_after: bool,
) -> TransferSummary {
    let mut summary = TransferSummary::default();
    let total = sources.iter().map(|path| path_size(path)).sum::<u64>();
    let mut done = 0u64;

    for source in sources {
        if cancelled.load(Ordering::Relaxed) {
            summary.cancelled = true;
            break;
        }
        let destination = match resolve_destination(source, target_dir, policy) {
            Ok(Some(destination)) => destination,
            Ok(None) => {
                summary.skipped += 1;
                continue;
            }
            Err(err) => {
                summary.errors.push((source.clone(), err));
                continue;
            }
        };

        match copy_entry(
            source,
            &destination,
            policy,
            cancelled,
            &mut done,
            total,
            on_progress,
        ) {
            Ok(Some(_)) => {
                summary.copied += 1;
                summary.bytes = done;
                if remove_after && let Err(err) = remove_one(source) {
                    summary.errors.push((source.clone(), err));
                }
            }
            Ok(None) => summary.skipped += 1,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                summary.cancelled = true;
                break;
            }
            Err(err) => summary.errors.push((source.clone(), err)),
        }
    }

    summary
}

/// Куда копировать корневой объект. `None` — политика «пропустить».
fn resolve_destination(
    source: &Path,
    target_dir: &Path,
    policy: ConflictPolicy,
) -> io::Result<Option<PathBuf>> {
    let Some(name) = source.file_name() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "у источника нет имени",
        ));
    };

    resolve_target(source, &target_dir.join(name), policy)
}

fn parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

fn entry_exists(path: &Path) -> io::Result<bool> {
    // В отличие от exists(), учитывает и битые ссылки, не скрывает отказ доступа.
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

fn canonical_target(path: &Path) -> io::Result<PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let name = path.file_name().ok_or(err)?;
            // Проверяем также ещё не созданных потомков и ссылки в родителях.
            Ok(canonical_target(parent_dir(path))?.join(name))
        }
        Err(err) => Err(err),
    }
}

fn resolve_target(
    source: &Path,
    destination: &Path,
    policy: ConflictPolicy,
) -> io::Result<Option<PathBuf>> {
    let source_path = std::fs::canonicalize(source)?;
    let target = if entry_exists(destination)? {
        match policy {
            ConflictPolicy::Skip => return Ok(None),
            ConflictPolicy::Replace => destination.to_path_buf(),
            ConflictPolicy::Rename => {
                let name = destination.file_name().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "у цели нет имени")
                })?;
                unique_name(parent_dir(destination), name)
            }
        }
    } else {
        destination.to_path_buf()
    };
    // canonicalize раскрывает ссылки и возвращает реальный регистр имён Windows.
    // Rename проверяем уже с отдельным именем: дублирование в той же папке законно.
    let target_path = canonical_target(&target)?;
    if source_path == target_path {
        return Ok(None);
    }
    if (source.is_dir() && target_path.starts_with(&source_path))
        || source_path.starts_with(&target_path)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "нельзя копировать папку внутрь себя или заменять родителя источника",
        ));
    }
    Ok(Some(target))
}

/// Имя, которого ещё нет: `a.txt` → `a (2).txt`, `a (2).txt` → `a (3).txt`.
fn unique_name(dir: &Path, name: &std::ffi::OsStr) -> PathBuf {
    let name = name.to_string_lossy();
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem.to_string(), format!(".{extension}")),
        _ => (name.to_string(), String::new()),
    };

    for index in 2..1000 {
        let candidate = dir.join(format!("{stem} ({index}){extension}"));
        if candidate
            .symlink_metadata()
            .is_err_and(|err| err.kind() == io::ErrorKind::NotFound)
        {
            return candidate;
        }
    }
    // Практически недостижимо: 999 вариантов заняты — добавим счётчик счётчика.
    dir.join(format!("{stem} (999+){extension}"))
}

/// Временный сосед на том же томе. Внутри — новая копия и, только на время
/// подмены, прежний объект. Drop убирает недописанное при любой ошибке/отмене.
struct Replacement {
    directory: PathBuf,
    keep_backup: bool,
}

impl Replacement {
    fn new(destination: &Path) -> io::Result<Self> {
        use std::sync::atomic::AtomicU64;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let seq = NEXT.fetch_add(1, Ordering::Relaxed);
            let directory = parent_dir(destination)
                .join(format!(".archiver-transfer-{}-{seq}", std::process::id()));
            if directory == destination {
                continue;
            }
            // Резервируем имя атомарно, чужие временные файлы не перезаписываем.
            match std::fs::create_dir(&directory) {
                Ok(()) => {
                    return Ok(Self {
                        directory,
                        keep_backup: false,
                    });
                }
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(err),
            }
        }
    }

    fn entry(&self) -> PathBuf {
        self.directory.join("entry")
    }

    fn install(
        &mut self,
        destination: &Path,
        policy: ConflictPolicy,
        cancelled: &AtomicBool,
    ) -> io::Result<()> {
        if policy != ConflictPolicy::Replace {
            if entry_exists(destination)? {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "имя назначения уже занято",
                ));
            }
            let staged = self.entry();
            if staged.is_dir() {
                return std::fs::rename(staged, destination);
            }
            match std::fs::hard_link(&staged, destination) {
                Ok(()) => return Ok(()),
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => return Err(err),
                Err(_) => {}
            }
            // На FAT/exFAT нет hard link; create_new всё равно не затирает чужой файл.
            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)?;
            let result = (|| {
                use std::io::{Read as _, Write as _};
                let mut input = std::fs::File::open(staged)?;
                let mut buffer = vec![0u8; 256 * 1024];
                loop {
                    if cancelled.load(Ordering::Relaxed) {
                        return Err(interrupted());
                    }
                    let read = input.read(&mut buffer)?;
                    if read == 0 {
                        break;
                    }
                    output.write_all(&buffer[..read])?;
                }
                output.sync_all()
            })();
            drop(output);
            if result.is_err() {
                let _ = std::fs::remove_file(destination);
            }
            return result;
        }
        let backup = self.directory.join("previous");
        let replacing = entry_exists(destination)?;
        if replacing {
            // Работает и для непустых каталогов, и для конфликтов файл ↔ каталог.
            std::fs::rename(destination, &backup)?;
        }
        if let Err(err) = std::fs::rename(self.entry(), destination) {
            if replacing && let Err(rollback) = std::fs::rename(&backup, destination) {
                // Не стираем единственную старую копию, если ОС отказала в откате.
                self.keep_backup = true;
                return Err(io::Error::new(
                    err.kind(),
                    format!(
                        "{err}; не удалось восстановить «{}»: {rollback}; прежний объект: «{}»",
                        destination.display(),
                        backup.display()
                    ),
                ));
            }
            return Err(err);
        }
        if replacing && let Err(err) = remove_one(&backup) {
            self.keep_backup = true;
            return Err(io::Error::new(
                err.kind(),
                format!(
                    "копия записана, но не удалось удалить резерв «{}»: {err}",
                    backup.display()
                ),
            ));
        }
        Ok(())
    }
}

impl Drop for Replacement {
    fn drop(&mut self) {
        if !self.keep_backup {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }
}

fn copy_entry(
    source: &Path,
    destination: &Path,
    policy: ConflictPolicy,
    cancelled: &AtomicBool,
    done: &mut u64,
    total: u64,
    on_progress: &mut dyn FnMut(Progress),
) -> io::Result<Option<PathBuf>> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(interrupted());
    }
    // Вызывается только после resolve_target, до этого ничего не менялось.
    std::fs::create_dir_all(parent_dir(destination))?;
    let mut replacement = Replacement::new(destination)?;
    let before = *done;
    let result = (|| {
        // Готовим весь корневой объект: Replace не превращается в слияние папок,
        // а Skip не оставляет нескопированных детей, которые Move затем удалит.
        copy_tree(
            source,
            &replacement.entry(),
            cancelled,
            done,
            total,
            on_progress,
        )?;
        if cancelled.load(Ordering::Relaxed) {
            return Err(interrupted());
        }
        // Конфликт мог появиться за время копирования, в том числе из callback.
        let Some(target) = resolve_target(source, destination, policy)? else {
            return Ok(None);
        };
        match replacement.install(&target, policy, cancelled) {
            Ok(()) => Ok(Some(target)),
            Err(err)
                if policy == ConflictPolicy::Skip && err.kind() == io::ErrorKind::AlreadyExists =>
            {
                Ok(None)
            }
            Err(err) => Err(err),
        }
    })();
    if !matches!(&result, Ok(Some(_))) {
        *done = before;
    }
    result
}

/// Копирует только в зарезервированное пустое место; политики применяются
/// к корню снаружи, поэтому успешный результат означает полную копию дерева.
fn copy_tree(
    source: &Path,
    destination: &Path,
    cancelled: &AtomicBool,
    done: &mut u64,
    total: u64,
    on_progress: &mut dyn FnMut(Progress),
) -> io::Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(interrupted());
    }
    let meta = std::fs::symlink_metadata(source)?;
    if meta.file_type().is_symlink() || (!meta.is_dir() && !meta.is_file()) {
        // Не разыменовываем ссылки и не читаем устройства/FIFO как обычные файлы.
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "ссылки и специальные файлы пока не копируются",
        ));
    }
    if meta.is_dir() {
        std::fs::create_dir(destination)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            copy_tree(
                &entry.path(),
                &destination.join(entry.file_name()),
                cancelled,
                done,
                total,
                on_progress,
            )?;
        }
        // Время каталога ставим после детей: их создание обновляет mtime.
        // На Windows каталог нельзя открыть как файл без unsafe/WinAPI,
        // поэтому там время каталогов не переносится.
        #[cfg(unix)]
        {
            let mut times = std::fs::FileTimes::new();
            if let Ok(modified) = meta.modified() {
                times = times.set_modified(modified);
            }
            if let Ok(accessed) = meta.accessed() {
                times = times.set_accessed(accessed);
            }
            if let Ok(dir) = std::fs::File::open(destination) {
                let _ = dir.set_times(times);
            }
            let _ = std::fs::set_permissions(destination, meta.permissions());
        }
        return Ok(());
    }

    // Каталоги не считаем файлами в прогрессе.
    on_progress(Progress::Item(source.display().to_string()));
    copy_file(source, destination, cancelled, done, total, on_progress)
}

/// destination находится внутри Replacement, никогда не указывает на старый файл.
fn copy_file(
    source: &Path,
    destination: &Path,
    cancelled: &AtomicBool,
    done: &mut u64,
    total: u64,
    on_progress: &mut dyn FnMut(Progress),
) -> io::Result<()> {
    use std::io::{Read as _, Write as _};

    if cancelled.load(Ordering::Relaxed) {
        return Err(interrupted());
    }
    let mut input = std::fs::File::open(source)?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut buffer = vec![0u8; 256 * 1024];

    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err(interrupted());
        }
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        *done += read as u64;
        if let Some(percent) = (*done * 100).checked_div(total) {
            on_progress(Progress::Percent(percent.min(100) as u8));
        }
    }
    output.flush()?;
    // В том числе выявляем отложенные ошибки записи до замены старого объекта.
    output.sync_all()?;
    preserve_metadata(source, &output);
    drop(output);
    drop(input);
    Ok(())
}

/// Время и права оригинала — на копию. Best-effort: FAT не умеет часть
/// атрибутов, и отказ их выставить не должен обесценивать успешную запись.
fn preserve_metadata(source: &Path, output: &std::fs::File) {
    let Ok(meta) = std::fs::metadata(source) else {
        return;
    };
    let mut times = std::fs::FileTimes::new();
    if let Ok(modified) = meta.modified() {
        times = times.set_modified(modified);
    }
    if let Ok(accessed) = meta.accessed() {
        times = times.set_accessed(accessed);
    }
    // Время создания std умеет ставить только на Windows; на unix из-за него
    // отказал бы весь вызов — не рискуем остальными атрибутами.
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTimesExt as _;
        if let Ok(created) = meta.created() {
            times = times.set_created(created);
        }
    }
    let _ = output.set_times(times);
    #[cfg(unix)]
    let _ = output.set_permissions(meta.permissions());
}

fn path_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            total += path_size(&entry.path());
        }
    }
    total
}

fn interrupted() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "отменено")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "archiver-fs-{tag}-{}-{nanos}-{seq}",
            std::process::id()
        ))
    }

    fn write_test_entry(path: &Path, directory: bool, contents: &[u8]) {
        let file = if directory {
            path.join("nested/data")
        } else {
            path.to_path_buf()
        };
        std::fs::create_dir_all(parent_dir(&file)).unwrap();
        std::fs::write(file, contents).unwrap();
    }

    fn read_test_entry(path: &Path, directory: bool) -> Vec<u8> {
        std::fs::read(if directory {
            path.join("nested/data")
        } else {
            path.to_path_buf()
        })
        .unwrap()
    }

    fn assert_no_replacements(parent: &Path) {
        for entry in std::fs::read_dir(parent).unwrap() {
            assert!(
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".archiver-transfer-"),
                "остался временный объект в {}",
                parent.display()
            );
        }
    }

    fn assert_self_transfer_skipped(source: &Path, destination: &Path) {
        for transfer in [copy_to, move_to] {
            for policy in [ConflictPolicy::Replace, ConflictPolicy::Skip] {
                let written = transfer(
                    source,
                    destination,
                    policy,
                    &AtomicBool::new(false),
                    &mut |_| panic!("копирование в себя не должно начинаться"),
                )
                .unwrap();
                assert!(written.is_none());
            }
        }
    }

    #[test]
    fn rejects_names_windows_cannot_use() {
        for name in [
            "", "   ", ".", "..", "a/b", "a\\b", "c:name", "what?", "a*b",
        ] {
            assert!(
                check_name(name).is_err(),
                "имя {name:?} должно быть отклонено"
            );
        }
        // Хвостовую точку Windows отрежет молча — её отвергаем; хвостовой
        // пробел срезаем сами (UI тоже передаёт уже обрезанное имя).
        assert_eq!(check_name("name."), Err(NameError::Trailing));
        assert!(check_name("name ").is_ok());
        // Зарезервированные имена — и с расширением тоже.
        assert_eq!(check_name("CON"), Err(NameError::Reserved));
        assert_eq!(check_name("con.txt"), Err(NameError::Reserved));
        assert_eq!(check_name("COM7"), Err(NameError::Reserved));
    }

    #[test]
    fn accepts_ordinary_names() {
        for name in ["docs", "a.txt", "Фото 2026", ".gitignore", "x.7z.001"] {
            assert!(check_name(name).is_ok(), "имя {name:?} должно проходить");
        }
    }

    #[test]
    fn creates_renames_and_deletes() {
        let dir = unique_temp_dir("roundtrip");
        std::fs::create_dir_all(&dir).unwrap();

        let folder = create_folder(&dir, "docs").unwrap();
        assert!(folder.is_dir());

        let file = create_empty_file(&dir, "a.txt").unwrap();
        assert!(file.is_file());

        // Повторное создание — ошибка, а не тихое обнуление.
        assert!(create_empty_file(&dir, "a.txt").is_err());
        assert!(create_folder(&dir, "docs").is_err());

        let renamed = rename_in(&dir, "a.txt", "b.txt").unwrap();
        assert!(renamed.is_file());
        assert!(!dir.join("a.txt").exists());

        // Коллизия: имя уже занято другим объектом.
        assert!(rename_in(&dir, "b.txt", "docs").is_err());

        let removed = delete_paths(&[dir.join("b.txt"), dir.join("docs")]);
        assert_eq!(removed.deleted, 2);
        assert!(removed.errors.is_empty());
        assert!(!dir.join("b.txt").exists());
        assert!(!dir.join("docs").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn case_only_rename_is_not_a_collision() {
        let dir = unique_temp_dir("case");
        std::fs::create_dir_all(&dir).unwrap();
        create_empty_file(&dir, "photo.jpg").unwrap();

        // На Windows `exists()` для «Photo.jpg» истинно (файл тот же) —
        // проверка коллизии не должна запрещать смену регистра.
        let renamed = rename_in(&dir, "photo.jpg", "Photo.jpg").unwrap();
        assert!(renamed.exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rename_to_same_name_is_noop() {
        let dir = unique_temp_dir("noop");
        std::fs::create_dir_all(&dir).unwrap();
        create_empty_file(&dir, "a.txt").unwrap();

        assert!(rename_in(&dir, "a.txt", "a.txt").is_ok());
        assert!(dir.join("a.txt").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn case_only_rename_collides_on_case_sensitive_fs() {
        let dir = unique_temp_dir("case-collide");
        std::fs::create_dir_all(&dir).unwrap();
        create_empty_file(&dir, "File.txt").unwrap();
        create_empty_file(&dir, "file.txt").unwrap();

        // На unix это два разных объекта: перезапись запрещена.
        assert!(rename_in(&dir, "File.txt", "file.txt").is_err());
        assert!(dir.join("File.txt").exists());
        assert!(dir.join("file.txt").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn copy_preserves_modified_time() {
        let dir = unique_temp_dir("mtime");
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("a.txt");
        std::fs::write(&source, b"hello").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(86_400 * 30);
        let file = std::fs::File::options().write(true).open(&source).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(old))
            .unwrap();
        drop(file);

        let target = dir.join("b.txt");
        copy_to(
            &source,
            &target,
            ConflictPolicy::Replace,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();

        assert_eq!(
            std::fs::metadata(&source).unwrap().modified().unwrap(),
            std::fs::metadata(&target).unwrap().modified().unwrap(),
            "время модификации должно перейти на копию"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_removes_nested_folder_entirely() {
        let dir = unique_temp_dir("nested");
        std::fs::create_dir_all(dir.join("outer/inner")).unwrap();
        std::fs::write(dir.join("outer/inner/a.txt"), b"x").unwrap();

        let summary = delete_paths(&[dir.join("outer")]);
        assert_eq!(summary.deleted, 1);
        assert!(!dir.join("outer").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn copies_file_and_folder_tree() {
        let dir = unique_temp_dir("copy");
        let source = dir.join("src");
        let target = dir.join("dst");
        std::fs::create_dir_all(source.join("sub")).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(source.join("a.txt"), b"hello").unwrap();
        std::fs::write(source.join("sub/b.txt"), b"world").unwrap();

        let cancelled = AtomicBool::new(false);
        let mut progress = |_: Progress| {};
        let summary = copy_paths(
            &[source.join("a.txt"), source.join("sub")],
            &target,
            ConflictPolicy::Replace,
            &cancelled,
            &mut progress,
        );

        assert_eq!(summary.copied, 2);
        assert!(summary.errors.is_empty(), "{:?}", summary.errors);
        assert_eq!(std::fs::read(target.join("a.txt")).unwrap(), b"hello");
        assert_eq!(std::fs::read(target.join("sub/b.txt")).unwrap(), b"world");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn conflict_policies_skip_replace_and_rename() {
        let dir = unique_temp_dir("conflict");
        let source = dir.join("src");
        let target = dir.join("dst");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(source.join("a.txt"), b"new").unwrap();
        std::fs::write(target.join("a.txt"), b"old").unwrap();

        let cancelled = AtomicBool::new(false);
        let mut progress = |_: Progress| {};
        let file = source.join("a.txt");

        let skipped = copy_paths(
            std::slice::from_ref(&file),
            &target,
            ConflictPolicy::Skip,
            &cancelled,
            &mut progress,
        );
        assert_eq!(skipped.skipped, 1);
        assert_eq!(std::fs::read(target.join("a.txt")).unwrap(), b"old");

        let replaced = copy_paths(
            std::slice::from_ref(&file),
            &target,
            ConflictPolicy::Replace,
            &cancelled,
            &mut progress,
        );
        assert_eq!(replaced.copied, 1);
        assert_eq!(std::fs::read(target.join("a.txt")).unwrap(), b"new");

        let renamed = copy_paths(
            &[file],
            &target,
            ConflictPolicy::Rename,
            &cancelled,
            &mut progress,
        );
        assert_eq!(renamed.copied, 1);
        assert_eq!(std::fs::read(target.join("a (2).txt")).unwrap(), b"new");
        // Старая копия не тронута.
        assert_eq!(std::fs::read(target.join("a.txt")).unwrap(), b"new");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn move_removes_source_after_copy() {
        let dir = unique_temp_dir("move");
        let source = dir.join("src");
        let target = dir.join("dst");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(source.join("a.txt"), b"x").unwrap();

        let cancelled = AtomicBool::new(false);
        let mut progress = |_: Progress| {};
        let summary = move_paths(
            &[source.join("a.txt")],
            &target,
            ConflictPolicy::Replace,
            &cancelled,
            &mut progress,
        );

        assert_eq!(summary.copied, 1);
        assert!(!source.join("a.txt").exists(), "источник должен исчезнуть");
        assert!(target.join("a.txt").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn copying_folder_into_itself_is_refused() {
        let dir = unique_temp_dir("selfcopy");
        std::fs::create_dir_all(dir.join("sub")).unwrap();

        let cancelled = AtomicBool::new(false);
        let mut progress = |_: Progress| {};
        let summary = copy_paths(
            &[dir.join("sub")],
            &dir.join("sub"),
            ConflictPolicy::Replace,
            &cancelled,
            &mut progress,
        );

        assert_eq!(summary.copied, 0);
        assert_eq!(summary.errors.len(), 1);
        assert_eq!(summary.errors[0].1.kind(), std::io::ErrorKind::InvalidInput);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn copy_to_creates_destination_with_new_name() {
        let dir = unique_temp_dir("copytopath");
        let source = dir.join("src");
        std::fs::create_dir_all(source.join("sub")).unwrap();
        std::fs::write(source.join("sub/a.txt"), b"x").unwrap();

        let cancelled = AtomicBool::new(false);
        let mut progress = |_: Progress| {};
        let destination = dir.join("renamed");
        let written = copy_to(
            &source,
            &destination,
            ConflictPolicy::Replace,
            &cancelled,
            &mut progress,
        )
        .unwrap()
        .expect("скопировано");

        assert_eq!(written, destination);
        assert!(destination.join("sub/a.txt").is_file());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn copy_to_file_path_copies_as_new_file() {
        let dir = unique_temp_dir("copyfilepath");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"hello").unwrap();

        let cancelled = AtomicBool::new(false);
        let mut progress = |_: Progress| {};
        let destination = dir.join("b.txt");
        copy_to(
            &dir.join("a.txt"),
            &destination,
            ConflictPolicy::Replace,
            &cancelled,
            &mut progress,
        )
        .unwrap();

        assert_eq!(std::fs::read(&destination).unwrap(), b"hello");
        assert!(dir.join("a.txt").exists(), "источник при копировании цел");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn move_to_removes_source_and_honours_skip() {
        let dir = unique_temp_dir("movetopath");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"x").unwrap();
        std::fs::write(dir.join("b.txt"), b"old").unwrap();

        let cancelled = AtomicBool::new(false);
        let mut progress = |_: Progress| {};

        // Пропуск: цель уже есть — ничего не делаем, источник остаётся.
        let skipped = move_to(
            &dir.join("a.txt"),
            &dir.join("b.txt"),
            ConflictPolicy::Skip,
            &cancelled,
            &mut progress,
        )
        .unwrap();
        assert!(skipped.is_none());
        assert!(dir.join("a.txt").exists());

        // Перемещение в новое имя: источник исчезает, цель появляется.
        move_to(
            &dir.join("a.txt"),
            &dir.join("c.txt"),
            ConflictPolicy::Replace,
            &cancelled,
            &mut progress,
        )
        .unwrap();
        assert!(!dir.join("a.txt").exists());
        assert!(dir.join("c.txt").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn copy_and_move_to_self_preserve_files_and_directories() {
        for directory in [false, true] {
            let dir = unique_temp_dir("self-to");
            let source = dir.join("item");
            write_test_entry(&source, directory, b"original");
            assert_self_transfer_skipped(&source, &source);
            assert_self_transfer_skipped(&source, &dir.join("./item"));
            assert_eq!(read_test_entry(&source, directory), b"original");
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[cfg(windows)]
    #[test]
    fn copy_and_move_to_case_alias_preserve_original() {
        let dir = unique_temp_dir("self-case");
        let source = dir.join("Photo.txt");
        write_test_entry(&source, false, b"original");
        let alias = dir.join("PHOTO.TXT");
        // Windows допускает opt-in case-sensitive каталог: там это другое имя.
        if alias.exists() {
            assert_self_transfer_skipped(&source, &alias);
        }
        assert_eq!(std::fs::read(&source).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn copy_and_move_to_symlink_alias_preserve_original() {
        for directory in [false, true] {
            let dir = unique_temp_dir("self-link");
            let source = dir.join("item");
            let alias = dir.join("alias");
            write_test_entry(&source, directory, b"original");
            std::os::unix::fs::symlink(&source, &alias).unwrap();
            assert_self_transfer_skipped(&source, &alias);
            assert_eq!(read_test_entry(&source, directory), b"original");
            assert!(std::fs::symlink_metadata(&alias).unwrap().is_symlink());
            assert_no_replacements(&dir);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn rename_allows_copy_and_move_to_self_under_separate_name() {
        for (moving, transfer) in [copy_to, move_to].into_iter().enumerate() {
            for directory in [false, true] {
                let dir = unique_temp_dir("self-rename-to");
                let source = dir.join("item");
                write_test_entry(&source, directory, b"original");
                let written = transfer(
                    &source,
                    &source,
                    ConflictPolicy::Rename,
                    &AtomicBool::new(false),
                    &mut |_| {},
                )
                .unwrap()
                .unwrap();
                assert_eq!(written, dir.join("item (2)"));
                assert_eq!(read_test_entry(&written, directory), b"original");
                assert_eq!(source.exists(), moving == 0);
                assert_no_replacements(&dir);
                std::fs::remove_dir_all(dir).unwrap();
            }
        }
    }

    #[test]
    fn paths_in_same_directory_skip_self_but_allow_rename() {
        for (moving, transfer) in [copy_paths, move_paths].into_iter().enumerate() {
            for directory in [false, true] {
                for policy in [
                    ConflictPolicy::Replace,
                    ConflictPolicy::Skip,
                    ConflictPolicy::Rename,
                ] {
                    let dir = unique_temp_dir("self-paths");
                    let source = dir.join("item");
                    write_test_entry(&source, directory, b"original");
                    let summary = transfer(
                        std::slice::from_ref(&source),
                        &dir,
                        policy,
                        &AtomicBool::new(false),
                        &mut |_| {},
                    );
                    assert!(summary.errors.is_empty(), "{:?}", summary.errors);
                    let renamed = policy == ConflictPolicy::Rename;
                    assert_eq!(summary.copied, usize::from(renamed));
                    assert_eq!(summary.skipped, usize::from(!renamed));
                    assert_eq!(source.exists(), moving == 0 || !renamed);
                    let written = if renamed {
                        dir.join("item (2)")
                    } else {
                        source
                    };
                    assert_eq!(read_test_entry(&written, directory), b"original");
                    assert_no_replacements(&dir);
                    std::fs::remove_dir_all(dir).unwrap();
                }
            }
        }
    }

    #[test]
    fn rejects_missing_descendants_and_replacing_source_ancestor_before_changes() {
        let dir = unique_temp_dir("overlap");
        let source = dir.join("source");
        write_test_entry(&source, true, b"original");
        for transfer in [copy_to, move_to] {
            for destination in [source.join("missing/child"), dir.clone()] {
                let err = transfer(
                    &source,
                    &destination,
                    ConflictPolicy::Replace,
                    &AtomicBool::new(false),
                    &mut |_| panic!("опасный перенос начался"),
                )
                .unwrap_err();
                assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
                assert_eq!(read_test_entry(&source, true), b"original");
                assert!(!source.join("missing").exists());
                assert_no_replacements(&dir);
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn replacing_entries_handles_type_conflicts_without_merging_directories() {
        for (moving, transfer) in [copy_to, move_to].into_iter().enumerate() {
            for source_dir in [false, true] {
                for target_dir in [false, true] {
                    let dir = unique_temp_dir("replace-types");
                    let source = dir.join("source");
                    let target = dir.join("target");
                    write_test_entry(&source, source_dir, b"new");
                    write_test_entry(&target, target_dir, b"old");
                    if target_dir {
                        std::fs::write(target.join("old-only"), b"obsolete").unwrap();
                    }
                    let written = transfer(
                        &source,
                        &target,
                        ConflictPolicy::Replace,
                        &AtomicBool::new(false),
                        &mut |event| {
                            if matches!(event, Progress::Percent(_)) {
                                assert_eq!(read_test_entry(&target, target_dir), b"old");
                            }
                        },
                    )
                    .unwrap();
                    assert_eq!(written, Some(target.clone()));
                    assert_eq!(read_test_entry(&target, source_dir), b"new");
                    assert!(!target.join("old-only").exists());
                    assert_eq!(source.exists(), moving == 0);
                    assert_no_replacements(&dir);
                    std::fs::remove_dir_all(dir).unwrap();
                }
            }
        }
    }

    #[test]
    fn skip_and_rename_preserve_existing_entries_across_type_conflicts() {
        for (moving, transfer) in [copy_to, move_to].into_iter().enumerate() {
            for source_dir in [false, true] {
                for target_dir in [false, true] {
                    for policy in [ConflictPolicy::Skip, ConflictPolicy::Rename] {
                        let dir = unique_temp_dir("conflict-types");
                        let source = dir.join("source");
                        let target = dir.join("target");
                        write_test_entry(&source, source_dir, b"new");
                        write_test_entry(&target, target_dir, b"old");
                        let written = transfer(
                            &source,
                            &target,
                            policy,
                            &AtomicBool::new(false),
                            &mut |_| {},
                        )
                        .unwrap();
                        assert_eq!(read_test_entry(&target, target_dir), b"old");
                        if policy == ConflictPolicy::Skip {
                            assert!(written.is_none());
                        } else {
                            assert_eq!(written, Some(dir.join("target (2)")));
                            assert_eq!(read_test_entry(&written.unwrap(), source_dir), b"new");
                        }
                        assert_eq!(
                            source.exists(),
                            moving == 0 || policy == ConflictPolicy::Skip
                        );
                        if source.exists() {
                            assert_eq!(read_test_entry(&source, source_dir), b"new");
                        }
                        assert_no_replacements(&dir);
                        std::fs::remove_dir_all(dir).unwrap();
                    }
                }
            }
        }
    }

    #[test]
    fn missing_source_does_not_remove_existing_target() {
        for transfer in [copy_to, move_to] {
            for directory in [false, true] {
                let dir = unique_temp_dir("missing-source");
                let target = dir.join("target");
                write_test_entry(&target, directory, b"old");
                assert!(
                    transfer(
                        &dir.join("missing"),
                        &target,
                        ConflictPolicy::Replace,
                        &AtomicBool::new(false),
                        &mut |_| panic!("источника нет"),
                    )
                    .is_err()
                );
                assert_eq!(read_test_entry(&target, directory), b"old");
                assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
                std::fs::remove_dir_all(dir).unwrap();
            }
        }
    }

    #[test]
    fn cancellation_does_not_publish_new_entries_or_leave_temporary_files() {
        for transfer in [copy_to, move_to] {
            for directory in [false, true] {
                for empty_file in [false, true] {
                    let dir = unique_temp_dir("cancel-new");
                    let source = dir.join("source");
                    let target = dir.join("target");
                    let contents: &[u8] = if empty_file { b"" } else { b"new" };
                    write_test_entry(&source, directory, contents);
                    let cancelled = AtomicBool::new(false);
                    let err = transfer(
                        &source,
                        &target,
                        ConflictPolicy::Replace,
                        &cancelled,
                        &mut |event| {
                            if matches!(event, Progress::Percent(_))
                                || (empty_file && matches!(event, Progress::Item(_)))
                            {
                                cancelled.store(true, Ordering::Relaxed);
                            }
                        },
                    )
                    .unwrap_err();
                    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
                    assert_eq!(read_test_entry(&source, directory), contents);
                    assert!(!target.exists());
                    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
                    assert_no_replacements(&dir);
                    std::fs::remove_dir_all(dir).unwrap();
                }
            }
        }
    }

    #[test]
    fn cancelled_replacements_preserve_both_versions_and_clean_staging() {
        let contents = vec![b'n'; 256 * 1024 + 1];
        for transfer in [copy_to, move_to] {
            for source_dir in [false, true] {
                for target_dir in [false, true] {
                    for cancel_at_end in [false, true] {
                        let dir = unique_temp_dir("cancel-replace");
                        let source = dir.join("source");
                        let target = dir.join("target");
                        write_test_entry(&source, source_dir, &contents);
                        write_test_entry(&target, target_dir, b"old");
                        let cancelled = AtomicBool::new(false);
                        let err = transfer(
                            &source,
                            &target,
                            ConflictPolicy::Replace,
                            &cancelled,
                            &mut |event| {
                                if let Progress::Percent(percent) = event {
                                    assert_eq!(read_test_entry(&target, target_dir), b"old");
                                    if !cancel_at_end || percent == 100 {
                                        cancelled.store(true, Ordering::Relaxed);
                                    }
                                }
                            },
                        )
                        .unwrap_err();
                        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
                        assert_eq!(read_test_entry(&source, source_dir), contents);
                        assert_eq!(read_test_entry(&target, target_dir), b"old");
                        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
                        assert_no_replacements(&dir);
                        std::fs::remove_dir_all(dir).unwrap();
                    }
                }
            }
        }
    }

    #[test]
    fn paths_replace_entire_tree_and_preserve_it_on_cancellation() {
        for (moving, transfer) in [copy_paths, move_paths].into_iter().enumerate() {
            for cancel in [false, true] {
                let dir = unique_temp_dir("paths-replace");
                let source = dir.join("src/item");
                let target = dir.join("dst/item");
                write_test_entry(&source, true, b"new");
                write_test_entry(&target, true, b"old");
                std::fs::write(target.join("old-only"), b"obsolete").unwrap();
                let cancelled = AtomicBool::new(false);
                let summary = transfer(
                    std::slice::from_ref(&source),
                    parent_dir(&target),
                    ConflictPolicy::Replace,
                    &cancelled,
                    &mut |event| {
                        if matches!(event, Progress::Percent(_)) {
                            assert_eq!(read_test_entry(&target, true), b"old");
                            cancelled.store(cancel, Ordering::Relaxed);
                        }
                    },
                );
                assert!(summary.errors.is_empty(), "{:?}", summary.errors);
                assert_eq!(summary.cancelled, cancel);
                assert_eq!(summary.copied, usize::from(!cancel));
                assert_eq!(summary.bytes, if cancel { 0 } else { 3 });
                assert_eq!(source.exists(), moving == 0 || cancel);
                assert_eq!(
                    read_test_entry(&target, true),
                    if cancel { b"old" } else { b"new" }
                );
                assert_eq!(target.join("old-only").exists(), cancel);
                assert_no_replacements(parent_dir(&target));
                std::fs::remove_dir_all(dir).unwrap();
            }
        }
    }

    #[test]
    fn move_paths_skip_preserves_entire_source_even_for_late_conflicts() {
        for late_conflict in [false, true] {
            let dir = unique_temp_dir("move-skip-tree");
            let source = dir.join("src/item");
            let target = dir.join("dst/item");
            write_test_entry(&source, true, b"new");
            std::fs::write(source.join("source-only"), b"keep").unwrap();
            std::fs::create_dir_all(parent_dir(&target)).unwrap();
            if !late_conflict {
                write_test_entry(&target, true, b"old");
            }
            let summary = move_paths(
                std::slice::from_ref(&source),
                parent_dir(&target),
                ConflictPolicy::Skip,
                &AtomicBool::new(false),
                &mut |event| {
                    if late_conflict && matches!(event, Progress::Percent(_)) {
                        write_test_entry(&target, true, b"old");
                    }
                },
            );
            assert!(summary.errors.is_empty(), "{:?}", summary.errors);
            assert_eq!(summary.copied, 0);
            assert_eq!(summary.skipped, 1);
            assert_eq!(summary.bytes, 0);
            assert_eq!(read_test_entry(&source, true), b"new");
            assert_eq!(std::fs::read(source.join("source-only")).unwrap(), b"keep");
            assert_eq!(read_test_entry(&target, true), b"old");
            assert!(!target.join("source-only").exists());
            assert_no_replacements(parent_dir(&target));
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn late_conflicts_honour_skip_and_rename_for_move_to() {
        for policy in [ConflictPolicy::Skip, ConflictPolicy::Rename] {
            let dir = unique_temp_dir("late-conflict");
            let source = dir.join("source");
            let target = dir.join("target");
            write_test_entry(&source, false, b"new");
            let written = move_to(
                &source,
                &target,
                policy,
                &AtomicBool::new(false),
                &mut |event| {
                    if matches!(event, Progress::Percent(_)) {
                        std::fs::write(&target, b"old").unwrap();
                    }
                },
            )
            .unwrap();
            assert_eq!(std::fs::read(&target).unwrap(), b"old");
            if policy == ConflictPolicy::Skip {
                assert!(written.is_none());
                assert_eq!(std::fs::read(&source).unwrap(), b"new");
            } else {
                assert_eq!(written, Some(dir.join("target (2)")));
                assert_eq!(std::fs::read(written.unwrap()).unwrap(), b"new");
                assert!(!source.exists());
            }
            assert_no_replacements(&dir);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn output_creation_failure_preserves_source_and_target_and_cleans_staging() {
        let dir = unique_temp_dir("write-error");
        let source = dir.join("source");
        let target = dir.join("target");
        write_test_entry(&source, false, b"new");
        write_test_entry(&target, false, b"old");
        // Детерминированный отказ открытия вывода, без chmod (не работает под root).
        let err = move_to(
            &source,
            &target,
            ConflictPolicy::Replace,
            &AtomicBool::new(false),
            &mut |event| {
                if matches!(event, Progress::Item(_)) {
                    for entry in std::fs::read_dir(&dir).unwrap() {
                        let entry = entry.unwrap();
                        if entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".archiver-transfer-")
                        {
                            std::fs::create_dir(entry.path().join("entry")).unwrap();
                        }
                    }
                }
            },
        )
        .unwrap_err();
        assert_ne!(err.kind(), io::ErrorKind::Interrupted);
        assert_eq!(std::fs::read(&source).unwrap(), b"new");
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert_no_replacements(&dir);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn install_without_replace_never_removes_a_late_conflict() {
        for policy in [ConflictPolicy::Skip, ConflictPolicy::Rename] {
            for directory in [false, true] {
                let dir = unique_temp_dir("install-conflict");
                std::fs::create_dir_all(&dir).unwrap();
                let target = dir.join("target");
                {
                    let mut replacement = Replacement::new(&target).unwrap();
                    write_test_entry(&replacement.entry(), directory, b"new");
                    write_test_entry(&target, directory, b"late arrival");
                    let error = replacement
                        .install(&target, policy, &AtomicBool::new(false))
                        .unwrap_err();
                    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
                    assert_eq!(read_test_entry(&target, directory), b"late arrival");
                }
                assert_no_replacements(&dir);
                std::fs::remove_dir_all(dir).unwrap();
            }
        }
    }

    #[test]
    fn failed_install_restores_previous_entry_and_cleans_backup() {
        for directory in [false, true] {
            let dir = unique_temp_dir("rollback");
            let target = dir.join("target");
            write_test_entry(&target, directory, b"old");
            {
                let mut replacement = Replacement::new(&target).unwrap();
                // Нет подготовленного entry: rename старого объекта успешен,
                // установка нового гарантированно падает и должна откатиться.
                assert!(
                    replacement
                        .install(&target, ConflictPolicy::Replace, &AtomicBool::new(false))
                        .is_err()
                );
                assert_eq!(read_test_entry(&target, directory), b"old");
            }
            assert_no_replacements(&dir);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn unsupported_tree_entry_preserves_both_trees_and_cleans_staging() {
        let dir = unique_temp_dir("tree-error");
        let source = dir.join("source");
        let target = dir.join("target");
        write_test_entry(&source, true, b"new");
        write_test_entry(&target, true, b"old");
        std::os::unix::fs::symlink(source.join("nested/data"), source.join("link")).unwrap();
        let err = move_to(
            &source,
            &target,
            ConflictPolicy::Replace,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
        assert_eq!(read_test_entry(&source, true), b"new");
        assert_eq!(read_test_entry(&target, true), b"old");
        assert!(
            std::fs::symlink_metadata(source.join("link"))
                .unwrap()
                .is_symlink()
        );
        assert_no_replacements(&dir);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn replacing_symlink_target_does_not_modify_its_referent() {
        for directory in [false, true] {
            let dir = unique_temp_dir("replace-link");
            let source = dir.join("source");
            let referent = dir.join("referent");
            let target = dir.join("target");
            write_test_entry(&source, directory, b"new");
            write_test_entry(&referent, directory, b"old");
            std::os::unix::fs::symlink(&referent, &target).unwrap();
            copy_to(
                &source,
                &target,
                ConflictPolicy::Replace,
                &AtomicBool::new(false),
                &mut |_| {},
            )
            .unwrap();
            assert_eq!(read_test_entry(&referent, directory), b"old");
            assert_eq!(read_test_entry(&target, directory), b"new");
            assert!(!std::fs::symlink_metadata(&target).unwrap().is_symlink());
            assert_no_replacements(&dir);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn cancel_before_start_does_nothing() {
        let dir = unique_temp_dir("cancelcopy");
        let source = dir.join("src");
        let target = dir.join("dst");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(source.join("a.txt"), b"x").unwrap();

        let cancelled = AtomicBool::new(true);
        let mut progress = |_: Progress| {};
        let summary = copy_paths(
            &[source.join("a.txt")],
            &target,
            ConflictPolicy::Replace,
            &cancelled,
            &mut progress,
        );

        assert!(summary.cancelled);
        assert_eq!(summary.copied, 0);
        assert!(!target.join("a.txt").exists());

        std::fs::remove_dir_all(&dir).ok();
    }
}

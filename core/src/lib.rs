//! Ядро архиватора. Ничего не знает про UI и про GPUI.
//!
//! * [`locate`] — где взять 7-Zip и какой у него версия;
//! * [`list`] — разбор `7z l -slt`;
//! * [`dir`] — обычный каталог файловой системы;
//! * [`archive`] — сворачивание путей архива в «вид из корня»;
//! * [`entry`] — общая модель строки для всех источников;
//! * [`mask`] — выделение по маске (`*`, `?`, несколько масок через пробел);
//! * [`run`] — общий запуск 7z с прогрессом и отменой;
//! * [`extract`], [`create`], [`test`] — операции поверх него;
//! * [`hash`] — контрольные суммы (`7z h`).

pub mod archive;
pub mod bench;
pub mod create;
pub mod dir;
pub mod entry;
pub mod extract;
pub mod format;
pub mod fsops;
pub mod hash;
pub mod list;
pub mod locate;
pub mod mask;
pub mod run;
pub mod test;
pub mod tool;
pub mod version;

pub use archive::{child_prefix, parent_prefix, root_view, view_at};
pub use bench::{
    BenchFreq, BenchOutcome, BenchPair, BenchReport, BenchRow, BenchValue,
    build_args as build_bench_args, parse_output as parse_bench_output, run_benchmark,
};
pub use create::{
    CreateError, CreateRequest, CreateSummary, Level, Method, build_args as build_create_args,
    create,
};
pub use dir::list_dir;
pub use entry::{
    Entry, SortKey, file_extension, format_time, group_digits, human_size, sort_entries,
};
pub use extract::{
    ExtractError, ExtractRequest, ExtractSummary, Overwrite, PathMode,
    build_args as build_extract_args, extract,
};
pub use format::is_archive_name;
pub use fsops::{
    ConflictPolicy, DeleteSummary, NameError, TransferSummary, check_name, copy_paths, copy_to,
    create_empty_file, create_folder, delete_paths, move_paths, move_to, rename_in,
};
pub use hash::{
    HashAlgorithm, HashError, HashReport, HashRequest, HashSummary, HashedFile,
    build_args as build_hash_args, hash_files, parse_report as parse_hash_report,
};
pub use list::{ListError, Listing, list_archive, parse_slt};
pub use locate::{
    LocateError, Locator, ResolvedTool, Warning, binary_name, binary_names, outdated_warnings,
};
pub use mask::mask_matches;
pub use run::{Progress, RunError, RunOutcome, item_name, run_7z, run_7z_captured};
pub use test::{
    TestError, TestRequest, TestStats, TestSummary, build_args as build_test_args, parse_stats,
    test_archive,
};
pub use tool::{ProbeError, ToolInfo, probe};
pub use version::{Arch, Banner, Version, parse_banner};

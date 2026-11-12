//! Общие имена для всех модулей приложения.
//!
//! Модулей много и они плоские, поэтому вместо десятка `use crate::job::…`
//! в каждом файле — один `use crate::prelude::*;`. Это внутренний prelude:
//! наружу (в другие крейты) ничего не торчит, в нём только то, что нужно
//! самому приложению.
//!
//! `allow(unused_imports)`: prelude импортирует всё сразу, и часть имён
//! конкретному модулю не нужна — это нормально для prelude, а не ошибка.
#![allow(unused_imports)]

pub(crate) use crate::*;

pub(crate) use crate::archive::*;
pub(crate) use crate::bench::*;
pub(crate) use crate::diag::*;
pub(crate) use crate::dialogs::*;
pub(crate) use crate::files::*;
pub(crate) use crate::fsutil::*;
pub(crate) use crate::icons::*;
pub(crate) use crate::job::*;
pub(crate) use crate::menu::*;
pub(crate) use crate::nav::*;
pub(crate) use crate::panel::*;
pub(crate) use crate::picker::*;
pub(crate) use crate::select::*;
pub(crate) use crate::shell::*;
pub(crate) use crate::text::*;
pub(crate) use crate::transfer::*;
pub(crate) use crate::ui_about::*;
pub(crate) use crate::ui_chrome::*;
pub(crate) use crate::ui_dialog_bench::*;
pub(crate) use crate::ui_dialog_create::*;
pub(crate) use crate::ui_dialog_picker::*;
pub(crate) use crate::ui_dialog_progress::*;
pub(crate) use crate::ui_dialog_simple::*;
pub(crate) use crate::ui_list::*;
pub(crate) use crate::ui_menu::*;
pub(crate) use crate::ui_widgets::*;
pub(crate) use crate::window::*;

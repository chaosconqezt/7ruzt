//! Диагностика: лог событий интерфейса.
//!
//! Живёт отдельно от логики: это инструмент, а не поведение приложения.
//! Включается переменной `ARCHIVER_UI_LOG=1`, файл пишется рядом с exe.

use crate::prelude::*;

/// Счётчики кадра и отклика ввода.
///
/// Считаются сглаженными: одиночный всплеск не должен выглядеть как «200 мс
/// на кадр». Живут отдельно от логики окна — их видно только в статус-баре.
pub(crate) struct FrameMeter {
    pub(crate) last_render: Instant,
    pub(crate) frame_ms: f32,
    pub(crate) render_ms: f32,
    /// Когда пришло последнее нажатие клавиши — от него считается отклик.
    pub(crate) key_at: Option<Instant>,
    pub(crate) key_ms: f32,
    /// Непрерывный замер (`ARCHIVER_FPS=1`): просим кадр каждый кадр.
    pub(crate) fps_spin: bool,
}

impl FrameMeter {
    pub(crate) fn new() -> Self {
        Self {
            last_render: Instant::now(),
            frame_ms: 0.0,
            render_ms: 0.0,
            key_at: None,
            key_ms: 0.0,
            fps_spin: std::env::var_os("ARCHIVER_FPS").is_some_and(|value| value == "1"),
        }
    }

    /// Начало кадра: сглаживает интервал между кадрами.
    ///
    /// В обычном режиме кадры рисуются только по изменению состояния, и всплески
    /// «два кадра за 2 мс» дают фантастические 400 fps — такие интервалы не
    /// считаем. Возвращает момент старта и «просить ли следующий кадр».
    pub(crate) fn begin(&mut self) -> (Instant, bool) {
        let now = Instant::now();
        let delta = now.duration_since(self.last_render).as_secs_f32();
        if delta > 0.004 && delta < 1.0 {
            // Сглаживаем ИНТЕРВАЛ, а fps считаем из него: если сглаживать обе
            // величины по отдельности, они расходятся (36 fps и 237 мс сразу).
            let millis = delta * 1000.0;
            self.frame_ms = if self.frame_ms <= 0.0 {
                millis
            } else {
                self.frame_ms * 0.85 + millis * 0.15
            };
        }
        self.last_render = now;
        (now, self.fps_spin)
    }

    /// Конец кадра: стоимость сборки дерева и отклик ввода.
    pub(crate) fn finish(&mut self, started: Instant) {
        if let Some(pressed) = self.key_at.take() {
            let echo = pressed.elapsed().as_secs_f32() * 1000.0;
            self.key_ms = if self.key_ms <= 0.0 {
                echo
            } else {
                self.key_ms * 0.7 + echo * 0.3
            };
        }

        let spent = started.elapsed().as_secs_f32() * 1000.0;
        self.render_ms = if self.render_ms <= 0.0 {
            spent
        } else {
            self.render_ms * 0.85 + spent * 0.15
        };
    }

    /// Текст для правого края статус-бара.
    pub(crate) fn status_text(&self) -> String {
        if self.fps_spin && self.frame_ms > 0.0 {
            // Непрерывный замер: настоящая частота конвейера и стоимость кадра.
            format!(
                "{:.0} fps · {:.1} мс · кадр {:.1} мс",
                1000.0 / self.frame_ms,
                self.frame_ms,
                self.render_ms
            )
        } else if self.key_ms > 0.0 {
            // Отклик ввода важнее «fps»: именно он ощущается как тормоз.
            format!("кадр {:.1} мс · ввод {:.0} мс", self.render_ms, self.key_ms)
        } else {
            format!("кадр {:.1} мс", self.render_ms)
        }
    }
}

use std::io::Write;
use std::sync::{Mutex, OnceLock};

static LOG_FILE: OnceLock<Mutex<Option<std::io::BufWriter<std::fs::File>>>> = OnceLock::new();

/// Пишет строку в `ui.log` рядом с exe, если задан `ARCHIVER_UI_LOG=1`.
///
/// Метка времени — от старта окна, чтобы по логу было видно паузы между
/// событиями, не сверяясь с системными часами.
pub(crate) fn log(clock: Instant, line: &str) {
    let mutex = LOG_FILE.get_or_init(|| {
        if !std::env::var_os("ARCHIVER_UI_LOG").is_some_and(|value| value == "1") {
            return Mutex::new(None);
        }
        let path = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join("ui.log")))
            .unwrap_or_else(|| PathBuf::from("ui.log"));
        let writer = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(std::io::BufWriter::new);
        Mutex::new(writer)
    });

    if let Ok(mut guard) = mutex.lock()
        && let Some(writer) = guard.as_mut()
    {
        let _ = writeln!(writer, "t={:>6} {line}", clock.elapsed().as_millis());
        let _ = writer.flush();
    }
}

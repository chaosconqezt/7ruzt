use crate::prelude::*;
/// «1 объект / 2 объекта / 5 объектов» — мелочь, но «1 объектов» выглядит как баг.
pub(crate) fn plural_objects(count: usize) -> &'static str {
    let tail = count % 100;
    if (11..=14).contains(&tail) {
        return "объектов";
    }
    match count % 10 {
        1 => "объект",
        2..=4 => "объекта",
        _ => "объектов",
    }
}

/// Байты в человеческий вид: `1.9 MB`.
pub(crate) fn human(bytes: u64) -> String {
    const KB: f64 = 1024.;
    const MB: f64 = KB * 1024.;
    const GB: f64 = MB * 1024.;

    let value = bytes as f64;
    if value >= GB {
        format!("{:.1} GB", value / GB)
    } else if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        format!("{:.1} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Время как в 7-Zip: `00:07` до часа, дальше `1:02:03`.
pub(crate) fn format_clock(duration: Duration) -> String {
    let total = duration.as_secs();
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

use crate::prelude::*;
/// Как открыть объект системной оболочкой.
#[derive(Clone, Copy)]
pub(crate) enum ShellVerb {
    Open,
    Edit,
    Properties,
}

/// Открывает путь системной оболочкой.
///
/// `windows`-крейт не тянем (`unsafe` запрещён правилами), поэтому через
/// PowerShell: `Start-Process` — это ShellExecute, свойства — через
/// `Shell.Application`. Действие редкое и по нажатию, лишние 200–300 мс на
/// запуск PowerShell не заметны.
pub(crate) fn shell_open(path: &Path, verb: ShellVerb) -> std::io::Result<std::process::Child> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let quoted = path.display().to_string().replace('\'', "''");
        let script = match verb {
            ShellVerb::Open => format!("Start-Process -FilePath '{quoted}'"),
            // Если у типа файла нет глагола Edit, Windows открывает программу
            // по умолчанию — ровно как проводник (F3 и F4 тогда совпадают).
            ShellVerb::Edit => format!(
                "try {{ Start-Process -FilePath '{quoted}' -Verb Edit -ErrorAction Stop }} \
                 catch {{ Start-Process -FilePath '{quoted}' }}"
            ),
            // Свойства: глагол `properties` через Shell.Application на части
            // сборок Windows не показывает диалог. Рабочий путь — ShellExecuteEx
            // с SEE_MASK_INVOKEIDLIST; собираем вызов в PowerShell (без unsafe).
            ShellVerb::Properties => PROPERTIES_SCRIPT.replace("__PATH__", &quoted),
        };

        let mut command = std::process::Command::new("powershell");
        // `-EncodedCommand` вместо `-Command`: скрипт с кавычками и here-string
        // переживает передачу аргументом без экранирования, которое PowerShell
        // и Windows понимают по-разному.
        let encoded = powershell_encoded(&script);
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-EncodedCommand",
            &encoded,
        ]);
        command.creation_flags(CREATE_NO_WINDOW);
        command.spawn()
    }
    #[cfg(not(windows))]
    {
        if !matches!(verb, ShellVerb::Open) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "на этой платформе доступно только открытие",
            ));
        }
        let program = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(program).arg(path).spawn()
    }
}

/// Кодирует PowerShell-скрипт для `-EncodedCommand`: UTF-16LE и Base64.
#[cfg(windows)]
pub(crate) fn powershell_encoded(script: &str) -> String {
    let bytes: Vec<u8> = script
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    base64(&bytes)
}

#[cfg(windows)]
pub(crate) fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((triple >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((triple >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(triple & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// PowerShell-скрипт для системного диалога свойств.
///
/// `ShellExecuteEx` с `SEE_MASK_INVOKEIDLIST` (0x0C) — способ показать штатные
/// свойства файла без `unsafe` и без `windows`-крейта: `Add-Type` компилирует
/// крошечную обёртку в памяти. Глагол `properties` через `Shell.Application`
/// на части сборок Windows молчит, поэтому здесь P/Invoke.
#[cfg(windows)]
const PROPERTIES_SCRIPT: &str = r#"$src = @"
using System;
using System.Runtime.InteropServices;
public static class ArchiverShell {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct SEI {
        public int cbSize; public uint fMask; public IntPtr hwnd;
        public string lpVerb; public string lpFile; public string lpParameters; public string lpDirectory;
        public int nShow; public IntPtr hInstApp; public IntPtr lpIDList; public string lpClass;
        public IntPtr hkeyClass; public uint dwHotKey; public IntPtr hIcon; public IntPtr hProcess;
    }
    [DllImport("shell32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool ShellExecuteEx(ref SEI info);
    public static bool ShowProperties(string path) {
        var info = new SEI();
        info.cbSize = Marshal.SizeOf(typeof(SEI));
        info.fMask = 0x0000000C;
        info.lpVerb = "properties";
        info.lpFile = path;
        info.nShow = 1;
        return ShellExecuteEx(ref info);
    }
}
"@
Add-Type -TypeDefinition $src
if (-not [ArchiverShell]::ShowProperties('__PATH__')) { exit 1 }
"#;

#[cfg(all(test, windows))]
mod tests {
    use super::{base64, powershell_encoded};

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn powershell_encoding_is_utf16_le() {
        // 'A' в UTF-16LE — 0x41 0x00, в base64 это «QQA=».
        assert_eq!(powershell_encoded("A"), "QQA=");
    }
}

impl MainWindow {
    /// Открывает извлечённые во временную папку файлы; итог — строкой в статус-бар.
    pub(crate) fn open_extracted(&mut self, paths: &[PathBuf], outcome: &JobOutcome) -> String {
        for path in paths {
            if !path.is_file() {
                return with_warnings(&format!("«{}» не извлечён", name_of(path)), outcome);
            }
            match shell_open(path, ShellVerb::Open) {
                // Дочерний процесс нужно дождаться: на Unix иначе остался бы
                // зомби. Отдельный поток — окно ждать внешнюю программу не должно.
                Ok(mut child) => {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
                Err(err) => return format!("Не удалось открыть «{}»: {err}", name_of(path)),
            }
        }
        with_warnings("Файл открыт во внешней программе", outcome)
    }

    /// F3/F4/Shift+Enter/Alt+Enter: открыть выделенное системной оболочкой.
    ///
    /// Внутри архива файла на диске нет: Enter/F3 извлекают его во временную
    /// папку и открывают ассоциированной программой, как 7-Zip FM. Правка с
    /// обратной упаковкой (F4) и свойства для содержимого архива не делаются.
    pub(crate) fn open_entry_with(
        &mut self,
        verb: ShellVerb,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (is_archive, is_dir, name, prefix, path) = {
            let panel = self.active();
            let Some(index) = panel
                .active
                .or_else(|| panel.selection.iter().next().copied())
            else {
                self.notice = Some("Ничего не выделено".to_string());
                return;
            };
            let Some(entry) = panel.entries.get(index) else {
                return;
            };
            (
                panel.is_archive,
                entry.is_dir,
                entry.name.clone(),
                panel.prefix.clone(),
                panel.path.join(&entry.name),
            )
        };

        if is_archive {
            if !matches!(verb, ShellVerb::Open) {
                self.notice =
                    Some("Внутри архива доступно только открытие (Enter или F3)".to_string());
                return;
            }
            if is_dir {
                return;
            }
            self.open_from_archive(format!("{prefix}{name}"), window, cx);
            return;
        }

        match shell_open(&path, verb) {
            Ok(mut child) => {
                // Свойства рисует оболочка после выхода PowerShell: если процесс
                // упал, человек должен увидеть причину, а не тишину.
                if matches!(verb, ShellVerb::Properties) {
                    cx.spawn(async move |this, cx| {
                        let status = cx
                            .background_executor()
                            .spawn(async move { child.wait() })
                            .await;
                        if !matches!(status, Ok(status) if status.success()) {
                            let _ = this.update(cx, |this, cx| {
                                this.notice = Some("Не удалось показать свойства".to_string());
                                cx.notify();
                            });
                        }
                    })
                    .detach();
                } else {
                    #[cfg(not(windows))]
                    cx.spawn(async move |_this, cx| {
                        let _ = cx
                            .background_executor()
                            .spawn(async move { child.wait() })
                            .await;
                    })
                    .detach();
                }
            }
            Err(err) => self.notice = Some(format!("Не удалось открыть: {err}")),
        }
    }
}

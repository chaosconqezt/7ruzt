//! Запуск `7z i` и разбор его вывода.

use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
};

use crate::version::{Arch, Version, parse_banner};

/// Что удалось узнать про конкретный бинарник 7-Zip.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub exe: PathBuf,
    pub version: Version,
    pub arch: Arch,
    /// Пути из секции `Libs:` — какие кодечные библиотеки реально подхватились.
    /// Пусто, если рядом с `7z.exe` нет `7z.dll`: тогда доступен только формат 7z.
    pub loaded_libs: Vec<PathBuf>,
}

impl ToolInfo {
    /// Загрузилась ли `7z.dll` (то есть доступны ли не-7z форматы).
    pub fn has_codec_dll(&self) -> bool {
        !self.loaded_libs.is_empty()
    }
}

#[derive(Debug)]
pub enum ProbeError {
    Spawn { exe: PathBuf, source: io::Error },
    NoBanner { exe: PathBuf },
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn { exe, source } => {
                write!(f, "не удалось запустить `{}`: {source}", exe.display())
            }
            Self::NoBanner { exe } => {
                write!(f, "`{} i` не вернул строку с версией", exe.display())
            }
        }
    }
}

impl std::error::Error for ProbeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } => Some(source),
            Self::NoBanner { .. } => None,
        }
    }
}

/// Опрашивает бинарник: `7z i` и разбор баннера/секции `Libs:`.
pub fn probe(exe: &Path) -> Result<ToolInfo, ProbeError> {
    let output = command(exe)
        .arg("i")
        .output()
        .map_err(|source| ProbeError::Spawn {
            exe: exe.to_path_buf(),
            source,
        })?;

    // Баннер и имена DLL — ASCII/UTF-8, lossy-декодирование безопасно.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let banner = stdout
        .lines()
        .find_map(parse_banner)
        .ok_or_else(|| ProbeError::NoBanner {
            exe: exe.to_path_buf(),
        })?;

    Ok(ToolInfo {
        exe: exe.to_path_buf(),
        version: banner.version,
        arch: banner.arch,
        loaded_libs: parse_libs(&stdout),
    })
}

/// Достаёт пути из блока
///
/// ```text
/// Libs:
///  0 : 26.06 : C:\...\7z.dll
/// ```
pub fn parse_libs(output: &str) -> Vec<PathBuf> {
    let mut libs = Vec::new();
    let mut inside = false;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed == "Libs:" {
            inside = true;
            continue;
        }
        if !inside || trimmed.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split(" : ").collect();
        match parts.as_slice() {
            [_, _, path] => libs.push(PathBuf::from(path.trim())),
            // Любая другая строка — блок закончился (дальше идут Formats: и Codecs:).
            _ => inside = false,
        }
    }

    libs
}

fn command(exe: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(exe);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: из GUI-процесса не должно мигать консольное окно.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_libs_block() {
        let output = "\n7-Zip 26.06 (x64) : Copyright\n\n\nLibs:\n 0 : 26.06 : C:\\app\\7z.dll\n\nFormats:\n";
        assert_eq!(parse_libs(output), vec![PathBuf::from("C:\\app\\7z.dll")]);
    }

    #[test]
    fn no_libs_block_means_no_dll() {
        let output = "\n7-Zip 26.06 (x64) : Copyright\n\nFormats:\n 0 C 7z\n";
        assert!(parse_libs(output).is_empty());
    }

    #[test]
    fn stops_at_next_section() {
        let output = "Libs:\n 0 : 26.06 : /usr/lib/7z.so\nFormats:\n 0 C 7z\n 1 C zip\n";
        assert_eq!(parse_libs(output), vec![PathBuf::from("/usr/lib/7z.so")]);
    }
}

//! Версия и архитектура 7-Zip, разобранные из вывода `7z i`.

use std::fmt;

/// Версия 7-Zip в виде `major.minor.build` (build почти всегда 0).
///
/// Историческая странность: minor всегда двузначный — `9.20`, `16.02`, `21.07`,
/// `26.06`. Поэтому `Display` печатает его с лидирующим нулём.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32, build: u32) -> Self {
        Self {
            major,
            minor,
            build,
        }
    }

    /// Первая версия с `-sccUTF-8` (UTF-8 вывод в консоль). Всё, что ниже,
    /// отдаёт имена файлов в системной кодировке — для них нужен отдельный путь декодирования.
    pub const UTF8_CONSOLE_SINCE: Self = Self::new(21, 7, 0);

    /// Ниже этой версии показываем предупреждение «устарела, возможны глюки».
    /// Не отказ: работаем, но честно предупреждаем.
    pub const OUTDATED_BELOW: Self = Self::new(23, 1, 0);

    /// Версия, с которой мы тестируем и которую рекомендуем.
    pub const RECOMMENDED: Self = Self::new(26, 6, 0);

    /// Умеет ли этот 7-Zip отдавать UTF-8 в консоль.
    pub fn supports_utf8_console(self) -> bool {
        self >= Self::UTF8_CONSOLE_SINCE
    }

    /// Стоит ли предупреждать пользователя, что версия старая.
    pub fn is_outdated(self) -> bool {
        self < Self::OUTDATED_BELOW
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.build == 0 {
            write!(f, "{}.{:02}", self.major, self.minor)
        } else {
            write!(f, "{}.{:02}.{}", self.major, self.minor, self.build)
        }
    }
}

/// Архитектура бинарника 7-Zip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86,
    X64,
    Arm64,
    Other,
}

impl Arch {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "x86" | "i386" | "i686" | "386" => Self::X86,
            "x64" | "amd64" => Self::X64,
            "arm64" | "aarch64" => Self::Arm64,
            _ => Self::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::X86 => "x86",
            Self::X64 => "x64",
            Self::Arm64 => "arm64",
            Self::Other => "unknown",
        }
    }
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Первая строка вывода `7z i`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Banner {
    pub version: Version,
    pub arch: Arch,
}

/// Разбирает строку вида:
///
/// * `7-Zip 26.06 (x64) : Copyright (c) 1999-2026 Igor Pavlov : 2026-11-06`
/// * `7-Zip (a) 24.09 (x64) : ...` — standalone-редакция (`7za`, `7zr`)
/// * `7-Zip (z) 23.01 (x64) : ...` — unix-редакция (`7zz`)
///
/// Возвращает `None`, если строка не баннер (в `7z i` есть ещё списки форматов и кодеков).
pub fn parse_banner(line: &str) -> Option<Banner> {
    let rest = line.trim().strip_prefix("7-Zip")?;
    let rest = rest.trim_start();

    // Метка редакции — ровно "(x)" из одной буквы.
    let rest = match rest.strip_prefix('(') {
        Some(after) if after.as_bytes().get(1) == Some(&b')') => after[2..].trim_start(),
        _ => rest,
    };

    let version_end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    let version = parse_version(&rest[..version_end])?;

    let tail = &rest[version_end..];
    let arch = tail
        .find('(')
        .and_then(|open| {
            let inner = &tail[open + 1..];
            inner.find(')').map(|close| Arch::parse(&inner[..close]))
        })
        .unwrap_or(Arch::Other);

    Some(Banner { version, arch })
}

fn parse_version(raw: &str) -> Option<Version> {
    let mut parts = raw.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    let build: u32 = match parts.next() {
        Some(part) => part.parse().ok()?,
        None => 0,
    };
    if parts.next().is_some() {
        return None;
    }
    Some(Version::new(major, minor, build))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_windows_banner() {
        let banner =
            parse_banner("7-Zip 26.06 (x64) : Copyright (c) 1999-2026 Igor Pavlov : 2026-11-06")
                .expect("баннер должен разобраться");
        assert_eq!(banner.version, Version::new(26, 6, 0));
        assert_eq!(banner.arch, Arch::X64);
        assert_eq!(banner.version.to_string(), "26.06");
    }

    #[test]
    fn parses_standalone_edition_marker() {
        let banner =
            parse_banner("7-Zip (a) 24.09 (x64) : Copyright (c) 1999-2024 Igor Pavlov").unwrap();
        assert_eq!(banner.version, Version::new(24, 9, 0));
        assert_eq!(banner.arch, Arch::X64);
    }

    #[test]
    fn parses_unix_7zz_banner() {
        let banner =
            parse_banner("7-Zip (z) 23.01 (x64) : Copyright (c) 1999-2023 Igor Pavlov").unwrap();
        assert_eq!(banner.version, Version::new(23, 1, 0));
    }

    #[test]
    fn parses_old_two_digit_minor() {
        let banner =
            parse_banner("7-Zip 9.20 (x86) : Copyright (c) 1999-2010 Igor Pavlov : 2010-11-18")
                .unwrap();
        assert_eq!(banner.version, Version::new(9, 20, 0));
        assert_eq!(banner.version.to_string(), "9.20");
        assert_eq!(banner.arch, Arch::X86);
    }

    #[test]
    fn banner_without_arch() {
        let banner = parse_banner("7-Zip 26.06 : Copyright (c) 1999-2026 Igor Pavlov").unwrap();
        assert_eq!(banner.arch, Arch::Other);
    }

    #[test]
    fn rejects_non_banner_lines() {
        assert!(parse_banner("Formats:").is_none());
        assert!(parse_banner("7-Zip").is_none());
        assert!(parse_banner("").is_none());
        assert!(parse_banner("7-Zip (a)").is_none());
    }

    #[test]
    fn thresholds_and_ordering() {
        assert!(Version::new(16, 4, 0).is_outdated());
        assert!(!Version::new(23, 1, 0).is_outdated());
        assert!(Version::new(21, 7, 0).supports_utf8_console());
        assert!(!Version::new(21, 6, 0).supports_utf8_console());
        assert!(Version::new(26, 6, 0) > Version::new(9, 20, 0));
    }
}

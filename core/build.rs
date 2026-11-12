//! Кладёт вендоренный 7-Zip рядом с исполняемым файлом программы.
//!
//! Скрипт живёт у ядра, потому что ядро — единственная зависимость и `app`,
//! и `cli`: их сборки кладут бинарники в одну папку `target/<профиль>`, так что
//! копия из одного места обслуживает обе. Иначе пришлось бы держать два почти
//! одинаковых `build.rs`.

use std::env;
use std::fs;
use std::path::Path;

/// Какие файлы 7-Zip кладём рядом с программой для каждой ОС.
/// License.txt распространяется вместе с бинарниками — его требует лицензия.
fn vendor_files(
    target_os: &str,
    target_arch: &str,
) -> Option<(&'static str, &'static [&'static str])> {
    match target_os {
        "windows" => Some((
            "../vendor/7zip/win-x64",
            &["7z.exe", "7z.dll", "License.txt"],
        )),
        // На unix официальный 7-Zip — один статический бинарь, DLL нет.
        "linux" => Some(("../vendor/7zip/linux-x64", &["7zz", "License.txt"])),
        // У macOS отдельные сборки для Apple Silicon и Intel: универсального
        // архива в релизах 7-Zip нет, поэтому выбираем по архитектуре цели.
        "macos" => match target_arch {
            "aarch64" => Some(("../vendor/7zip/macos-arm64", &["7zz", "License.txt"])),
            "x86_64" => Some(("../vendor/7zip/macos-x64", &["7zz", "License.txt"])),
            _ => None,
        },
        _ => None,
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../vendor/7zip");

    let out_dir = match env::var("OUT_DIR") {
        Ok(val) => val,
        Err(_) => return,
    };

    // `cfg!(target_os)` в build-скрипте — это ОС хоста, а нам нужна ОС сборки:
    // при кросс-компиляции они различаются.
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let Some((vendor, files)) = vendor_files(&target_os, &target_arch) else {
        return;
    };

    // Папка профиля (target/<triple>/release) — рядом с ней cargo кладёт бинарник.
    // Глубину не считаем: раскладка build-скриптов у cargo разная
    // (`build/<пакет>-<хеш>/out` в старых версиях и `build/<пакет>/<хеш>/out`
    // в новых). Фиксированный nth(3) на новой раскладке попадает в `build`,
    // и 7zz уезжает в target/<triple>/release/build/ вместо .../release/.
    let Some(dest_dir) = profile_dir(&out_dir) else {
        return;
    };

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let vendor_dir = Path::new(&manifest_dir).join(vendor);
    if !vendor_dir.exists() {
        return;
    }

    for file in files {
        let src = vendor_dir.join(file);
        let dst = dest_dir.join(file);
        copy(&src, &dst, &target_os);
    }
}

/// Папка профиля сборки: родитель ближайшего предка с именем `build`.
///
/// Так работает и старая раскладка (`.../release/build/<пакет>-<хеш>/out`),
/// и новая (`.../release/build/<пакет>/<хеш>/out`).
fn profile_dir(out_dir: &str) -> Option<&Path> {
    let build = Path::new(out_dir)
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "build"))?;
    build.parent()
}

fn copy(src: &Path, dst: &Path, target_os: &str) {
    if !src.exists() {
        println!(
            "cargo:warning=вендоренный файл не найден: {}",
            src.display()
        );
        return;
    }
    if let Err(err) = fs::copy(src, dst) {
        // Не валим сборку (exe может держать запущенное приложение), но молчать
        // нельзя: без 7z рядом программа не работает.
        println!(
            "cargo:warning=не удалось скопировать {} -> {}: {err}",
            src.display(),
            dst.display()
        );
        return;
    }
    // Из git на Linux бинарник приходит без бита исполнения (Windows его не
    // хранит), поэтому выставляем права здесь. Бит нужен только unix-целям;
    // на unix-хосте это доступно, при кросс-сборке с Windows — нет (тогда бит
    // должен прийти из git/CI).
    #[cfg(unix)]
    if matches!(target_os, "linux" | "macos") {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(dst, fs::Permissions::from_mode(0o755));
    }
    #[cfg(not(unix))]
    let _ = target_os;
}

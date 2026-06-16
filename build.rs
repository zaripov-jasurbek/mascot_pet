use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=vendor/vosk-win64-0.3.45");

    if env::var("CARGO_CFG_WINDOWS").is_err() {
        return;
    }

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let vosk_dir = manifest_dir.join("vendor").join("vosk-win64-0.3.45");
    if !vosk_dir.exists() {
        return;
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let Some(profile_dir) = out_dir
        .ancestors()
        .nth(3)
        .map(PathBuf::from)
    else {
        return;
    };

    let Ok(entries) = fs::read_dir(&vosk_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("dll")) {
            let Some(file_name) = path.file_name() else {
                continue;
            };
            let dest = profile_dir.join(file_name);
            fs::copy(&path, dest).expect("failed to copy Vosk runtime DLL");
        }
    }
}

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::paths;

pub struct ImportReport {
    pub source: PathBuf,
    pub output_pack: PathBuf,
    pub pack_entries: usize,
    pub extracted_sound_files: usize,
}

pub fn import_jar_mod(path: &Path) -> Result<ImportReport, String> {
    if !path.exists() {
        return Err(format!("Input file does not exist: {}", path.to_string_lossy()));
    }
    if !path.is_file() {
        return Err(format!("Input path is not a file: {}", path.to_string_lossy()));
    }

    let input_file = File::open(path)
        .map_err(|err| format!("Failed to open '{}': {err}", path.to_string_lossy()))?;
    let mut archive = ZipArchive::new(input_file)
        .map_err(|err| format!("'{}' is not a valid zip/jar archive: {err}", path.to_string_lossy()))?;

    let assets_root = selected_assets_root();
    fs::create_dir_all(&assets_root)
        .map_err(|err| format!("Failed to create assets root '{}': {err}", assets_root.to_string_lossy()))?;

    let stem = sanitize_file_stem(path);
    let packs_dir = assets_root.join("packs");
    fs::create_dir_all(&packs_dir)
        .map_err(|err| format!("Failed to create packs dir '{}': {err}", packs_dir.to_string_lossy()))?;
    let output_pack = packs_dir.join(format!("{stem}.rcpack.zip"));

    let out_file = File::create(&output_pack).map_err(|err| {
        format!(
            "Failed to create output pack '{}': {err}",
            output_pack.to_string_lossy()
        )
    })?;
    let mut writer = ZipWriter::new(out_file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let sounds_root = assets_root.join("sound").join("mods").join(&stem);
    if let Err(err) = fs::create_dir_all(&sounds_root) {
        log::warn!(
            "Failed to create extracted sounds directory '{}': {}",
            sounds_root.to_string_lossy(),
            err
        );
    }

    let mut pack_entries = 0usize;
    let mut extracted_sound_files = 0usize;
    let mut written_paths: HashSet<String> = HashSet::new();

    for idx in 0..archive.len() {
        let mut entry = match archive.by_index(idx) {
            Ok(entry) => entry,
            Err(err) => {
                log::warn!(
                    "Failed to read entry #{} in '{}': {}",
                    idx,
                    path.to_string_lossy(),
                    err
                );
                continue;
            }
        };
        if entry.is_dir() {
            continue;
        }

        let normalized = normalize_archive_path(entry.name());
        let pack_target = map_pack_entry_path(&normalized);
        let sound_target = map_sound_extract_path(&normalized);
        if pack_target.is_none() && sound_target.is_none() {
            continue;
        }

        let mut bytes = Vec::new();
        if let Err(err) = entry.read_to_end(&mut bytes) {
            log::warn!(
                "Failed to read '{}' from '{}': {}",
                entry.name(),
                path.to_string_lossy(),
                err
            );
            continue;
        }

        if let Some(pack_path) = pack_target {
            if written_paths.insert(pack_path.clone()) {
                if let Err(err) = writer.start_file(pack_path, options) {
                    log::warn!(
                        "Failed to write entry into '{}': {}",
                        output_pack.to_string_lossy(),
                        err
                    );
                } else if let Err(err) = writer.write_all(&bytes) {
                    log::warn!(
                        "Failed to write entry into '{}': {}",
                        output_pack.to_string_lossy(),
                        err
                    );
                } else {
                    pack_entries += 1;
                }
            }
        }

        if let Some(sound_rel) = sound_target {
            let out_path = sounds_root.join(sound_rel);
            if let Some(parent) = out_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Err(err) = fs::write(&out_path, &bytes) {
                log::warn!(
                    "Failed to extract sound '{}': {}",
                    out_path.to_string_lossy(),
                    err
                );
            } else {
                extracted_sound_files += 1;
            }
        }
    }

    writer.finish().map_err(|err| {
        format!(
            "Failed to finalize output pack '{}': {err}",
            output_pack.to_string_lossy()
        )
    })?;

    if pack_entries == 0 && extracted_sound_files == 0 {
        let _ = fs::remove_file(&output_pack);
        return Err(
            "No supported textures/sounds found. Expected assets/<namespace>/textures or assets/<namespace>/sounds entries."
                .to_string(),
        );
    }

    Ok(ImportReport {
        source: path.to_path_buf(),
        output_pack,
        pack_entries,
        extracted_sound_files,
    })
}

fn selected_assets_root() -> PathBuf {
    let roots = paths::asset_roots();
    if let Some(existing) = roots.iter().find(|p| p.exists()) {
        return existing.clone();
    }
    roots
        .into_iter()
        .next()
        .unwrap_or_else(|| PathBuf::from("assets"))
}

fn sanitize_file_stem(path: &Path) -> String {
    let raw = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("mod")
        .trim();
    let mut out = String::with_capacity(raw.len().max(3));
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "mod".to_string()
    } else {
        out
    }
}

fn normalize_archive_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches('/')
        .to_ascii_lowercase()
}

fn map_pack_entry_path(path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 4 || parts[0] != "assets" {
        return None;
    }
    if parts.iter().any(|p| p.is_empty() || *p == "." || *p == "..") {
        return None;
    }

    let namespace = parts[1];
    let category = parts[2];
    let rel = parts[3..].join("/");
    if rel.is_empty() {
        return None;
    }

    match category {
        "textures" if has_supported_image_ext(path) => {
            Some(format!("assets/{namespace}/textures/{rel}"))
        }
        "sounds" if has_supported_audio_ext(path) || rel == "sounds.json" => {
            Some(format!("assets/{namespace}/sounds/{rel}"))
        }
        "models" | "blockstates" if rel.ends_with(".json") => {
            Some(format!("assets/{namespace}/{category}/{rel}"))
        }
        "lang" if rel.ends_with(".json") || rel.ends_with(".lang") || rel.ends_with(".txt") => {
            Some(format!("assets/{namespace}/lang/{rel}"))
        }
        _ => None,
    }
}

fn map_sound_extract_path(path: &str) -> Option<PathBuf> {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 5 || parts[0] != "assets" || parts[2] != "sounds" {
        return None;
    }
    if parts.iter().any(|p| p.is_empty() || *p == "." || *p == "..") {
        return None;
    }
    if !has_supported_audio_ext(path) {
        return None;
    }
    let namespace = parts[1];
    let rel = parts[3..].join("/");
    if rel.is_empty() {
        return None;
    }
    Some(PathBuf::from(namespace).join(rel))
}

fn has_supported_image_ext(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "avif")
}

fn has_supported_audio_ext(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    matches!(ext.as_str(), "ogg" | "mp3" | "wav" | "flac")
}

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::menu::Settings;
use crate::world::block::Block;

pub type BlockMap = HashMap<(i32, i32, i32), Block>;

fn saves_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("saves")
}

fn ensure_saves_dir() -> Option<PathBuf> {
    let dir = saves_dir();
    match fs::create_dir_all(&dir) {
        Ok(_) => Some(dir),
        Err(err) => {
            log::warn!("Failed to create saves dir {:?}: {}", dir, err);
            None
        }
    }
}

fn world_blocks_path(seed: u32) -> Option<PathBuf> {
    ensure_saves_dir().map(|dir| dir.join(format!("world_{seed}.blocks")))
}

fn settings_path() -> Option<PathBuf> {
    ensure_saves_dir().map(|dir| dir.join("settings.cfg"))
}

pub fn load_world_blocks(seed: u32) -> BlockMap {
    let Some(path) = world_blocks_path(seed) else {
        return HashMap::new();
    };
    let Ok(text) = fs::read_to_string(&path) else {
        return HashMap::new();
    };

    let mut out = HashMap::new();
    for (line_idx, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let x = parts.next().and_then(|v| v.parse::<i32>().ok());
        let y = parts.next().and_then(|v| v.parse::<i32>().ok());
        let z = parts.next().and_then(|v| v.parse::<i32>().ok());
        let id = parts.next().and_then(|v| v.parse::<u8>().ok());
        let Some((x, y, z, id)) = x.zip(y).zip(z).zip(id).map(|(((a, b), c), d)| (a, b, c, d)) else {
            log::warn!(
                "Skipping malformed world block line {} in {:?}",
                line_idx + 1,
                path
            );
            continue;
        };
        if let Some(block) = Block::from_id(id) {
            out.insert((x, y, z), block);
        }
    }
    out
}

pub fn save_world_blocks(seed: u32, blocks: &BlockMap) {
    let Some(path) = world_blocks_path(seed) else {
        return;
    };
    let mut entries: Vec<_> = blocks.iter().collect();
    entries.sort_unstable_by_key(|((x, y, z), _)| (*x, *y, *z));

    let mut text = String::with_capacity(entries.len() * 20);
    text.push_str("# x y z block_id\n");
    for ((x, y, z), block) in entries {
        text.push_str(&format!("{x} {y} {z} {}\n", block.id()));
    }

    match fs::File::create(&path).and_then(|mut f| f.write_all(text.as_bytes())) {
        Ok(_) => {}
        Err(err) => log::warn!("Failed to save world blocks {:?}: {}", path, err),
    }
}

pub fn load_settings() -> Option<Settings> {
    let path = settings_path()?;
    let text = fs::read_to_string(path).ok()?;

    let mut map = HashMap::<String, String>::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }

    Some(Settings {
        render_dist: parse_i32(&map, "render_dist", 6),
        fly_speed: parse_f32(&map, "fly_speed", 8.0),
        mouse_sens: parse_f32(&map, "mouse_sens", 0.15),
        vsync: parse_bool(&map, "vsync", false),
        show_fps: parse_bool(&map, "show_fps", true),
        ambient_boost: parse_f32(&map, "ambient_boost", 1.02),
        sun_softness: parse_f32(&map, "sun_softness", 0.34),
        fog_density: parse_f32(&map, "fog_density", 1.0),
        exposure: parse_f32(&map, "exposure", 1.0),
    })
}

pub fn save_settings(settings: &Settings) {
    let Some(path) = settings_path() else {
        return;
    };
    let text = format!(
        "# Engine settings\n\
         render_dist={}\n\
         fly_speed={}\n\
         mouse_sens={}\n\
         vsync={}\n\
         show_fps={}\n\
         ambient_boost={}\n\
         sun_softness={}\n\
         fog_density={}\n\
         exposure={}\n",
        settings.render_dist,
        settings.fly_speed,
        settings.mouse_sens,
        settings.vsync,
        settings.show_fps,
        settings.ambient_boost,
        settings.sun_softness,
        settings.fog_density,
        settings.exposure
    );

    match fs::File::create(&path).and_then(|mut f| f.write_all(text.as_bytes())) {
        Ok(_) => {}
        Err(err) => log::warn!("Failed to save settings {:?}: {}", path, err),
    }
}

fn parse_i32(map: &HashMap<String, String>, key: &str, default: i32) -> i32 {
    map.get(key)
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(default)
}

fn parse_f32(map: &HashMap<String, String>, key: &str, default: f32) -> f32 {
    map.get(key)
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(default)
}

fn parse_bool(map: &HashMap<String, String>, key: &str, default: bool) -> bool {
    map.get(key)
        .map(|v| {
            let vv = v.to_ascii_lowercase();
            vv == "1" || vv == "true" || vv == "yes" || vv == "on"
        })
        .unwrap_or(default)
}

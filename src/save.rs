use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::inventory::{Inventory, Slot, WoodenTool};
use crate::menu::Settings;
use crate::world::block::Block;

pub type BlockMap = HashMap<(i32, i32, i32), Block>;

pub struct InventoryState {
    pub selected: usize,
    pub hotbar: [Slot; 9],
    pub grid: [Slot; 27],
    pub tool_durability: [u16; 4],
}

static WORLD_FILE_OVERRIDES: OnceLock<Mutex<HashMap<u32, PathBuf>>> = OnceLock::new();

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

fn world_file_overrides() -> &'static Mutex<HashMap<u32, PathBuf>> {
    WORLD_FILE_OVERRIDES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn world_file_override(seed: u32) -> Option<PathBuf> {
    world_file_overrides()
        .lock()
        .ok()
        .and_then(|m| m.get(&seed).cloned())
}

pub fn set_world_file_override(seed: u32, path: PathBuf) {
    if let Ok(mut map) = world_file_overrides().lock() {
        map.insert(seed, path);
    }
}

pub fn clear_world_file_override(seed: u32) {
    if let Ok(mut map) = world_file_overrides().lock() {
        map.remove(&seed);
    }
}

fn world_blocks_rc_path(seed: u32) -> Option<PathBuf> {
    if let Some(path) = world_file_override(seed) {
        return Some(path);
    }
    ensure_saves_dir().map(|dir| dir.join(format!("world_{seed}.rc")))
}

fn world_blocks_legacy_path(seed: u32) -> Option<PathBuf> {
    ensure_saves_dir().map(|dir| dir.join(format!("world_{seed}.blocks")))
}

fn inventory_path(seed: u32) -> Option<PathBuf> {
    if let Some(world_path) = world_file_override(seed) {
        let stem = world_path.file_stem()?.to_str()?;
        let parent = world_path.parent().map(Path::to_path_buf).unwrap_or_else(saves_dir);
        return Some(parent.join(format!("{stem}.inventory.rc")));
    }
    ensure_saves_dir().map(|dir| dir.join(format!("world_{seed}.inventory.rc")))
}

fn settings_path() -> Option<PathBuf> {
    ensure_saves_dir().map(|dir| dir.join("settings.cfg"))
}

pub fn resolve_world_seed_from_path(path: &Path) -> u32 {
    infer_world_seed_from_path(path).unwrap_or_else(|| hash_path_seed(path))
}

pub fn infer_world_seed_from_path(path: &Path) -> Option<u32> {
    if let Some(seed) = infer_seed_from_filename(path) {
        return Some(seed);
    }
    infer_seed_from_file_header(path)
}

fn infer_seed_from_filename(path: &Path) -> Option<u32> {
    let stem = path.file_stem()?.to_str()?.to_ascii_lowercase();
    let rest = stem.strip_prefix("world_")?;
    rest.parse::<u32>().ok()
}

fn infer_seed_from_file_header(path: &Path) -> Option<u32> {
    let text = fs::read_to_string(path).ok()?;
    for line in text.lines().take(8) {
        let trimmed = line.trim();
        if !trimmed.starts_with('#') {
            continue;
        }
        let body = trimmed.trim_start_matches('#').trim();
        let body = body.strip_prefix("seed=")?;
        if let Ok(seed) = body.trim().parse::<u32>() {
            return Some(seed);
        }
    }
    None
}

fn hash_path_seed(path: &Path) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let norm = path.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    for b in norm.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    if h == 0 { 1 } else { h }
}

pub fn load_world_blocks(seed: u32) -> BlockMap {
    let has_override = world_file_override(seed).is_some();
    let Some(rc_path) = world_blocks_rc_path(seed) else {
        return HashMap::new();
    };

    if let Ok(text) = fs::read_to_string(&rc_path) {
        return parse_world_blocks_text(&text, &rc_path);
    }
    if has_override {
        return HashMap::new();
    }

    let Some(legacy_path) = world_blocks_legacy_path(seed) else {
        return HashMap::new();
    };
    let Ok(text) = fs::read_to_string(&legacy_path) else {
        return HashMap::new();
    };

    let out = parse_world_blocks_text(&text, &legacy_path);
    if !out.is_empty() {
        log::info!(
            "Loaded legacy world format {:?}; saving migrated copy to {:?}",
            legacy_path,
            rc_path
        );
        save_world_blocks(seed, &out);
    }
    out
}

fn parse_world_blocks_text(text: &str, path: &Path) -> BlockMap {
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
    let Some(path) = world_blocks_rc_path(seed) else {
        return;
    };
    ensure_parent_dir(&path);
    let mut entries: Vec<_> = blocks.iter().collect();
    entries.sort_unstable_by_key(|((x, y, z), _)| (*x, *y, *z));

    let mut text = String::with_capacity(entries.len() * 20);
    text.push_str(&format!("# seed={seed}\n"));
    text.push_str("# x y z block_id\n");
    for ((x, y, z), block) in entries {
        text.push_str(&format!("{x} {y} {z} {}\n", block.id()));
    }

    match fs::File::create(&path).and_then(|mut f| f.write_all(text.as_bytes())) {
        Ok(_) => {}
        Err(err) => log::warn!("Failed to save world blocks {:?}: {}", path, err),
    }
}

pub fn load_inventory_state(seed: u32) -> Option<InventoryState> {
    let path = inventory_path(seed)?;
    let text = fs::read_to_string(&path).ok()?;

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

    let selected = parse_usize(&map, "selected", 0).min(8);
    let mut hotbar = [Slot::empty(); 9];
    let mut grid = [Slot::empty(); 27];
    parse_slot_array(map.get("hotbar").map(String::as_str).unwrap_or(""), &mut hotbar);
    parse_slot_array(map.get("grid").map(String::as_str).unwrap_or(""), &mut grid);
    let tool_durability =
        parse_tool_durability(map.get("tool_durability").map(String::as_str).unwrap_or(""));

    Some(InventoryState {
        selected,
        hotbar,
        grid,
        tool_durability,
    })
}

pub fn save_inventory_state(
    seed: u32,
    inventory: &Inventory,
    tool_durability: [u16; 4],
) {
    let Some(path) = inventory_path(seed) else {
        return;
    };
    ensure_parent_dir(&path);
    let hotbar = inventory
        .hotbar
        .iter()
        .map(encode_slot)
        .collect::<Vec<_>>()
        .join(";");
    let grid = inventory
        .grid
        .iter()
        .map(encode_slot)
        .collect::<Vec<_>>()
        .join(";");
    let durability = format!(
        "{},{},{},{}",
        tool_durability[0], tool_durability[1], tool_durability[2], tool_durability[3]
    );

    let text = format!(
        "# RustyCraft inventory\n\
         selected={}\n\
         hotbar={}\n\
         grid={}\n\
         tool_durability={}\n",
        inventory.selected.min(8),
        hotbar,
        grid,
        durability
    );

    match fs::File::create(&path).and_then(|mut f| f.write_all(text.as_bytes())) {
        Ok(_) => {}
        Err(err) => log::warn!("Failed to save inventory {:?}: {}", path, err),
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
        mouse_sens: parse_f32(&map, "mouse_sens", 0.15),
        fov: parse_f32(&map, "fov", 70.0),
        vsync: parse_bool(&map, "vsync", false),
        ray_tracing: parse_bool(&map, "ray_tracing", false),
        show_fps: parse_bool(&map, "show_fps", true),
        master_volume: parse_f32(&map, "master_volume", 1.0),
        music_volume: parse_f32(&map, "music_volume", 0.70),
        ambient_volume: parse_f32(&map, "ambient_volume", 0.90),
        sfx_volume: parse_f32(&map, "sfx_volume", 1.0),
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
         mouse_sens={}\n\
         fov={}\n\
         vsync={}\n\
         ray_tracing={}\n\
         show_fps={}\n\
         master_volume={}\n\
         music_volume={}\n\
         ambient_volume={}\n\
         sfx_volume={}\n\
         ambient_boost={}\n\
         sun_softness={}\n\
         fog_density={}\n\
         exposure={}\n",
        settings.render_dist,
        settings.mouse_sens,
        settings.fov,
        settings.vsync,
        settings.ray_tracing,
        settings.show_fps,
        settings.master_volume,
        settings.music_volume,
        settings.ambient_volume,
        settings.sfx_volume,
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

fn ensure_parent_dir(path: &Path) {
    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            log::warn!("Failed to create directory {:?}: {}", parent, err);
        }
    }
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

fn parse_usize(map: &HashMap<String, String>, key: &str, default: usize) -> usize {
    map.get(key)
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default)
}

fn parse_slot_array<const N: usize>(raw: &str, out: &mut [Slot; N]) {
    for (i, token) in raw.split(';').enumerate() {
        if i >= N {
            break;
        }
        out[i] = decode_slot(token).unwrap_or_else(Slot::empty);
    }
}

fn parse_tool_durability(raw: &str) -> [u16; 4] {
    let mut out = [0u16; 4];
    for (i, token) in raw.split(',').enumerate() {
        if i >= out.len() {
            break;
        }
        if let Ok(v) = token.trim().parse::<u16>() {
            out[i] = v;
        }
    }
    out
}

fn encode_slot(slot: &Slot) -> String {
    if slot.is_empty() {
        return "e".to_string();
    }
    if let Some(tool) = slot.tool {
        return format!("t,{}", tool_key(tool));
    }
    format!("b,{},{}", slot.block.id(), slot.count)
}

fn decode_slot(raw: &str) -> Option<Slot> {
    let token = raw.trim();
    if token.is_empty() || token.eq_ignore_ascii_case("e") {
        return Some(Slot::empty());
    }

    let mut parts = token.split(',');
    let kind = parts.next()?.trim();
    if kind.eq_ignore_ascii_case("t") {
        let tool = parse_tool_key(parts.next()?.trim())?;
        return Some(Slot::new_tool(tool));
    }
    if kind.eq_ignore_ascii_case("b") {
        let id = parts.next()?.trim().parse::<u8>().ok()?;
        let count = parts.next()?.trim().parse::<u16>().ok().unwrap_or(1);
        let block = Block::from_id(id)?;
        return Some(Slot::new(block, count.max(1)));
    }
    None
}

fn tool_key(tool: WoodenTool) -> &'static str {
    match tool {
        WoodenTool::Pickaxe => "pickaxe",
        WoodenTool::Axe => "axe",
        WoodenTool::Shovel => "shovel",
        WoodenTool::Hoe => "hoe",
    }
}

fn parse_tool_key(raw: &str) -> Option<WoodenTool> {
    let key = raw.trim().to_ascii_lowercase();
    match key.as_str() {
        "pickaxe" | "wooden_pickaxe" | "wp" => Some(WoodenTool::Pickaxe),
        "axe" | "wooden_axe" | "wa" => Some(WoodenTool::Axe),
        "shovel" | "wooden_shovel" | "ws" => Some(WoodenTool::Shovel),
        "hoe" | "wooden_hoe" | "wh" => Some(WoodenTool::Hoe),
        _ => None,
    }
}

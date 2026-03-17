use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::inventory::{Inventory, Slot, WoodenTool};
use crate::menu::Settings;
use crate::paths;
use crate::world::block::Block;

pub type BlockMap = HashMap<(i32, i32, i32), Block>;

pub struct InventoryState {
    pub selected: usize,
    pub hotbar: [Slot; 9],
    pub grid: [Slot; 27],
    pub tool_durability: [u16; 15],
}

pub struct PlayerState {
    pub pos: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub day_time: f32,
    pub health: f32,
    pub hunger: f32,
    pub stamina: f32,
    pub rain_active: bool,
    pub rain_strength: f32,
    pub surface_wetness: f32,
}

static WORLD_FILE_OVERRIDES: OnceLock<Mutex<HashMap<u32, PathBuf>>> = OnceLock::new();
const WORLD_DIR_BLOCKS_FILE: &str = "world.rc";
const WORLD_DIR_INVENTORY_FILE: &str = "inventory.rc";
const WORLD_DIR_PLAYER_FILE: &str = "player.rc";

#[derive(Debug, Clone)]
struct SavePaths {
    world_blocks: PathBuf,
    inventory: PathBuf,
    player: PathBuf,
    legacy_world_blocks: Vec<PathBuf>,
    legacy_inventory: Vec<PathBuf>,
    legacy_player: Vec<PathBuf>,
}

fn saves_dir() -> PathBuf {
    paths::saves_dir()
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

fn has_rc_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("rc"))
        .unwrap_or(false)
}

fn world_dir_name(seed: u32) -> String {
    format!("world_{seed}")
}

fn default_world_dir(seed: u32) -> Option<PathBuf> {
    ensure_saves_dir().map(|dir| dir.join(world_dir_name(seed)))
}

fn save_paths(seed: u32) -> Option<SavePaths> {
    if let Some(path) = world_file_override(seed) {
        if has_rc_extension(&path) {
            let parent = path.parent().map(Path::to_path_buf).unwrap_or_else(saves_dir);
            let stem = path.file_stem()?.to_str()?;
            let root = parent.join(stem);
            return Some(SavePaths {
                world_blocks: root.join(WORLD_DIR_BLOCKS_FILE),
                inventory: root.join(WORLD_DIR_INVENTORY_FILE),
                player: root.join(WORLD_DIR_PLAYER_FILE),
                legacy_world_blocks: vec![path.clone()],
                legacy_inventory: vec![parent.join(format!("{stem}.inventory.rc"))],
                legacy_player: vec![parent.join(format!("{stem}.player.rc"))],
            });
        }

        return Some(SavePaths {
            world_blocks: path.join(WORLD_DIR_BLOCKS_FILE),
            inventory: path.join(WORLD_DIR_INVENTORY_FILE),
            player: path.join(WORLD_DIR_PLAYER_FILE),
            legacy_world_blocks: Vec::new(),
            legacy_inventory: Vec::new(),
            legacy_player: Vec::new(),
        });
    }

    let root = default_world_dir(seed)?;
    let saves = saves_dir();
    Some(SavePaths {
        world_blocks: root.join(WORLD_DIR_BLOCKS_FILE),
        inventory: root.join(WORLD_DIR_INVENTORY_FILE),
        player: root.join(WORLD_DIR_PLAYER_FILE),
        legacy_world_blocks: vec![
            saves.join(format!("world_{seed}.rc")),
            saves.join(format!("world_{seed}.blocks")),
        ],
        legacy_inventory: vec![saves.join(format!("world_{seed}.inventory.rc"))],
        legacy_player: vec![saves.join(format!("world_{seed}.player.rc"))],
    })
}

fn world_blocks_rc_path(seed: u32) -> Option<PathBuf> {
    save_paths(seed).map(|p| p.world_blocks)
}

fn inventory_path(seed: u32) -> Option<PathBuf> {
    save_paths(seed).map(|p| p.inventory)
}

fn player_path(seed: u32) -> Option<PathBuf> {
    save_paths(seed).map(|p| p.player)
}

fn settings_path() -> Option<PathBuf> {
    ensure_saves_dir().map(|dir| dir.join("settings.cfg"))
}

pub fn resolve_world_seed_from_path(path: &Path) -> u32 {
    infer_world_seed_from_path(path).unwrap_or_else(|| hash_path_seed(path))
}

pub fn infer_world_seed_from_path(path: &Path) -> Option<u32> {
    if let Some(seed) = infer_seed_from_path_name(path) {
        return Some(seed);
    }
    if has_rc_extension(path) {
        return infer_seed_from_file_header(path);
    }
    infer_seed_from_file_header(&path.join(WORLD_DIR_BLOCKS_FILE))
}

fn infer_seed_from_path_name(path: &Path) -> Option<u32> {
    let name = if has_rc_extension(path) {
        path.file_stem()?.to_str()?
    } else {
        path.file_name()?.to_str()?
    };
    infer_seed_from_name(name)
}

fn infer_seed_from_name(name: &str) -> Option<u32> {
    let lower = name.to_ascii_lowercase();
    let rest = lower.strip_prefix("world_")?;
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
    let Some(paths) = save_paths(seed) else {
        return HashMap::new();
    };
    let rc_path = &paths.world_blocks;

    if let Ok(text) = fs::read_to_string(rc_path) {
        return parse_world_blocks_text(&text, rc_path);
    }

    for legacy_path in &paths.legacy_world_blocks {
        let Ok(text) = fs::read_to_string(legacy_path) else {
            continue;
        };

        let out = parse_world_blocks_text(&text, legacy_path);
        log::info!(
            "Loaded legacy world format {:?}; saving migrated copy to {:?}",
            legacy_path,
            rc_path
        );
        save_world_blocks(seed, &out);
        return out;
    }

    HashMap::new()
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
    let paths = save_paths(seed)?;

    if let Ok(text) = fs::read_to_string(&paths.inventory) {
        if let Some(state) = parse_inventory_state_text(&text) {
            return Some(state);
        }
    }

    for legacy_path in &paths.legacy_inventory {
        let Ok(text) = fs::read_to_string(legacy_path) else {
            continue;
        };
        let Some(state) = parse_inventory_state_text(&text) else {
            continue;
        };
        write_inventory_state(&paths.inventory, &state);
        log::info!(
            "Loaded legacy inventory format {:?}; saving migrated copy to {:?}",
            legacy_path,
            paths.inventory
        );
        return Some(state);
    }

    None
}

pub fn save_inventory_state(
    seed: u32,
    inventory: &Inventory,
    tool_durability: [u16; 15],
) {
    let Some(path) = inventory_path(seed) else {
        return;
    };
    let state = InventoryState {
        selected: inventory.selected.min(8),
        hotbar: inventory.hotbar,
        grid: inventory.grid,
        tool_durability,
    };
    write_inventory_state(&path, &state);
}

pub fn load_player_state(seed: u32) -> Option<PlayerState> {
    let paths = save_paths(seed)?;

    if let Ok(text) = fs::read_to_string(&paths.player) {
        if let Some(state) = parse_player_state_text(&text) {
            return Some(state);
        }
    }

    for legacy_path in &paths.legacy_player {
        let Ok(text) = fs::read_to_string(legacy_path) else {
            continue;
        };
        let Some(state) = parse_player_state_text(&text) else {
            continue;
        };
        save_player_state(seed, &state);
        log::info!(
            "Loaded legacy player format {:?}; saving migrated copy to {:?}",
            legacy_path,
            paths.player
        );
        return Some(state);
    }

    None
}

pub fn save_player_state(seed: u32, state: &PlayerState) {
    let Some(path) = player_path(seed) else {
        return;
    };
    ensure_parent_dir(&path);
    let text = format!(
        "# RustyCraft player\n\
         pos_x={}\n\
         pos_y={}\n\
         pos_z={}\n\
         yaw={}\n\
         pitch={}\n\
         day_time={}\n\
         health={}\n\
         hunger={}\n\
         stamina={}\n\
         rain_active={}\n\
         rain_strength={}\n\
         surface_wetness={}\n",
        state.pos[0],
        state.pos[1],
        state.pos[2],
        state.yaw,
        state.pitch,
        state.day_time,
        state.health,
        state.hunger,
        state.stamina,
        state.rain_active,
        state.rain_strength,
        state.surface_wetness
    );

    match fs::File::create(&path).and_then(|mut f| f.write_all(text.as_bytes())) {
        Ok(_) => {}
        Err(err) => log::warn!("Failed to save player state {:?}: {}", path, err),
    }
}

fn parse_inventory_state_text(text: &str) -> Option<InventoryState> {
    let map = parse_kv_map(text);
    if map.is_empty() {
        return None;
    }

    let selected = parse_usize(&map, "selected", 0).min(8);
    let mut hotbar = [Slot::empty(); 9];
    let mut grid = [Slot::empty(); 27];
    parse_slot_array(
        map.get("hotbar").map(String::as_str).unwrap_or(""),
        &mut hotbar,
    );
    parse_slot_array(
        map.get("grid").map(String::as_str).unwrap_or(""),
        &mut grid,
    );
    let tool_durability = parse_tool_durability(
        map.get("tool_durability").map(String::as_str).unwrap_or(""),
    );

    Some(InventoryState {
        selected,
        hotbar,
        grid,
        tool_durability,
    })
}

fn write_inventory_state(path: &Path, state: &InventoryState) {
    ensure_parent_dir(path);
    let hotbar = state
        .hotbar
        .iter()
        .map(encode_slot)
        .collect::<Vec<_>>()
        .join(";");
    let grid = state
        .grid
        .iter()
        .map(encode_slot)
        .collect::<Vec<_>>()
        .join(";");
    let durability = state
        .tool_durability
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let text = format!(
        "# RustyCraft inventory\n\
         selected={}\n\
         hotbar={}\n\
         grid={}\n\
         tool_durability={}\n",
        state.selected.min(8),
        hotbar,
        grid,
        durability
    );

    match fs::File::create(path).and_then(|mut f| f.write_all(text.as_bytes())) {
        Ok(_) => {}
        Err(err) => log::warn!("Failed to save inventory {:?}: {}", path, err),
    }
}

fn parse_player_state_text(text: &str) -> Option<PlayerState> {
    let map = parse_kv_map(text);
    if map.is_empty() {
        return None;
    }

    Some(PlayerState {
        pos: [
            parse_f32(&map, "pos_x", 0.0),
            parse_f32(&map, "pos_y", 0.0),
            parse_f32(&map, "pos_z", 0.0),
        ],
        yaw: parse_f32(&map, "yaw", -90.0),
        pitch: parse_f32(&map, "pitch", 0.0),
        day_time: parse_f32(&map, "day_time", 0.25),
        health: parse_f32(&map, "health", 20.0),
        hunger: parse_f32(&map, "hunger", 20.0),
        stamina: parse_f32(&map, "stamina", 20.0),
        rain_active: parse_bool(&map, "rain_active", false),
        rain_strength: parse_f32(&map, "rain_strength", 0.0),
        surface_wetness: parse_f32(&map, "surface_wetness", 0.0),
    })
}

fn parse_kv_map(text: &str) -> HashMap<String, String> {
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
    map
}

pub fn load_settings() -> Option<Settings> {
    let path = settings_path()?;
    let text = fs::read_to_string(path).ok()?;
    let map = parse_kv_map(&text);

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

fn parse_tool_durability(raw: &str) -> [u16; 15] {
    let mut out = [0u16; 15];
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
        WoodenTool::Sword => "sword",
        WoodenTool::StonePickaxe => "stone_pickaxe",
        WoodenTool::StoneAxe => "stone_axe",
        WoodenTool::StoneShovel => "stone_shovel",
        WoodenTool::StoneHoe => "stone_hoe",
        WoodenTool::StoneSword => "stone_sword",
        WoodenTool::IronPickaxe => "iron_pickaxe",
        WoodenTool::IronAxe => "iron_axe",
        WoodenTool::IronShovel => "iron_shovel",
        WoodenTool::IronHoe => "iron_hoe",
        WoodenTool::IronSword => "iron_sword",
    }
}

fn parse_tool_key(raw: &str) -> Option<WoodenTool> {
    let key = raw.trim().to_ascii_lowercase();
    match key.as_str() {
        "pickaxe" | "wooden_pickaxe" | "wp" => Some(WoodenTool::Pickaxe),
        "axe" | "wooden_axe" | "wa" => Some(WoodenTool::Axe),
        "shovel" | "wooden_shovel" | "ws" => Some(WoodenTool::Shovel),
        "hoe" | "wooden_hoe" | "wh" => Some(WoodenTool::Hoe),
        "sword" | "wood_sword" | "wooden_sword" | "sw" => Some(WoodenTool::Sword),
        "stone_pickaxe" | "sp" => Some(WoodenTool::StonePickaxe),
        "stone_axe" | "sa" => Some(WoodenTool::StoneAxe),
        "stone_shovel" | "ss" => Some(WoodenTool::StoneShovel),
        "stone_hoe" | "sh" => Some(WoodenTool::StoneHoe),
        "stone_sword" | "sx" => Some(WoodenTool::StoneSword),
        "iron_pickaxe" | "ip" => Some(WoodenTool::IronPickaxe),
        "iron_axe" | "ia" => Some(WoodenTool::IronAxe),
        "iron_shovel" | "is" => Some(WoodenTool::IronShovel),
        "iron_hoe" | "ih" => Some(WoodenTool::IronHoe),
        "iron_sword" | "ix" => Some(WoodenTool::IronSword),
        _ => None,
    }
}

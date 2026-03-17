use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::inventory::WoodenTool;
use crate::paths;
use crate::world::block::Block;

const DEFAULT_WOOD_TOOL_DURABILITY: u16 = 170;
const DEFAULT_STONE_TOOL_DURABILITY: u16 = 320;
const DEFAULT_IRON_TOOL_DURABILITY: u16 = 620;
const FALLBACK_TOOL_ID_BASE: u16 = 1000;

#[derive(Clone, Debug, Deserialize)]
struct ItemConfigFile {
    items: Vec<ItemConfigEntry>,
}

#[derive(Clone, Debug, Deserialize)]
struct ItemConfigEntry {
    key: String,
    id: u16,
    #[serde(default)]
    texture: Option<String>,
    #[serde(default)]
    break_seconds: Option<f32>,
    #[serde(default)]
    durability: Option<u16>,
    #[serde(default)]
    multipliers: HashMap<String, f32>,
}

#[derive(Clone, Debug)]
struct ItemEntry {
    id: u16,
    texture: Option<String>,
    break_seconds: Option<f32>,
    durability: Option<u16>,
    multipliers: HashMap<String, f32>,
}

pub struct ItemRegistry {
    by_key: HashMap<String, ItemEntry>,
    by_id: HashMap<u16, String>,
}

static ITEM_REGISTRY: OnceLock<ItemRegistry> = OnceLock::new();

impl ItemRegistry {
    fn load() -> Self {
        if let Some(registry) = Self::load_from_json() {
            return registry;
        }
        log::warn!("Item registry JSON is missing or invalid, using built-in defaults");
        Self::from_entries(default_entries())
    }

    fn load_from_json() -> Option<Self> {
        let path = items_json_path()?;
        let text = fs::read_to_string(&path).ok()?;
        let parsed: ItemConfigFile = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(err) => {
                log::warn!("Failed to parse item registry {:?}: {}", path, err);
                return None;
            }
        };

        let entries = parsed
            .items
            .into_iter()
            .map(|e| {
                (
                    e.key,
                    ItemEntry {
                        id: e.id,
                        texture: e.texture,
                        break_seconds: e.break_seconds,
                        durability: e.durability,
                        multipliers: e.multipliers,
                    },
                )
            })
            .collect::<Vec<_>>();

        if entries.is_empty() {
            log::warn!("Item registry {:?} has zero items", path);
            return None;
        }

        let mut by_key = HashMap::new();
        let mut by_id = HashMap::new();
        for (key, entry) in entries {
            if by_key.contains_key(&key) {
                log::warn!("Duplicate item key '{}' in {:?}", key, path);
                continue;
            }
            if by_id.contains_key(&entry.id) {
                log::warn!("Duplicate item id {} in {:?}", entry.id, path);
                continue;
            }
            by_id.insert(entry.id, key.clone());
            by_key.insert(key, entry);
        }

        if by_key.is_empty() {
            return None;
        }

        Some(Self { by_key, by_id })
    }

    fn from_entries(entries: Vec<(String, ItemEntry)>) -> Self {
        let mut by_key = HashMap::new();
        let mut by_id = HashMap::new();
        for (key, entry) in entries {
            if by_key.contains_key(&key) || by_id.contains_key(&entry.id) {
                continue;
            }
            by_id.insert(entry.id, key.clone());
            by_key.insert(key, entry);
        }
        Self { by_key, by_id }
    }

    fn item_id_for_key(&self, key: &str) -> Option<u16> {
        self.by_key.get(key).map(|e| e.id)
    }

    fn block_break_seconds(&self, block: Block) -> f32 {
        self.by_key
            .get(block.item_key())
            .and_then(|e| e.break_seconds)
            .unwrap_or_else(|| default_block_break_seconds(block))
    }

    fn tool_max_durability(&self, tool: WoodenTool) -> u16 {
        self.by_key
            .get(tool_item_key(tool))
            .and_then(|e| e.durability)
            .unwrap_or_else(|| default_tool_durability(tool))
    }

    fn tool_break_multiplier(&self, tool: WoodenTool, block: Block) -> f32 {
        let Some(tool_entry) = self.by_key.get(tool_item_key(tool)) else {
            return default_tool_break_multiplier(tool, block);
        };
        if let Some(v) = tool_entry.multipliers.get(block.item_key()) {
            return (*v).max(0.1);
        }
        if let Some(v) = tool_entry.multipliers.get("default") {
            return (*v).max(0.1);
        }
        default_tool_break_multiplier(tool, block)
    }

    fn item_texture_rel_path(&self, key: &str) -> Option<&str> {
        self.by_key
            .get(key)
            .and_then(|e| e.texture.as_deref())
            .filter(|p| !p.trim().is_empty())
    }
}

pub fn global() -> &'static ItemRegistry {
    ITEM_REGISTRY.get_or_init(ItemRegistry::load)
}

pub fn block_item_id(block: Block) -> u16 {
    global()
        .item_id_for_key(block.item_key())
        .unwrap_or(block.id() as u16)
}

pub fn tool_item_id(tool: WoodenTool) -> u16 {
    global()
        .item_id_for_key(tool_item_key(tool))
        .unwrap_or(FALLBACK_TOOL_ID_BASE + tool.idx() as u16)
}

pub fn item_key_from_id(id: u16) -> Option<&'static str> {
    global().by_id.get(&id).map(|s| s.as_str())
}

pub fn block_texture_path(block: Block) -> Option<PathBuf> {
    let rel = global().item_texture_rel_path(block.item_key())?;
    resolve_item_texture_path(rel)
}

pub fn tool_texture_path(tool: WoodenTool) -> Option<PathBuf> {
    let rel = global().item_texture_rel_path(tool_item_key(tool))?;
    resolve_item_texture_path(rel)
}

pub fn tool_max_durability(tool: WoodenTool) -> u16 {
    global().tool_max_durability(tool)
}

pub fn block_break_seconds(block: Block, tool: Option<WoodenTool>) -> f32 {
    let base = global().block_break_seconds(block);
    if !base.is_finite() || base <= 0.0 {
        return base;
    }
    let speed_mult = tool.map_or(1.0, |t| global().tool_break_multiplier(t, block));
    (base / speed_mult.max(0.1)).max(0.05)
}

fn items_json_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for base in paths::base_roots() {
        candidates.push(base.join("src").join("assets").join("items").join("items.json"));
        candidates.push(base.join("assets").join("items").join("items.json"));
    }
    candidates.into_iter().find(|p| p.exists())
}

fn resolve_item_texture_path(rel_or_abs: &str) -> Option<PathBuf> {
    let normalized = rel_or_abs.replace('\\', "/");
    let candidate = PathBuf::from(&normalized);
    if candidate.is_absolute() {
        return candidate.exists().then_some(candidate);
    }

    let rel = Path::new(&normalized);
    let mut candidates = Vec::new();
    for base in paths::base_roots() {
        candidates.push(base.join("src").join("assets").join(rel));
        candidates.push(base.join("assets").join(rel));
        candidates.push(base.join(rel));
    }
    candidates.into_iter().find(|p| p.exists())
}

fn tool_item_key(tool: WoodenTool) -> &'static str {
    match tool {
        WoodenTool::Pickaxe => "wooden_pickaxe",
        WoodenTool::Axe => "wooden_axe",
        WoodenTool::Shovel => "wooden_shovel",
        WoodenTool::Hoe => "wooden_hoe",
        WoodenTool::Sword => "wood_sword",
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

fn default_block_break_seconds(block: Block) -> f32 {
    match block {
        Block::Air
        | Block::CaveAir
        | Block::Water
        | Block::Stick
        | Block::Coal
        | Block::IronIngot => 0.0,
        Block::Torch => 0.2,
        Block::Bedrock => f32::INFINITY,
        Block::Dirt
        | Block::Grass
        | Block::FarmlandDry
        | Block::FarmlandWet
        | Block::Sand
        | Block::Leaves => 1.0,
        Block::Log | Block::LogBottom | Block::Workbench | Block::Wood => 3.0,
        Block::Stone | Block::CoalOre | Block::IronOre | Block::CopperOre | Block::Furnace => 2.2,
    }
}

fn default_tool_break_multiplier(tool: WoodenTool, block: Block) -> f32 {
    match tool {
        WoodenTool::Pickaxe => match block {
            Block::Stone | Block::CoalOre | Block::IronOre | Block::CopperOre | Block::Furnace => 2.0,
            _ => 1.0,
        },
        WoodenTool::Axe => match block {
            Block::Log | Block::LogBottom | Block::Leaves | Block::Workbench | Block::Wood => 2.1,
            _ => 1.0,
        },
        WoodenTool::Shovel => match block {
            Block::Dirt | Block::Grass | Block::FarmlandDry | Block::FarmlandWet | Block::Sand => {
                2.0
            }
            _ => 1.0,
        },
        WoodenTool::Hoe => 1.0,
        WoodenTool::Sword => 1.0,
        WoodenTool::StonePickaxe => match block {
            Block::Stone | Block::CoalOre | Block::IronOre | Block::CopperOre | Block::Furnace => 3.1,
            _ => 1.0,
        },
        WoodenTool::StoneAxe => match block {
            Block::Log | Block::LogBottom | Block::Leaves | Block::Workbench | Block::Wood => 2.8,
            _ => 1.0,
        },
        WoodenTool::StoneShovel => match block {
            Block::Dirt | Block::Grass | Block::FarmlandDry | Block::FarmlandWet | Block::Sand => {
                2.6
            }
            _ => 1.0,
        },
        WoodenTool::StoneHoe => 1.0,
        WoodenTool::StoneSword => 1.0,
        WoodenTool::IronPickaxe => match block {
            Block::Stone | Block::CoalOre | Block::IronOre | Block::CopperOre | Block::Furnace => 4.2,
            _ => 1.0,
        },
        WoodenTool::IronAxe => match block {
            Block::Log | Block::LogBottom | Block::Leaves | Block::Workbench | Block::Wood => 3.6,
            _ => 1.0,
        },
        WoodenTool::IronShovel => match block {
            Block::Dirt | Block::Grass | Block::FarmlandDry | Block::FarmlandWet | Block::Sand => {
                3.3
            }
            _ => 1.0,
        },
        WoodenTool::IronHoe => 1.0,
        WoodenTool::IronSword => 1.0,
    }
}

fn default_tool_durability(tool: WoodenTool) -> u16 {
    match tool {
        WoodenTool::Pickaxe
        | WoodenTool::Axe
        | WoodenTool::Shovel
        | WoodenTool::Hoe
        | WoodenTool::Sword => DEFAULT_WOOD_TOOL_DURABILITY,
        WoodenTool::StonePickaxe
        | WoodenTool::StoneAxe
        | WoodenTool::StoneShovel
        | WoodenTool::StoneHoe
        | WoodenTool::StoneSword => DEFAULT_STONE_TOOL_DURABILITY,
        WoodenTool::IronPickaxe
        | WoodenTool::IronAxe
        | WoodenTool::IronShovel
        | WoodenTool::IronHoe
        | WoodenTool::IronSword => DEFAULT_IRON_TOOL_DURABILITY,
    }
}

fn default_entries() -> Vec<(String, ItemEntry)> {
    let mut out = Vec::new();
    let blocks = [
        (Block::Air, 0u16),
        (Block::Grass, 1),
        (Block::Dirt, 2),
        (Block::Stone, 3),
        (Block::Sand, 4),
        (Block::Water, 5),
        (Block::Bedrock, 6),
        (Block::Log, 7),
        (Block::Leaves, 8),
        (Block::LogBottom, 9),
        (Block::CoalOre, 10),
        (Block::IronOre, 11),
        (Block::CopperOre, 12),
        (Block::FarmlandDry, 13),
        (Block::FarmlandWet, 14),
        (Block::CaveAir, 15),
        (Block::Workbench, 16),
        (Block::Wood, 17),
        (Block::Stick, 18),
        (Block::Furnace, 19),
        (Block::Coal, 20),
        (Block::Torch, 21),
        (Block::IronIngot, 22),
    ];
    for (block, id) in blocks {
        out.push((
            block.item_key().to_string(),
            ItemEntry {
                id,
                texture: None,
                break_seconds: Some(default_block_break_seconds(block)),
                durability: None,
                multipliers: HashMap::new(),
            },
        ));
    }

    out.push(tool_entry(
        "wooden_pickaxe",
        1001,
        DEFAULT_WOOD_TOOL_DURABILITY,
        &[("default", 1.0), ("stone", 2.0), ("coal_ore", 2.0), ("iron_ore", 2.0), ("copper_ore", 2.0)],
    ));
    out.push(tool_entry(
        "wooden_axe",
        1002,
        DEFAULT_WOOD_TOOL_DURABILITY,
        &[("default", 1.0), ("log", 2.1), ("log_bottom", 2.1), ("leaves", 2.1), ("workbench", 2.1), ("wood", 2.1)],
    ));
    out.push(tool_entry(
        "wooden_shovel",
        1003,
        DEFAULT_WOOD_TOOL_DURABILITY,
        &[("default", 1.0), ("dirt", 2.0), ("grass", 2.0), ("farmland_dry", 2.0), ("farmland_wet", 2.0), ("sand", 2.0)],
    ));
    out.push(tool_entry(
        "wooden_hoe",
        1004,
        DEFAULT_WOOD_TOOL_DURABILITY,
        &[("default", 1.0)],
    ));
    out.push(tool_entry(
        "wood_sword",
        1005,
        DEFAULT_WOOD_TOOL_DURABILITY,
        &[("default", 1.0)],
    ));
    out.push(tool_entry(
        "stone_pickaxe",
        1010,
        DEFAULT_STONE_TOOL_DURABILITY,
        &[("default", 1.0), ("stone", 3.1), ("coal_ore", 3.1), ("iron_ore", 3.1), ("copper_ore", 3.1)],
    ));
    out.push(tool_entry(
        "stone_axe",
        1014,
        DEFAULT_STONE_TOOL_DURABILITY,
        &[("default", 1.0), ("log", 2.8), ("log_bottom", 2.8), ("leaves", 2.8), ("workbench", 2.8), ("wood", 2.8)],
    ));
    out.push(tool_entry(
        "stone_shovel",
        1018,
        DEFAULT_STONE_TOOL_DURABILITY,
        &[("default", 1.0), ("dirt", 2.6), ("grass", 2.6), ("farmland_dry", 2.6), ("farmland_wet", 2.6), ("sand", 2.6)],
    ));
    out.push(tool_entry(
        "stone_hoe",
        1019,
        DEFAULT_STONE_TOOL_DURABILITY,
        &[("default", 1.0)],
    ));
    out.push(tool_entry(
        "stone_sword",
        1006,
        DEFAULT_STONE_TOOL_DURABILITY,
        &[("default", 1.0)],
    ));
    out.push(tool_entry(
        "iron_pickaxe",
        1011,
        DEFAULT_IRON_TOOL_DURABILITY,
        &[("default", 1.0), ("stone", 4.2), ("coal_ore", 4.2), ("iron_ore", 4.2), ("copper_ore", 4.2)],
    ));
    out.push(tool_entry(
        "iron_axe",
        1015,
        DEFAULT_IRON_TOOL_DURABILITY,
        &[("default", 1.0), ("log", 3.6), ("log_bottom", 3.6), ("leaves", 3.6), ("workbench", 3.6), ("wood", 3.6)],
    ));
    out.push(tool_entry(
        "iron_shovel",
        1020,
        DEFAULT_IRON_TOOL_DURABILITY,
        &[("default", 1.0), ("dirt", 3.3), ("grass", 3.3), ("farmland_dry", 3.3), ("farmland_wet", 3.3), ("sand", 3.3)],
    ));
    out.push(tool_entry(
        "iron_hoe",
        1021,
        DEFAULT_IRON_TOOL_DURABILITY,
        &[("default", 1.0)],
    ));
    out.push(tool_entry(
        "iron_sword",
        1007,
        DEFAULT_IRON_TOOL_DURABILITY,
        &[("default", 1.0)],
    ));

    out
}

fn tool_entry(
    key: &str,
    id: u16,
    durability: u16,
    multipliers: &[(&str, f32)],
) -> (String, ItemEntry) {
    let mut map = HashMap::new();
    for (name, value) in multipliers {
        map.insert((*name).to_string(), *value);
    }
    (
        key.to_string(),
        ItemEntry {
            id,
            texture: None,
            break_seconds: None,
            durability: Some(durability),
            multipliers: map,
        },
    )
}

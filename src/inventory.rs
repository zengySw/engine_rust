use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use egui::{self, Align2, Color32, FontId, RichText, Stroke};

use crate::item_registry;
use crate::paths;
use crate::world::block::Block;

const HOTBAR_SLOTS: usize = 9;
const GRID_SLOTS: usize = 27;
const PLAYER_CRAFT_COLS: usize = 2;
const PLAYER_CRAFT_ROWS: usize = 2;
const WORKBENCH_CRAFT_COLS: usize = 3;
const WORKBENCH_CRAFT_ROWS: usize = 3;
const PLAYER_CRAFT_SLOTS: usize = PLAYER_CRAFT_COLS * PLAYER_CRAFT_ROWS;
const WORKBENCH_CRAFT_SLOTS: usize = WORKBENCH_CRAFT_COLS * WORKBENCH_CRAFT_ROWS;
const MAX_STACK_SIZE: u16 = 64;
const FURNACE_SMELT_SECONDS: f32 = 1.0;
const CRAFT_SECTION_GAP: f32 = 8.0;
const CRAFT_RESULT_GAP: f32 = 4.0;
const CRAFT_ARROW_SIZE: f32 = 30.0;
const FURNACE_COLUMN_GAP: f32 = 8.0;

#[derive(Clone, Copy)]
pub struct Slot {
    pub block: Block,
    pub tool: Option<WoodenTool>,
    pub count: u16,
}

impl Slot {
    pub fn empty() -> Self {
        Self {
            block: Block::Air,
            tool: None,
            count: 0,
        }
    }

    pub fn new(block: Block, count: u16) -> Self {
        if block.is_air() || count == 0 {
            return Self::empty();
        }
        Self {
            block,
            tool: None,
            count: count.min(MAX_STACK_SIZE),
        }
    }

    pub fn new_tool(tool: WoodenTool) -> Self {
        Self {
            block: Block::Air,
            tool: Some(tool),
            count: 1,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0 || (self.block.is_air() && self.tool.is_none())
    }

    pub fn is_tool(&self) -> bool {
        !self.is_empty() && self.tool.is_some()
    }

    fn clear(&mut self) {
        *self = Self::empty();
    }

    fn available_space(&self) -> u16 {
        if self.is_empty() {
            MAX_STACK_SIZE
        } else if self.is_tool() {
            0
        } else {
            MAX_STACK_SIZE.saturating_sub(self.count)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SlotId {
    Hotbar(usize),
    Grid(usize),
    Craft(usize),
    FurnaceInput,
    FurnaceFuel,
    FurnaceOutput,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InventoryMode {
    Player,
    Workbench,
    Furnace,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragDistributeButton {
    Primary,
    Secondary,
}

struct DragDistributeState {
    button: DragDistributeButton,
    touched_slots: Vec<SlotId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WoodenTool {
    Pickaxe,
    Axe,
    Shovel,
    Hoe,
    Sword,
    StonePickaxe,
    StoneAxe,
    StoneShovel,
    StoneHoe,
    StoneSword,
    IronPickaxe,
    IronAxe,
    IronShovel,
    IronHoe,
    IronSword,
}

impl WoodenTool {
    pub fn idx(self) -> usize {
        match self {
            WoodenTool::Pickaxe => 0,
            WoodenTool::Axe => 1,
            WoodenTool::Shovel => 2,
            WoodenTool::Hoe => 3,
            WoodenTool::Sword => 4,
            WoodenTool::StonePickaxe => 5,
            WoodenTool::StoneAxe => 6,
            WoodenTool::StoneShovel => 7,
            WoodenTool::StoneHoe => 8,
            WoodenTool::StoneSword => 9,
            WoodenTool::IronPickaxe => 10,
            WoodenTool::IronAxe => 11,
            WoodenTool::IronShovel => 12,
            WoodenTool::IronHoe => 13,
            WoodenTool::IronSword => 14,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            WoodenTool::Pickaxe => "Wooden Pickaxe",
            WoodenTool::Axe => "Wooden Axe",
            WoodenTool::Shovel => "Wooden Shovel",
            WoodenTool::Hoe => "Wooden Hoe",
            WoodenTool::Sword => "Wooden Sword",
            WoodenTool::StonePickaxe => "Stone Pickaxe",
            WoodenTool::StoneAxe => "Stone Axe",
            WoodenTool::StoneShovel => "Stone Shovel",
            WoodenTool::StoneHoe => "Stone Hoe",
            WoodenTool::StoneSword => "Stone Sword",
            WoodenTool::IronPickaxe => "Iron Pickaxe",
            WoodenTool::IronAxe => "Iron Axe",
            WoodenTool::IronShovel => "Iron Shovel",
            WoodenTool::IronHoe => "Iron Hoe",
            WoodenTool::IronSword => "Iron Sword",
        }
    }

    pub fn is_hoe(self) -> bool {
        matches!(self, WoodenTool::Hoe | WoodenTool::StoneHoe | WoodenTool::IronHoe)
    }

    pub fn is_sword(self) -> bool {
        matches!(self, WoodenTool::Sword | WoodenTool::StoneSword | WoodenTool::IronSword)
    }
}

pub struct Inventory {
    pub open: bool,
    pub selected: usize,
    pub hotbar: [Slot; HOTBAR_SLOTS],
    pub grid: [Slot; GRID_SLOTS],
    craft_player: [Slot; PLAYER_CRAFT_SLOTS],
    craft_workbench: [Slot; WORKBENCH_CRAFT_SLOTS],
    mode: InventoryMode,
    furnace_input: Slot,
    furnace_fuel: Slot,
    furnace_output: Slot,
    furnace_burn_time_left: f32,
    furnace_burn_time_total: f32,
    furnace_smelt_progress: f32,
    furnace_active_input: Option<Block>,
    carried: Slot,
    crafted_tools: Vec<WoodenTool>,
    texture_cache: HashMap<u8, egui::TextureHandle>,
    texture_missing: HashSet<u8>,
    tool_texture_cache: HashMap<WoodenTool, egui::TextureHandle>,
    tool_texture_missing: HashSet<WoodenTool>,
    tool_durability_values: [u16; 15],
    drag_distribute: Option<DragDistributeState>,
}

impl Inventory {
    pub fn new() -> Self {
        Self {
            open: false,
            selected: 0,
            hotbar: [Slot::empty(); HOTBAR_SLOTS],
            grid: [Slot::empty(); GRID_SLOTS],
            craft_player: [Slot::empty(); PLAYER_CRAFT_SLOTS],
            craft_workbench: [Slot::empty(); WORKBENCH_CRAFT_SLOTS],
            mode: InventoryMode::Player,
            furnace_input: Slot::empty(),
            furnace_fuel: Slot::empty(),
            furnace_output: Slot::empty(),
            furnace_burn_time_left: 0.0,
            furnace_burn_time_total: 0.0,
            furnace_smelt_progress: 0.0,
            furnace_active_input: None,
            carried: Slot::empty(),
            crafted_tools: Vec::new(),
            texture_cache: HashMap::new(),
            texture_missing: HashSet::new(),
            tool_texture_cache: HashMap::new(),
            tool_texture_missing: HashSet::new(),
            tool_durability_values: [0; 15],
            drag_distribute: None,
        }
    }

    pub fn set_tool_durability_values(&mut self, values: [u16; 15]) {
        self.tool_durability_values = values;
    }

    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open_player();
        }
    }

    pub fn open_player(&mut self) {
        self.mode = InventoryMode::Player;
        self.open = true;
    }

    pub fn open_workbench(&mut self) {
        self.mode = InventoryMode::Workbench;
        self.open = true;
    }

    pub fn open_furnace(&mut self) {
        self.mode = InventoryMode::Furnace;
        self.open = true;
    }

    pub fn close(&mut self) {
        self.return_temporary_slots_to_storage();
        self.open = false;
        self.mode = InventoryMode::Player;
        self.drag_distribute = None;
    }

    pub fn selected_block(&self) -> Option<Block> {
        let slot = self.hotbar[self.selected];
        if slot.is_empty() || slot.is_tool() || !slot.block.is_placeable() {
            None
        } else {
            Some(slot.block)
        }
    }

    pub fn selected_tool(&self) -> Option<WoodenTool> {
        let slot = self.hotbar[self.selected];
        if slot.is_tool() {
            slot.tool
        } else {
            None
        }
    }

    pub fn consume_selected_one(&mut self) -> Option<Block> {
        let slot = &mut self.hotbar[self.selected];
        if slot.is_empty() || slot.is_tool() {
            return None;
        }
        let block = slot.block;
        slot.count = slot.count.saturating_sub(1);
        if slot.count == 0 {
            slot.clear();
        }
        Some(block)
    }

    pub fn add_block(&mut self, block: Block, count: u16) -> u16 {
        if block.is_air() || count == 0 {
            return count;
        }

        let mut remaining = count;

        for slot in self.hotbar.iter_mut().chain(self.grid.iter_mut()) {
            if slot.is_empty() || slot.block != block || slot.count >= MAX_STACK_SIZE {
                continue;
            }
            let add = remaining.min(slot.available_space());
            slot.count += add;
            remaining -= add;
            if remaining == 0 {
                return 0;
            }
        }

        for slot in self.hotbar.iter_mut().chain(self.grid.iter_mut()) {
            if !slot.is_empty() {
                continue;
            }
            let add = remaining.min(MAX_STACK_SIZE);
            *slot = Slot::new(block, add);
            remaining -= add;
            if remaining == 0 {
                return 0;
            }
        }

        remaining
    }

    pub fn add_tool(&mut self, tool: WoodenTool) -> bool {
        for slot in self.hotbar.iter_mut().chain(self.grid.iter_mut()) {
            if slot.is_empty() {
                *slot = Slot::new_tool(tool);
                return true;
            }
        }
        false
    }

    pub fn has_tool(&self, tool: WoodenTool) -> bool {
        self.hotbar
            .iter()
            .chain(self.grid.iter())
            .any(|slot| slot.tool == Some(tool) && !slot.is_empty())
    }

    pub fn remove_one_tool(&mut self, tool: WoodenTool) -> bool {
        for slot in self.hotbar.iter_mut().chain(self.grid.iter_mut()) {
            if slot.tool == Some(tool) && !slot.is_empty() {
                slot.clear();
                return true;
            }
        }
        false
    }

    #[allow(dead_code)]
    pub fn sort_storage(&mut self) {
        let mut blocks: Vec<(Block, u16)> = Vec::new();
        let mut tools: Vec<WoodenTool> = Vec::new();

        for slot in self.hotbar.iter().chain(self.grid.iter()) {
            if slot.is_empty() {
                continue;
            }
            if let Some(tool) = slot.tool {
                tools.push(tool);
            } else {
                if let Some((_, count)) = blocks.iter_mut().find(|(block, _)| *block == slot.block) {
                    *count = count.saturating_add(slot.count);
                } else {
                    blocks.push((slot.block, slot.count));
                }
            }
        }

        for slot in self.hotbar.iter_mut().chain(self.grid.iter_mut()) {
            slot.clear();
        }

        blocks.sort_by_key(|(block, _)| item_registry::block_item_id(*block));
        tools.sort_by_key(|tool| tool.idx());

        let mut packed: Vec<Slot> = Vec::new();
        for (block, mut count) in blocks {
            while count > 0 {
                let chunk = count.min(MAX_STACK_SIZE);
                packed.push(Slot::new(block, chunk));
                count -= chunk;
            }
        }
        for tool in tools {
            packed.push(Slot::new_tool(tool));
        }

        for (idx, slot) in packed.into_iter().enumerate() {
            if idx < HOTBAR_SLOTS {
                self.hotbar[idx] = slot;
            } else {
                let grid_idx = idx - HOTBAR_SLOTS;
                if grid_idx < GRID_SLOTS {
                    self.grid[grid_idx] = slot;
                } else {
                    break;
                }
            }
        }
    }

    pub fn quick_craft(&mut self) -> Option<&'static str> {
        const RECIPES: [CraftRecipe; 3] = [
            CraftRecipe {
                name: "Compacted Stone",
                inputs: &[(Block::Dirt, 4)],
                output: (Block::Stone, 1),
            },
            CraftRecipe {
                name: "Packed Stone",
                inputs: &[(Block::Sand, 4)],
                output: (Block::Stone, 1),
            },
            CraftRecipe {
                name: "Charcoal Mix",
                inputs: &[(Block::Log, 2), (Block::Stone, 1)],
                output: (Block::Coal, 2),
            },
        ];

        for recipe in RECIPES {
            if !self.can_craft(recipe) {
                continue;
            }
            for &(block, count) in recipe.inputs {
                self.remove_block_amount(block, count);
            }
            let _ = self.add_block(recipe.output.0, recipe.output.1);
            return Some(recipe.name);
        }

        None
    }

    pub fn count_block(&self, block: Block) -> u16 {
        self.total_count(block)
    }

    pub fn consume_blocks(&mut self, block: Block, amount: u16) -> bool {
        self.remove_block_amount(block, amount)
    }

    pub fn can_add_block_exact(&self, block: Block, amount: u16) -> bool {
        self.can_add_exact(block, amount)
    }

    pub fn take_crafted_tool(&mut self) -> Option<WoodenTool> {
        self.crafted_tools.pop()
    }

    pub fn select_hotbar_slot(&mut self, index: usize) {
        self.selected = index.min(self.hotbar.len().saturating_sub(1));
    }

    pub fn cycle_hotbar(&mut self, delta: i32) {
        let len = self.hotbar.len() as i32;
        if len <= 0 {
            return;
        }
        let next = (self.selected as i32 + delta).rem_euclid(len) as usize;
        self.selected = next;
    }

    pub fn tick_furnace(&mut self, dt: f32) {
        if dt <= f32::EPSILON {
            return;
        }

        let input_now = self.current_furnace_recipe().map(|r| r.input);
        if input_now != self.furnace_active_input {
            self.furnace_active_input = input_now;
            self.furnace_smelt_progress = 0.0;
        }

        let mut remaining_time = dt;
        while remaining_time > 0.0 {
            let Some(recipe) = self.current_furnace_recipe() else {
                self.cool_furnace_progress(remaining_time);
                return;
            };
            if !self.can_accept_furnace_output(recipe.output, recipe.output_count) {
                self.cool_furnace_progress(remaining_time);
                return;
            }

            if self.furnace_burn_time_left <= 0.0 && !self.try_consume_furnace_fuel() {
                self.cool_furnace_progress(remaining_time);
                return;
            }

            let step = remaining_time.min(self.furnace_burn_time_left.max(0.0));
            if step <= f32::EPSILON {
                break;
            }
            self.furnace_burn_time_left = (self.furnace_burn_time_left - step).max(0.0);
            self.furnace_smelt_progress += step;
            remaining_time -= step;

            while self.furnace_smelt_progress >= FURNACE_SMELT_SECONDS {
                let Some(active) = self.current_furnace_recipe() else {
                    self.furnace_smelt_progress = 0.0;
                    self.furnace_active_input = None;
                    break;
                };
                if !self.can_accept_furnace_output(active.output, active.output_count) {
                    self.furnace_smelt_progress = FURNACE_SMELT_SECONDS;
                    break;
                }
                if !consume_block_from_slot(&mut self.furnace_input, active.input, 1) {
                    self.furnace_smelt_progress = 0.0;
                    break;
                }
                let _ = self.push_furnace_output(active.output, active.output_count);
                self.furnace_smelt_progress -= FURNACE_SMELT_SECONDS;
                self.furnace_active_input = self.current_furnace_recipe().map(|r| r.input);
            }
        }

        if self.furnace_burn_time_left <= 0.0 {
            self.furnace_burn_time_left = 0.0;
            self.furnace_burn_time_total = 0.0;
        }
        self.furnace_smelt_progress = self.furnace_smelt_progress.clamp(0.0, FURNACE_SMELT_SECONDS);
    }

    fn current_furnace_recipe(&self) -> Option<FurnaceRecipe> {
        if self.furnace_input.is_empty() || self.furnace_input.is_tool() {
            return None;
        }
        furnace_recipe_for(self.furnace_input.block)
    }

    fn can_accept_furnace_output(&self, block: Block, count: u16) -> bool {
        if self.furnace_output.is_empty() {
            return count <= MAX_STACK_SIZE;
        }
        !self.furnace_output.is_tool()
            && self.furnace_output.block == block
            && self.furnace_output.available_space() >= count
    }

    fn push_furnace_output(&mut self, block: Block, count: u16) -> bool {
        if !self.can_accept_furnace_output(block, count) {
            return false;
        }
        if self.furnace_output.is_empty() {
            self.furnace_output = Slot::new(block, count);
            return true;
        }
        self.furnace_output.count = self.furnace_output.count.saturating_add(count).min(MAX_STACK_SIZE);
        true
    }

    fn try_consume_furnace_fuel(&mut self) -> bool {
        if self.furnace_fuel.is_empty() || self.furnace_fuel.is_tool() {
            return false;
        }
        let burn_amount = furnace_fuel_smelts(self.furnace_fuel.block) * FURNACE_SMELT_SECONDS;
        if burn_amount <= 0.0 {
            return false;
        }
        self.furnace_fuel.count = self.furnace_fuel.count.saturating_sub(1);
        if self.furnace_fuel.count == 0 {
            self.furnace_fuel.clear();
        }
        self.furnace_burn_time_left = burn_amount;
        self.furnace_burn_time_total = burn_amount;
        true
    }

    fn cool_furnace_progress(&mut self, dt: f32) {
        self.furnace_smelt_progress = (self.furnace_smelt_progress - dt * 0.30).max(0.0);
    }

    fn furnace_burn_progress(&self) -> f32 {
        if self.furnace_burn_time_total <= f32::EPSILON {
            0.0
        } else {
            (self.furnace_burn_time_left / self.furnace_burn_time_total).clamp(0.0, 1.0)
        }
    }

    fn furnace_smelt_progress_ratio(&self) -> f32 {
        (self.furnace_smelt_progress / FURNACE_SMELT_SECONDS).clamp(0.0, 1.0)
    }

    fn craft_dims(&self) -> (usize, usize) {
        if self.mode == InventoryMode::Workbench {
            (WORKBENCH_CRAFT_COLS, WORKBENCH_CRAFT_ROWS)
        } else {
            (PLAYER_CRAFT_COLS, PLAYER_CRAFT_ROWS)
        }
    }

    fn craft_slot(&self, idx: usize) -> Slot {
        if self.mode == InventoryMode::Workbench {
            self.craft_workbench[idx]
        } else {
            self.craft_player[idx]
        }
    }

    fn craft_slot_mut(&mut self, idx: usize) -> &mut Slot {
        if self.mode == InventoryMode::Workbench {
            &mut self.craft_workbench[idx]
        } else {
            &mut self.craft_player[idx]
        }
    }

    pub fn draw_hotbar(&mut self, ctx: &egui::Context) {
        if self.open {
            return;
        }

        let screen = ctx.screen_rect();
        let slot_size = 34.0;
        let spacing = 2.0;
        let total_w = slot_size * HOTBAR_SLOTS as f32 + spacing * (HOTBAR_SLOTS as f32 - 1.0) + 16.0;
        let total_h = slot_size + 16.0;
        let pos = egui::pos2(
            screen.center().x - total_w * 0.5,
            screen.bottom() - total_h - 12.0,
        );

        egui::Area::new("hotbar".into())
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                let frame = egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(198, 198, 198, 220))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(56, 56, 56)))
                    .inner_margin(egui::Margin::same(8.0));

                frame.show(ui, |ui| {
                    let mut style = (**ui.style()).clone();
                    style.spacing.item_spacing = egui::vec2(spacing, 0.0);
                    ui.set_style(style);

                    ui.horizontal(|ui| {
                        for i in 0..HOTBAR_SLOTS {
                            let slot = self.hotbar[i];
                            let tex = self.slot_texture_id(ctx, slot);
                            let resp = paint_slot(ui, &slot, i == self.selected, slot_size, tex);
                            if resp.clicked() {
                                self.select_hotbar_slot(i);
                            }
                        }
                    });
                });
            });
    }

    pub fn draw(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }

        let screen = ctx.screen_rect();
        let bg_layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("inv_bg"));
        ctx.layer_painter(bg_layer)
            .rect_filled(screen, 0.0, Color32::from_black_alpha(76));

        let (panel_size, title) = match self.mode {
            InventoryMode::Workbench => (egui::vec2(470.0, 458.0), "Crafting"),
            InventoryMode::Furnace => (egui::vec2(452.0, 426.0), "Furnace"),
            InventoryMode::Player => (egui::vec2(500.0, 510.0), "Inventory"),
        };
        let panel_rect = egui::Rect::from_center_size(screen.center(), panel_size);

        egui::Area::new("inventory_panel".into())
            .order(egui::Order::Foreground)
            .fixed_pos(panel_rect.min)
            .show(ctx, |ui| {
                ui.set_min_size(panel_size);
                ui.set_max_size(panel_size);
                let frame = egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(198, 198, 198, 248))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(52, 52, 52)))
                    .inner_margin(egui::Margin::same(12.0));

                frame.show(ui, |ui| {
                    let mut style = (**ui.style()).clone();
                    style.spacing.item_spacing = egui::vec2(2.0, 2.0);
                    ui.set_style(style);
                    ui.set_width(panel_size.x - 24.0);

                    if self.mode == InventoryMode::Furnace {
                        ui.label(RichText::new(title).size(17.0).strong().color(Color32::from_rgb(52, 52, 52)));
                        ui.add_space(6.0);
                        self.draw_furnace_panel(ui, ctx);
                    } else {
                        let (craft_cols, craft_rows) = self.craft_dims();
                        let craft_slot = 38.0;
                        let craft_grid_h = craft_rows as f32 * craft_slot
                            + (craft_rows.saturating_sub(1) as f32) * 2.0;
                        ui.horizontal_top(|ui| {
                            if self.mode == InventoryMode::Player {
                                draw_player_preview_stub(ui, craft_slot);
                                ui.add_space(CRAFT_SECTION_GAP);
                            }

                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("Crafting")
                                        .size(16.0)
                                        .color(Color32::from_rgb(52, 52, 52)),
                                );
                                ui.add_space(2.0);
                                ui.horizontal_top(|ui| {
                                    ui.vertical(|ui| {
                                        for row in 0..craft_rows {
                                            ui.horizontal(|ui| {
                                                for col in 0..craft_cols {
                                                    let idx = row * craft_cols + col;
                                                    let slot = self.craft_slot(idx);
                                                    let tex = self.slot_texture_id(ctx, slot);
                                                    let resp = paint_slot(ui, &slot, false, craft_slot, tex);
                                                    self.handle_slot_interaction(SlotId::Craft(idx), &resp);
                                                }
                                            });
                                        }
                                    });

                                    ui.add_space(CRAFT_SECTION_GAP);
                                    ui.vertical(|ui| {
                                        ui.add_space(((craft_grid_h - CRAFT_ARROW_SIZE) * 0.5).max(0.0));
                                        ui.label(
                                            RichText::new("->")
                                                .size(CRAFT_ARROW_SIZE)
                                                .color(Color32::from_rgb(148, 148, 148)),
                                        );
                                    });
                                    ui.add_space(CRAFT_RESULT_GAP);

                                    ui.vertical(|ui| {
                                        ui.add_space(((craft_grid_h - craft_slot) * 0.5).max(0.0));
                                        let output = self.current_craft_output();
                                        let out_tex = output.and_then(|o| match o {
                                            CraftOutput::Block { block, .. } => {
                                                self.block_texture_id(ctx, block)
                                            }
                                            CraftOutput::Tool(tool) => self.tool_texture_id(ctx, tool),
                                        });
                                        let out_resp =
                                            paint_craft_result_slot(ui, output.as_ref(), craft_slot, out_tex);
                                        if out_resp.clicked_by(egui::PointerButton::Primary) {
                                            if let Some(out) = output {
                                                self.take_craft_result(out, false);
                                            }
                                        } else if out_resp.clicked_by(egui::PointerButton::Secondary) {
                                            if let Some(out) = output {
                                                self.take_craft_result(out, true);
                                            }
                                        }
                                    });
                                });
                            });
                        });
                    }

                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("Inventory")
                            .size(16.0)
                            .color(Color32::from_rgb(52, 52, 52)),
                    );
                    ui.add_space(2.0);
                    let inv_slot = 38.0;
                    for row in 0..3 {
                        ui.horizontal(|ui| {
                            for col in 0..9 {
                                let idx = row * 9 + col;
                                let slot = self.grid[idx];
                                let tex = self.slot_texture_id(ctx, slot);
                                let resp = paint_slot(ui, &slot, false, inv_slot, tex);
                                self.handle_slot_interaction(SlotId::Grid(idx), &resp);
                            }
                        });
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        for i in 0..HOTBAR_SLOTS {
                            let slot = self.hotbar[i];
                            let tex = self.slot_texture_id(ctx, slot);
                            let resp = paint_slot(ui, &slot, i == self.selected, inv_slot, tex);
                            self.handle_slot_interaction(SlotId::Hotbar(i), &resp);
                        }
                    });
                });
            });

        self.finish_drag_distribution_if_released(ctx);
        self.draw_carried(ctx);
    }

    fn draw_furnace_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let slot_size = 34.0;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Input").size(14.0).color(Color32::from_rgb(52, 52, 52)));
                let input_tex = self.slot_texture_id(ctx, self.furnace_input);
                let input_resp = paint_slot(ui, &self.furnace_input, false, slot_size, input_tex);
                self.handle_slot_interaction(SlotId::FurnaceInput, &input_resp);
                ui.add_space(4.0);
                ui.label(RichText::new("Fuel").size(14.0).color(Color32::from_rgb(52, 52, 52)));
                let fuel_tex = self.slot_texture_id(ctx, self.furnace_fuel);
                let fuel_resp = paint_slot(ui, &self.furnace_fuel, false, slot_size, fuel_tex);
                self.handle_slot_interaction(SlotId::FurnaceFuel, &fuel_resp);
            });

            ui.add_space(FURNACE_COLUMN_GAP);
            ui.vertical_centered(|ui| {
                ui.add_space(12.0);
                draw_meter(
                    ui,
                    self.furnace_smelt_progress_ratio(),
                    egui::vec2(100.0, 10.0),
                    Color32::from_rgb(214, 166, 70),
                );
                ui.add_space(6.0);
                ui.label(RichText::new("Smelting").size(11.0).color(Color32::from_rgb(80, 80, 80)));
                ui.add_space(8.0);
                draw_meter(
                    ui,
                    self.furnace_burn_progress(),
                    egui::vec2(74.0, 8.0),
                    Color32::from_rgb(232, 112, 58),
                );
                ui.add_space(4.0);
                ui.label(RichText::new("Fuel").size(11.0).color(Color32::from_rgb(80, 80, 80)));
            });

            ui.add_space(FURNACE_COLUMN_GAP);
            ui.vertical(|ui| {
                ui.label(RichText::new("Output").size(14.0).color(Color32::from_rgb(52, 52, 52)));
                let out_tex = self.slot_texture_id(ctx, self.furnace_output);
                let out_resp = paint_slot(ui, &self.furnace_output, false, slot_size, out_tex);
                self.handle_slot_interaction(SlotId::FurnaceOutput, &out_resp);
            });
        });
    }

    fn handle_slot_interaction(&mut self, slot_id: SlotId, resp: &egui::Response) {
        self.show_slot_hover_tooltip(slot_id, resp);
        self.handle_drag_hover(slot_id, resp);

        let primary_clicked = resp.clicked_by(egui::PointerButton::Primary);
        let secondary_clicked = resp.clicked_by(egui::PointerButton::Secondary);
        let primary_pressed = resp
            .ctx
            .input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
        let secondary_pressed = resp
            .ctx
            .input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary));
        let primary_down = resp.ctx.input(|i| i.pointer.primary_down());
        let secondary_down = resp.ctx.input(|i| i.pointer.secondary_down());
        let shift_down = resp.ctx.input(|i| i.modifiers.shift);

        if let SlotId::Hotbar(i) = slot_id {
            if primary_clicked || secondary_clicked {
                self.select_hotbar_slot(i);
            }
        }

        if !self.open {
            return;
        }

        let pointer_on_this_slot = resp.hovered() || resp.contains_pointer();

        if self.drag_distribute.is_none() && pointer_on_this_slot {
            if primary_pressed || primary_down {
                if self.try_start_drag_distribution(slot_id, DragDistributeButton::Primary, shift_down) {
                    return;
                }
            } else if secondary_pressed || secondary_down {
                if self.try_start_drag_distribution(slot_id, DragDistributeButton::Secondary, shift_down) {
                    return;
                }
            }
        }

        // While drag-distribution is active we only collect hovered slots.
        // Placement is finalized on button release in finish_drag_distribution_if_released().
        if self.drag_distribute.is_some() {
            return;
        }

        if primary_clicked && shift_down {
            if self.shift_click_slot(slot_id) {
                return;
            }
        }

        if primary_clicked {
            self.primary_click_slot(slot_id);
        } else if secondary_clicked {
            self.secondary_click_slot(slot_id);
        }
    }

    fn try_start_drag_distribution(
        &mut self,
        slot_id: SlotId,
        button: DragDistributeButton,
        shift_down: bool,
    ) -> bool {
        if shift_down
            || slot_id == SlotId::FurnaceOutput
            || self.carried.is_empty()
            || self.carried.is_tool()
        {
            return false;
        }

        self.drag_distribute = Some(DragDistributeState {
            button,
            touched_slots: Vec::new(),
        });
        self.touch_drag_slot(slot_id);
        true
    }

    fn handle_drag_hover(&mut self, slot_id: SlotId, resp: &egui::Response) {
        if !(resp.hovered() || resp.contains_pointer()) {
            return;
        }
        let Some(state) = &self.drag_distribute else {
            return;
        };
        let button_down = resp.ctx.input(|i| match state.button {
            DragDistributeButton::Primary => i.pointer.primary_down(),
            DragDistributeButton::Secondary => i.pointer.secondary_down(),
        });
        if !button_down {
            return;
        }
        self.touch_drag_slot(slot_id);
    }

    fn touch_drag_slot(&mut self, slot_id: SlotId) {
        let Some(button) = self.drag_distribute.as_ref().map(|d| d.button) else {
            return;
        };
        if slot_id == SlotId::FurnaceOutput {
            return;
        }

        let already_touched = self
            .drag_distribute
            .as_ref()
            .is_some_and(|d| d.touched_slots.contains(&slot_id));
        if already_touched {
            return;
        }

        if let Some(drag) = &mut self.drag_distribute {
            drag.touched_slots.push(slot_id);
        }

        if button == DragDistributeButton::Secondary {
            self.place_one_from_carried(slot_id);
        }
    }

    fn finish_drag_distribution_if_released(&mut self, ctx: &egui::Context) {
        let Some(state) = self.drag_distribute.as_ref() else {
            return;
        };

        let still_down = ctx.input(|i| match state.button {
            DragDistributeButton::Primary => i.pointer.primary_down(),
            DragDistributeButton::Secondary => i.pointer.secondary_down(),
        });
        if still_down {
            return;
        }

        let Some(state) = self.drag_distribute.take() else {
            return;
        };
        if state.button == DragDistributeButton::Primary {
            self.distribute_carried_evenly(state.touched_slots);
        }
    }

    fn place_one_from_carried(&mut self, slot_id: SlotId) {
        if self.carried.is_empty() || self.carried.is_tool() || slot_id == SlotId::FurnaceOutput {
            return;
        }

        let one = Slot::new(self.carried.block, 1);
        if !self.can_place_in_slot(one, slot_id) {
            return;
        }

        let mut slot = self.slot(slot_id);
        if slot.is_empty() {
            slot = one;
        } else if !slot.is_tool()
            && slot.block == self.carried.block
            && slot.count < MAX_STACK_SIZE
        {
            slot.count += 1;
        } else {
            return;
        }

        self.carried.count = self.carried.count.saturating_sub(1);
        if self.carried.count == 0 {
            self.carried.clear();
        }
        *self.slot_mut(slot_id) = slot;
    }

    fn distribute_carried_evenly(&mut self, touched_slots: Vec<SlotId>) {
        if self.carried.is_empty() || self.carried.is_tool() || touched_slots.is_empty() {
            return;
        }

        if touched_slots.len() == 1 {
            self.primary_click_slot(touched_slots[0]);
            return;
        }

        let mut valid_slots = Vec::new();
        let mut capacities: Vec<u16> = Vec::new();
        for slot_id in touched_slots {
            let cap = self.slot_capacity_for_carried(slot_id);
            if cap > 0 {
                valid_slots.push(slot_id);
                capacities.push(cap);
            }
        }
        if valid_slots.is_empty() {
            return;
        }

        let total_capacity: u16 = capacities
            .iter()
            .fold(0u16, |acc, &cap| acc.saturating_add(cap));
        let to_place = self.carried.count.min(total_capacity);
        if to_place == 0 {
            return;
        }

        let slot_count = valid_slots.len() as u16;
        let base = to_place / slot_count;
        let rem = to_place % slot_count;

        for i in 0..valid_slots.len() {
            if self.carried.is_empty() {
                break;
            }
            let mut target = base;
            if (i as u16) < rem {
                target = target.saturating_add(1);
            }
            let put = target.min(capacities[i]).min(self.carried.count);
            if put == 0 {
                continue;
            }
            self.place_multiple_from_carried(valid_slots[i], put);
            capacities[i] = capacities[i].saturating_sub(put);
        }

        while !self.carried.is_empty() {
            let mut placed_any = false;
            for i in 0..valid_slots.len() {
                if capacities[i] == 0 || self.carried.is_empty() {
                    continue;
                }
                self.place_multiple_from_carried(valid_slots[i], 1);
                capacities[i] = capacities[i].saturating_sub(1);
                placed_any = true;
                if self.carried.is_empty() {
                    break;
                }
            }
            if !placed_any {
                break;
            }
        }
    }

    fn slot_capacity_for_carried(&self, slot_id: SlotId) -> u16 {
        if self.carried.is_empty() || self.carried.is_tool() || slot_id == SlotId::FurnaceOutput {
            return 0;
        }

        let slot = self.slot(slot_id);
        if slot.is_empty() {
            let one = Slot::new(self.carried.block, 1);
            if self.can_place_in_slot(one, slot_id) {
                return MAX_STACK_SIZE;
            }
            return 0;
        }

        if slot.is_tool() || slot.block != self.carried.block {
            return 0;
        }
        MAX_STACK_SIZE.saturating_sub(slot.count)
    }

    fn place_multiple_from_carried(&mut self, slot_id: SlotId, amount: u16) {
        if amount == 0 || self.carried.is_empty() || self.carried.is_tool() {
            return;
        }

        let mut slot = self.slot(slot_id);
        if slot.is_empty() {
            let one = Slot::new(self.carried.block, 1);
            if !self.can_place_in_slot(one, slot_id) {
                return;
            }
            slot = Slot::new(self.carried.block, amount.min(self.carried.count));
            self.carried.count = self.carried.count.saturating_sub(slot.count);
        } else if !slot.is_tool() && slot.block == self.carried.block {
            let room = slot.available_space();
            let put = room.min(amount).min(self.carried.count);
            if put == 0 {
                return;
            }
            slot.count += put;
            self.carried.count = self.carried.count.saturating_sub(put);
        } else {
            return;
        }

        if self.carried.count == 0 {
            self.carried.clear();
        }
        *self.slot_mut(slot_id) = slot;
    }

    fn shift_click_slot(&mut self, slot_id: SlotId) -> bool {
        match slot_id {
            SlotId::Hotbar(i) => {
                let mut slot = self.hotbar[i];
                if slot.is_empty() {
                    return false;
                }
                shift_merge_slot(&mut slot, &mut self.grid);
                let changed = slot.count != self.hotbar[i].count || slot.tool != self.hotbar[i].tool;
                self.hotbar[i] = slot;
                changed
            }
            SlotId::Grid(i) => {
                let mut slot = self.grid[i];
                if slot.is_empty() {
                    return false;
                }
                shift_merge_slot(&mut slot, &mut self.hotbar);
                let changed = slot.count != self.grid[i].count || slot.tool != self.grid[i].tool;
                self.grid[i] = slot;
                changed
            }
            SlotId::Craft(i) => {
                let mut slot = self.craft_slot(i);
                if slot.is_empty() {
                    return false;
                }
                shift_merge_slot(&mut slot, &mut self.hotbar);
                if !slot.is_empty() {
                    shift_merge_slot(&mut slot, &mut self.grid);
                }
                let changed = slot.count != self.craft_slot(i).count || slot.tool != self.craft_slot(i).tool;
                *self.craft_slot_mut(i) = slot;
                changed
            }
            SlotId::FurnaceInput => {
                let mut slot = self.furnace_input;
                if slot.is_empty() {
                    return false;
                }
                shift_merge_slot(&mut slot, &mut self.hotbar);
                if !slot.is_empty() {
                    shift_merge_slot(&mut slot, &mut self.grid);
                }
                let changed = slot.count != self.furnace_input.count || slot.tool != self.furnace_input.tool;
                self.furnace_input = slot;
                changed
            }
            SlotId::FurnaceFuel => {
                let mut slot = self.furnace_fuel;
                if slot.is_empty() {
                    return false;
                }
                shift_merge_slot(&mut slot, &mut self.hotbar);
                if !slot.is_empty() {
                    shift_merge_slot(&mut slot, &mut self.grid);
                }
                let changed = slot.count != self.furnace_fuel.count || slot.tool != self.furnace_fuel.tool;
                self.furnace_fuel = slot;
                changed
            }
            SlotId::FurnaceOutput => {
                let mut slot = self.furnace_output;
                if slot.is_empty() {
                    return false;
                }
                shift_merge_slot(&mut slot, &mut self.hotbar);
                if !slot.is_empty() {
                    shift_merge_slot(&mut slot, &mut self.grid);
                }
                let changed = slot.count != self.furnace_output.count || slot.tool != self.furnace_output.tool;
                self.furnace_output = slot;
                changed
            }
        }
    }

    fn show_slot_hover_tooltip(&self, slot_id: SlotId, resp: &egui::Response) {
        if !resp.hovered() {
            return;
        }

        let slot = self.slot(slot_id);
        if slot.is_empty() {
            return;
        }

        let (name, item_id) = if let Some(tool) = slot.tool {
            (tool.display_name(), item_registry::tool_item_id(tool))
        } else {
            (block_display_name(slot.block), item_registry::block_item_id(slot.block))
        };
        let shift_down = resp.ctx.input(|i| i.modifiers.shift);

        let _ = resp.clone().on_hover_ui_at_pointer(|ui| {
            egui::Frame::none()
                .fill(Color32::from_rgba_unmultiplied(18, 18, 18, 242))
                .stroke(Stroke::new(1.0, Color32::from_rgb(64, 64, 64)))
                .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                .show(ui, |ui| {
                    ui.set_min_width(170.0);
                    ui.label(
                        RichText::new(name)
                            .size(13.0)
                            .strong()
                            .color(Color32::from_rgb(236, 236, 236)),
                    );
                    if !shift_down {
                        ui.label(
                            RichText::new("Hold Shift for details")
                                .size(11.0)
                                .color(Color32::from_rgb(170, 170, 170)),
                        );
                        return;
                    }

                    if let Some(tool) = slot.tool {
                        let max_durability = item_registry::tool_max_durability(tool).max(1);
                        let mut durability = self.tool_durability_values[tool.idx()];
                        if durability == 0 {
                            durability = max_durability;
                        }
                        durability = durability.min(max_durability);
                        ui.label(
                            RichText::new(format!("Durability: {durability}/{max_durability}"))
                                .size(12.0)
                                .color(Color32::from_rgb(208, 208, 208)),
                        );
                    } else {
                        ui.label(
                            RichText::new(format!("Stack: {}", slot.count))
                                .size(12.0)
                                .color(Color32::from_rgb(208, 208, 208)),
                        );
                    }
                    ui.label(
                        RichText::new(format!("ID: {}", item_id))
                            .size(12.0)
                            .color(Color32::from_rgb(208, 208, 208)),
                    );
                    ui.label(
                        RichText::new("Source: minecraft")
                            .size(12.0)
                            .color(Color32::from_rgb(188, 188, 188)),
                    );
                });
        });
    }

    fn current_craft_output(&self) -> Option<CraftOutput> {
        let snapshot = self.craft_snapshot()?;
        for recipe in craft_recipes() {
            if snapshot.width != recipe.width || snapshot.height != recipe.height {
                continue;
            }
            if snapshot.cells == recipe.cells {
                return Some(recipe.output);
            }
            if recipe.allow_mirror {
                let mirrored = mirrored_cells(recipe.width, recipe.height, recipe.cells);
                if snapshot.cells == mirrored {
                    return Some(recipe.output);
                }
            }
        }
        None
    }

    fn craft_snapshot(&self) -> Option<CraftSnapshot> {
        let (craft_cols, craft_rows) = self.craft_dims();
        let mut min_r = craft_rows;
        let mut min_c = craft_cols;
        let mut max_r = 0usize;
        let mut max_c = 0usize;
        let mut has_any = false;

        for r in 0..craft_rows {
            for c in 0..craft_cols {
                let s = self.craft_slot(r * craft_cols + c);
                if s.is_empty() || s.is_tool() {
                    continue;
                }
                has_any = true;
                min_r = min_r.min(r);
                min_c = min_c.min(c);
                max_r = max_r.max(r);
                max_c = max_c.max(c);
            }
        }
        if !has_any {
            return None;
        }

        let width = max_c - min_c + 1;
        let height = max_r - min_r + 1;
        let mut cells = Vec::with_capacity(width * height);
        for r in min_r..=max_r {
            for c in min_c..=max_c {
                let s = self.craft_slot(r * craft_cols + c);
                if s.is_empty() || s.is_tool() {
                    cells.push(None);
                } else {
                    cells.push(Some(s.block));
                }
            }
        }

        Some(CraftSnapshot {
            width,
            height,
            cells,
        })
    }

    fn take_craft_result(&mut self, out: CraftOutput, secondary_click: bool) {
        match out {
            CraftOutput::Block { block, count } => {
                let give_count = if secondary_click {
                    count.min(1)
                } else {
                    count
                };
                if give_count == 0 {
                    return;
                }
                if !self.store_crafted_block(block, give_count) {
                    return;
                }
            }
            CraftOutput::Tool(tool) => {
                if !self.store_crafted_tool(tool) {
                    return;
                }
                self.crafted_tools.push(tool);
            }
        }

        self.consume_craft_ingredients_once();
    }

    fn storage_capacity_for_block(&self, block: Block) -> u16 {
        let mut capacity = 0u16;
        for slot in self.hotbar.iter().chain(self.grid.iter()) {
            if slot.is_empty() {
                capacity = capacity.saturating_add(MAX_STACK_SIZE);
            } else if !slot.is_tool() && slot.block == block {
                capacity = capacity.saturating_add(slot.available_space());
            }
        }
        capacity
    }

    fn carried_capacity_for_block(&self, block: Block) -> u16 {
        if self.carried.is_empty() {
            MAX_STACK_SIZE
        } else if !self.carried.is_tool() && self.carried.block == block {
            self.carried.available_space()
        } else {
            0
        }
    }

    fn can_store_crafted_block(&self, block: Block, count: u16) -> bool {
        let total_capacity = self
            .carried_capacity_for_block(block)
            .saturating_add(self.storage_capacity_for_block(block));
        total_capacity >= count
    }

    fn store_crafted_block(&mut self, block: Block, count: u16) -> bool {
        if count == 0 {
            return true;
        }
        if !self.can_store_crafted_block(block, count) {
            return false;
        }

        let mut remaining = count;

        if self.carried.is_empty() {
            let put = remaining.min(MAX_STACK_SIZE);
            self.carried = Slot::new(block, put);
            remaining -= put;
        } else if !self.carried.is_tool() && self.carried.block == block {
            let put = remaining.min(self.carried.available_space());
            self.carried.count += put;
            remaining -= put;
        }

        if remaining > 0 {
            remaining = self.add_block(block, remaining);
        }

        remaining == 0
    }

    fn store_crafted_tool(&mut self, tool: WoodenTool) -> bool {
        if self.carried.is_empty() {
            self.carried = Slot::new_tool(tool);
            return true;
        }
        self.add_tool(tool)
    }

    fn consume_craft_ingredients_once(&mut self) {
        let (craft_cols, craft_rows) = self.craft_dims();
        for r in 0..craft_rows {
            for c in 0..craft_cols {
                let idx = r * craft_cols + c;
                let slot = self.craft_slot_mut(idx);
                if slot.is_empty() || slot.is_tool() {
                    continue;
                }
                slot.count = slot.count.saturating_sub(1);
                if slot.count == 0 {
                    slot.clear();
                }
            }
        }
    }

    fn return_temporary_slots_to_storage(&mut self) {
        self.return_craft_grid_to_storage(false);
        self.return_craft_grid_to_storage(true);

        if !self.carried.is_empty() {
            let carried = self.carried;
            self.carried = self.store_or_keep_slot_item(carried);
        }
    }

    fn return_craft_grid_to_storage(&mut self, workbench: bool) {
        if workbench {
            for idx in 0..WORKBENCH_CRAFT_SLOTS {
                let slot = self.craft_workbench[idx];
                if slot.is_empty() {
                    continue;
                }
                self.craft_workbench[idx] = self.store_or_keep_slot_item(slot);
            }
        } else {
            for idx in 0..PLAYER_CRAFT_SLOTS {
                let slot = self.craft_player[idx];
                if slot.is_empty() {
                    continue;
                }
                self.craft_player[idx] = self.store_or_keep_slot_item(slot);
            }
        }
    }

    fn store_or_keep_slot_item(&mut self, slot: Slot) -> Slot {
        if slot.is_empty() {
            return Slot::empty();
        }
        if let Some(tool) = slot.tool {
            if self.add_tool(tool) {
                Slot::empty()
            } else {
                slot
            }
        } else {
            let remaining = self.add_block(slot.block, slot.count);
            Slot::new(slot.block, remaining)
        }
    }

    fn primary_click_slot(&mut self, slot_id: SlotId) {
        if slot_id == SlotId::FurnaceOutput {
            self.take_furnace_output_primary();
            return;
        }

        let target_restricted = is_restricted_input_slot(slot_id);
        let mut slot = self.slot(slot_id);

        if self.carried.is_empty() {
            if slot.is_empty() {
                return;
            }
            self.carried = slot;
            slot.clear();
            *self.slot_mut(slot_id) = slot;
            return;
        }

        if slot.is_empty() {
            if !self.can_place_in_slot(self.carried, slot_id) {
                return;
            }
            slot = self.carried;
            self.carried.clear();
            *self.slot_mut(slot_id) = slot;
            return;
        }

        if !slot.is_tool()
            && !self.carried.is_tool()
            && slot.block == self.carried.block
        {
            let moved = slot.available_space().min(self.carried.count);
            if moved > 0 {
                slot.count += moved;
                self.carried.count -= moved;
                if self.carried.count == 0 {
                    self.carried.clear();
                }
            }
            *self.slot_mut(slot_id) = slot;
            return;
        }

        if target_restricted && (slot.is_tool() || self.carried.is_tool()) {
            return;
        }
        if !self.can_place_in_slot(self.carried, slot_id) {
            return;
        }

        std::mem::swap(&mut slot, &mut self.carried);
        *self.slot_mut(slot_id) = slot;
    }

    fn secondary_click_slot(&mut self, slot_id: SlotId) {
        if slot_id == SlotId::FurnaceOutput {
            self.take_furnace_output_secondary();
            return;
        }

        let target_restricted = is_restricted_input_slot(slot_id);
        let mut slot = self.slot(slot_id);

        if self.carried.is_empty() {
            if slot.is_empty() {
                return;
            }
            if slot.is_tool() {
                self.carried = slot;
                slot.clear();
                *self.slot_mut(slot_id) = slot;
                return;
            }
            let take = (slot.count + 1) / 2;
            self.carried = Slot::new(slot.block, take);
            slot.count -= take;
            if slot.count == 0 {
                slot.clear();
            }
            *self.slot_mut(slot_id) = slot;
            return;
        }

        if slot.is_empty() {
            if self.carried.is_tool() {
                if target_restricted || !self.can_place_in_slot(self.carried, slot_id) {
                    return;
                }
                slot = self.carried;
                self.carried.clear();
            } else {
                let place_one = Slot::new(self.carried.block, 1);
                if !self.can_place_in_slot(place_one, slot_id) {
                    return;
                }
                slot = place_one;
                self.carried.count -= 1;
                if self.carried.count == 0 {
                    self.carried.clear();
                }
            }
            *self.slot_mut(slot_id) = slot;
            return;
        }

        if !slot.is_tool()
            && !self.carried.is_tool()
            && slot.block == self.carried.block
            && slot.count < MAX_STACK_SIZE
        {
            slot.count += 1;
            self.carried.count -= 1;
            if self.carried.count == 0 {
                self.carried.clear();
            }
            *self.slot_mut(slot_id) = slot;
        }
    }

    fn can_place_in_slot(&self, slot: Slot, target: SlotId) -> bool {
        if slot.is_empty() {
            return true;
        }

        match target {
            SlotId::Craft(_) => !slot.is_tool(),
            SlotId::FurnaceInput => !slot.is_tool() && furnace_recipe_for(slot.block).is_some(),
            SlotId::FurnaceFuel => !slot.is_tool() && furnace_fuel_smelts(slot.block) > 0.0,
            SlotId::FurnaceOutput => false,
            SlotId::Hotbar(_) | SlotId::Grid(_) => true,
        }
    }

    fn take_furnace_output_primary(&mut self) {
        if self.furnace_output.is_empty() {
            return;
        }

        if self.carried.is_empty() {
            self.carried = self.furnace_output;
            self.furnace_output.clear();
            return;
        }

        if self.carried.is_tool()
            || self.furnace_output.is_tool()
            || self.carried.block != self.furnace_output.block
        {
            return;
        }

        let moved = self.carried.available_space().min(self.furnace_output.count);
        if moved == 0 {
            return;
        }
        self.carried.count += moved;
        self.furnace_output.count -= moved;
        if self.furnace_output.count == 0 {
            self.furnace_output.clear();
        }
    }

    fn take_furnace_output_secondary(&mut self) {
        if self.furnace_output.is_empty() || self.furnace_output.is_tool() {
            return;
        }

        if self.carried.is_empty() {
            self.carried = Slot::new(self.furnace_output.block, 1);
            self.furnace_output.count -= 1;
            if self.furnace_output.count == 0 {
                self.furnace_output.clear();
            }
            return;
        }

        if self.carried.is_tool()
            || self.carried.block != self.furnace_output.block
            || self.carried.count >= MAX_STACK_SIZE
        {
            return;
        }
        self.carried.count += 1;
        self.furnace_output.count -= 1;
        if self.furnace_output.count == 0 {
            self.furnace_output.clear();
        }
    }

    fn draw_carried(&mut self, ctx: &egui::Context) {
        if self.carried.is_empty() {
            return;
        }

        let Some(pointer) = ctx.input(|i| i.pointer.latest_pos()) else {
            return;
        };

        let size = 28.0;
        let rect = egui::Rect::from_min_size(pointer + egui::vec2(8.0, 8.0), egui::vec2(size, size));
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("inv_carried_item"),
        ));

        paint_minecraft_slot_frame(&painter, rect, true);

        let item_rect = rect.shrink(4.0);
        let tex = self.slot_texture_id(ctx, self.carried);
        paint_slot_icon(&painter, item_rect, &self.carried, tex);

        if !self.carried.is_tool() && self.carried.count > 1 {
            painter.text(
                rect.right_bottom() - egui::vec2(2.0, 2.0),
                Align2::RIGHT_BOTTOM,
                self.carried.count.to_string(),
                FontId::proportional(12.0),
                Color32::WHITE,
            );
        }
    }

    fn block_texture_id(&mut self, ctx: &egui::Context, block: Block) -> Option<egui::TextureId> {
        if block.is_air() {
            return None;
        }

        let bid = block.id();
        if let Some(handle) = self.texture_cache.get(&bid) {
            return Some(handle.id());
        }
        if self.texture_missing.contains(&bid) {
            return None;
        }

        let Some(img) = load_inventory_block_texture(block) else {
            self.texture_missing.insert(bid);
            return None;
        };

        let tex_name = format!("inv_block_{}_{}", block.texture_name(), bid);
        let handle = ctx.load_texture(tex_name, img, egui::TextureOptions::NEAREST);
        let id = handle.id();
        self.texture_cache.insert(bid, handle);
        Some(id)
    }

    fn slot_texture_id(&mut self, ctx: &egui::Context, slot: Slot) -> Option<egui::TextureId> {
        if let Some(tool) = slot.tool {
            self.tool_texture_id(ctx, tool)
        } else {
            self.block_texture_id(ctx, slot.block)
        }
    }

    fn tool_texture_id(&mut self, ctx: &egui::Context, tool: WoodenTool) -> Option<egui::TextureId> {
        if let Some(handle) = self.tool_texture_cache.get(&tool) {
            return Some(handle.id());
        }
        if self.tool_texture_missing.contains(&tool) {
            return None;
        }

        let Some(img) = load_inventory_tool_texture(tool) else {
            self.tool_texture_missing.insert(tool);
            return None;
        };

        let tex_name = format!("inv_tool_{}", tool.display_name().replace(' ', "_").to_ascii_lowercase());
        let handle = ctx.load_texture(tex_name, img, egui::TextureOptions::NEAREST);
        let id = handle.id();
        self.tool_texture_cache.insert(tool, handle);
        Some(id)
    }

    fn slot(&self, slot_id: SlotId) -> Slot {
        match slot_id {
            SlotId::Hotbar(i) => self.hotbar[i],
            SlotId::Grid(i) => self.grid[i],
            SlotId::Craft(i) => self.craft_slot(i),
            SlotId::FurnaceInput => self.furnace_input,
            SlotId::FurnaceFuel => self.furnace_fuel,
            SlotId::FurnaceOutput => self.furnace_output,
        }
    }

    fn slot_mut(&mut self, slot_id: SlotId) -> &mut Slot {
        match slot_id {
            SlotId::Hotbar(i) => &mut self.hotbar[i],
            SlotId::Grid(i) => &mut self.grid[i],
            SlotId::Craft(i) => self.craft_slot_mut(i),
            SlotId::FurnaceInput => &mut self.furnace_input,
            SlotId::FurnaceFuel => &mut self.furnace_fuel,
            SlotId::FurnaceOutput => &mut self.furnace_output,
        }
    }

    fn total_count(&self, block: Block) -> u16 {
        self.hotbar
            .iter()
            .chain(self.grid.iter())
            .filter(|s| !s.is_tool() && s.block == block && !s.is_empty())
            .map(|s| s.count)
            .sum()
    }

    fn remove_block_amount(&mut self, block: Block, mut amount: u16) -> bool {
        if amount == 0 {
            return true;
        }
        if self.total_count(block) < amount {
            return false;
        }

        for slot in self.hotbar.iter_mut().chain(self.grid.iter_mut()) {
            if slot.is_tool() || slot.block != block || slot.is_empty() {
                continue;
            }
            let take = amount.min(slot.count);
            slot.count -= take;
            amount -= take;
            if slot.count == 0 {
                slot.clear();
            }
            if amount == 0 {
                return true;
            }
        }
        amount == 0
    }

    fn can_add_exact(&self, block: Block, count: u16) -> bool {
        if count == 0 {
            return true;
        }
        let mut capacity = 0u16;
        for slot in self.hotbar.iter().chain(self.grid.iter()) {
            if slot.is_empty() {
                capacity = capacity.saturating_add(MAX_STACK_SIZE);
            } else if !slot.is_tool() && slot.block == block {
                capacity = capacity.saturating_add(slot.available_space());
            }
            if capacity >= count {
                return true;
            }
        }
        false
    }

    fn can_craft(&self, recipe: CraftRecipe) -> bool {
        for &(block, count) in recipe.inputs {
            if self.total_count(block) < count {
                return false;
            }
        }
        self.can_add_exact(recipe.output.0, recipe.output.1)
    }
}

#[derive(Clone, Copy)]
struct CraftRecipe {
    name: &'static str,
    inputs: &'static [(Block, u16)],
    output: (Block, u16),
}

#[derive(Clone)]
struct CraftSnapshot {
    width: usize,
    height: usize,
    cells: Vec<Option<Block>>,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum CraftOutput {
    Block { block: Block, count: u16 },
    Tool(WoodenTool),
}

#[derive(Clone, Copy)]
struct CraftGridRecipe {
    width: usize,
    height: usize,
    cells: &'static [Option<Block>],
    output: CraftOutput,
    allow_mirror: bool,
}

const R_WORKBENCH: [Option<Block>; 4] = [
    Some(Block::Wood), Some(Block::Wood),
    Some(Block::Wood), Some(Block::Wood),
];

const R_WOOD_FROM_LOG: [Option<Block>; 1] = [
    Some(Block::Log),
];

const R_STICKS: [Option<Block>; 2] = [
    Some(Block::Wood),
    Some(Block::Wood),
];

const R_TORCHES: [Option<Block>; 2] = [
    Some(Block::Coal),
    Some(Block::Stick),
];

const R_WOOD_PICKAXE: [Option<Block>; 9] = [
    Some(Block::Wood), Some(Block::Wood), Some(Block::Wood),
    None,              Some(Block::Stick), None,
    None,              Some(Block::Stick), None,
];

const R_WOOD_AXE: [Option<Block>; 6] = [
    Some(Block::Wood), Some(Block::Wood),
    Some(Block::Wood), Some(Block::Stick),
    None,              Some(Block::Stick),
];

const R_WOOD_SHOVEL: [Option<Block>; 3] = [
    Some(Block::Wood),
    Some(Block::Stick),
    Some(Block::Stick),
];

const R_WOOD_HOE: [Option<Block>; 6] = [
    Some(Block::Wood), Some(Block::Wood),
    None,              Some(Block::Stick),
    None,              Some(Block::Stick),
];

const R_WOOD_SWORD: [Option<Block>; 3] = [
    Some(Block::Wood),
    Some(Block::Wood),
    Some(Block::Stick),
];

const R_STONE_PICKAXE: [Option<Block>; 9] = [
    Some(Block::Stone), Some(Block::Stone), Some(Block::Stone),
    None,               Some(Block::Stick), None,
    None,               Some(Block::Stick), None,
];

const R_STONE_AXE: [Option<Block>; 6] = [
    Some(Block::Stone), Some(Block::Stone),
    Some(Block::Stone), Some(Block::Stick),
    None,               Some(Block::Stick),
];

const R_STONE_SHOVEL: [Option<Block>; 3] = [
    Some(Block::Stone),
    Some(Block::Stick),
    Some(Block::Stick),
];

const R_STONE_HOE: [Option<Block>; 6] = [
    Some(Block::Stone), Some(Block::Stone),
    None,               Some(Block::Stick),
    None,               Some(Block::Stick),
];

const R_STONE_SWORD: [Option<Block>; 3] = [
    Some(Block::Stone),
    Some(Block::Stone),
    Some(Block::Stick),
];

const R_IRON_PICKAXE: [Option<Block>; 9] = [
    Some(Block::IronIngot), Some(Block::IronIngot), Some(Block::IronIngot),
    None,                   Some(Block::Stick),      None,
    None,                   Some(Block::Stick),      None,
];

const R_IRON_AXE: [Option<Block>; 6] = [
    Some(Block::IronIngot), Some(Block::IronIngot),
    Some(Block::IronIngot), Some(Block::Stick),
    None,                   Some(Block::Stick),
];

const R_IRON_SHOVEL: [Option<Block>; 3] = [
    Some(Block::IronIngot),
    Some(Block::Stick),
    Some(Block::Stick),
];

const R_IRON_HOE: [Option<Block>; 6] = [
    Some(Block::IronIngot), Some(Block::IronIngot),
    None,                   Some(Block::Stick),
    None,                   Some(Block::Stick),
];

const R_IRON_SWORD: [Option<Block>; 3] = [
    Some(Block::IronIngot),
    Some(Block::IronIngot),
    Some(Block::Stick),
];

const R_FURNACE: [Option<Block>; 9] = [
    Some(Block::Stone), Some(Block::Stone), Some(Block::Stone),
    Some(Block::Stone), None,               Some(Block::Stone),
    Some(Block::Stone), Some(Block::Stone), Some(Block::Stone),
];

fn craft_recipes() -> &'static [CraftGridRecipe] {
    const RECIPES: &[CraftGridRecipe] = &[
        CraftGridRecipe {
            width: 1,
            height: 1,
            cells: &R_WOOD_FROM_LOG,
            output: CraftOutput::Block {
                block: Block::Wood,
                count: 4,
            },
            allow_mirror: false,
        },
        CraftGridRecipe {
            width: 2,
            height: 2,
            cells: &R_WORKBENCH,
            output: CraftOutput::Block {
                block: Block::Workbench,
                count: 1,
            },
            allow_mirror: false,
        },
        CraftGridRecipe {
            width: 1,
            height: 2,
            cells: &R_STICKS,
            output: CraftOutput::Block {
                block: Block::Stick,
                count: 4,
            },
            allow_mirror: false,
        },
        CraftGridRecipe {
            width: 1,
            height: 2,
            cells: &R_TORCHES,
            output: CraftOutput::Block {
                block: Block::Torch,
                count: 4,
            },
            allow_mirror: false,
        },
        CraftGridRecipe {
            width: 3,
            height: 3,
            cells: &R_WOOD_PICKAXE,
            output: CraftOutput::Tool(WoodenTool::Pickaxe),
            allow_mirror: false,
        },
        CraftGridRecipe {
            width: 2,
            height: 3,
            cells: &R_WOOD_AXE,
            output: CraftOutput::Tool(WoodenTool::Axe),
            allow_mirror: true,
        },
        CraftGridRecipe {
            width: 1,
            height: 3,
            cells: &R_WOOD_SHOVEL,
            output: CraftOutput::Tool(WoodenTool::Shovel),
            allow_mirror: false,
        },
        CraftGridRecipe {
            width: 2,
            height: 3,
            cells: &R_WOOD_HOE,
            output: CraftOutput::Tool(WoodenTool::Hoe),
            allow_mirror: true,
        },
        CraftGridRecipe {
            width: 1,
            height: 3,
            cells: &R_WOOD_SWORD,
            output: CraftOutput::Tool(WoodenTool::Sword),
            allow_mirror: false,
        },
        CraftGridRecipe {
            width: 3,
            height: 3,
            cells: &R_STONE_PICKAXE,
            output: CraftOutput::Tool(WoodenTool::StonePickaxe),
            allow_mirror: false,
        },
        CraftGridRecipe {
            width: 2,
            height: 3,
            cells: &R_STONE_AXE,
            output: CraftOutput::Tool(WoodenTool::StoneAxe),
            allow_mirror: true,
        },
        CraftGridRecipe {
            width: 1,
            height: 3,
            cells: &R_STONE_SHOVEL,
            output: CraftOutput::Tool(WoodenTool::StoneShovel),
            allow_mirror: false,
        },
        CraftGridRecipe {
            width: 2,
            height: 3,
            cells: &R_STONE_HOE,
            output: CraftOutput::Tool(WoodenTool::StoneHoe),
            allow_mirror: true,
        },
        CraftGridRecipe {
            width: 1,
            height: 3,
            cells: &R_STONE_SWORD,
            output: CraftOutput::Tool(WoodenTool::StoneSword),
            allow_mirror: false,
        },
        CraftGridRecipe {
            width: 3,
            height: 3,
            cells: &R_IRON_PICKAXE,
            output: CraftOutput::Tool(WoodenTool::IronPickaxe),
            allow_mirror: false,
        },
        CraftGridRecipe {
            width: 2,
            height: 3,
            cells: &R_IRON_AXE,
            output: CraftOutput::Tool(WoodenTool::IronAxe),
            allow_mirror: true,
        },
        CraftGridRecipe {
            width: 1,
            height: 3,
            cells: &R_IRON_SHOVEL,
            output: CraftOutput::Tool(WoodenTool::IronShovel),
            allow_mirror: false,
        },
        CraftGridRecipe {
            width: 2,
            height: 3,
            cells: &R_IRON_HOE,
            output: CraftOutput::Tool(WoodenTool::IronHoe),
            allow_mirror: true,
        },
        CraftGridRecipe {
            width: 1,
            height: 3,
            cells: &R_IRON_SWORD,
            output: CraftOutput::Tool(WoodenTool::IronSword),
            allow_mirror: false,
        },
        CraftGridRecipe {
            width: 3,
            height: 3,
            cells: &R_FURNACE,
            output: CraftOutput::Block {
                block: Block::Furnace,
                count: 1,
            },
            allow_mirror: false,
        },
    ];
    RECIPES
}

#[derive(Clone, Copy)]
struct FurnaceRecipe {
    input: Block,
    output: Block,
    output_count: u16,
}

fn furnace_recipe_for(input: Block) -> Option<FurnaceRecipe> {
    const RECIPES: [FurnaceRecipe; 4] = [
        FurnaceRecipe {
            input: Block::Log,
            output: Block::Coal,
            output_count: 1,
        },
        FurnaceRecipe {
            input: Block::Wood,
            output: Block::Coal,
            output_count: 1,
        },
        FurnaceRecipe {
            input: Block::CoalOre,
            output: Block::Coal,
            output_count: 1,
        },
        FurnaceRecipe {
            input: Block::IronOre,
            output: Block::IronIngot,
            output_count: 1,
        },
    ];

    RECIPES.iter().copied().find(|r| r.input == input)
}

fn furnace_fuel_smelts(block: Block) -> f32 {
    match block {
        Block::Log => 2.0,
        Block::Wood => 1.5,
        Block::Coal => 5.0,
        _ => 0.0,
    }
}

fn is_restricted_input_slot(slot_id: SlotId) -> bool {
    matches!(slot_id, SlotId::Craft(_) | SlotId::FurnaceInput | SlotId::FurnaceFuel)
}

fn consume_block_from_slot(slot: &mut Slot, expected: Block, amount: u16) -> bool {
    if slot.is_empty() || slot.is_tool() || slot.block != expected || amount == 0 {
        return false;
    }
    if slot.count < amount {
        return false;
    }
    slot.count -= amount;
    if slot.count == 0 {
        slot.clear();
    }
    true
}

fn shift_merge_slot(source: &mut Slot, target: &mut [Slot]) {
    if source.is_empty() {
        return;
    }

    if source.is_tool() {
        for dst in target.iter_mut() {
            if dst.is_empty() {
                *dst = *source;
                source.clear();
                return;
            }
        }
        return;
    }

    for dst in target.iter_mut() {
        if source.is_empty() {
            return;
        }
        if dst.is_empty() || dst.is_tool() || dst.block != source.block || dst.count >= MAX_STACK_SIZE {
            continue;
        }
        let moved = source.count.min(dst.available_space());
        if moved == 0 {
            continue;
        }
        dst.count += moved;
        source.count -= moved;
        if source.count == 0 {
            source.clear();
            return;
        }
    }

    for dst in target.iter_mut() {
        if source.is_empty() {
            return;
        }
        if !dst.is_empty() {
            continue;
        }
        let moved = source.count.min(MAX_STACK_SIZE);
        *dst = Slot::new(source.block, moved);
        source.count -= moved;
        if source.count == 0 {
            source.clear();
            return;
        }
    }
}

fn draw_meter(ui: &mut egui::Ui, value: f32, size: egui::Vec2, fill: Color32) {
    let value = value.clamp(0.0, 1.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, egui::Rounding::same(2.0), Color32::from_rgb(48, 48, 48));
    painter.rect_stroke(
        rect,
        egui::Rounding::same(2.0),
        Stroke::new(1.0, Color32::from_rgb(20, 20, 20)),
    );
    let fill_width = rect.width() * value;
    if fill_width > 0.5 {
        let fill_rect = egui::Rect::from_min_max(
            rect.min,
            egui::pos2(rect.min.x + fill_width, rect.max.y),
        );
        painter.rect_filled(fill_rect, egui::Rounding::same(2.0), fill);
    }
}

fn mirrored_cells(width: usize, height: usize, cells: &[Option<Block>]) -> Vec<Option<Block>> {
    let mut out = Vec::with_capacity(cells.len());
    for r in 0..height {
        for c in 0..width {
            out.push(cells[r * width + (width - 1 - c)]);
        }
    }
    out
}

#[allow(dead_code)]
fn craft_output_label(output: CraftOutput) -> &'static str {
    match output {
        CraftOutput::Block { block: Block::Wood, .. } => "Wood x4",
        CraftOutput::Block { block: Block::Workbench, .. } => "Workbench",
        CraftOutput::Block { block: Block::Stick, .. } => "Stick x4",
        CraftOutput::Block { block: Block::Furnace, .. } => "Furnace",
        CraftOutput::Block { .. } => "Crafted Block",
        CraftOutput::Tool(tool) => tool.display_name(),
    }
}

fn draw_player_preview_stub(ui: &mut egui::Ui, slot_size: f32) {
    let armor_slot = (slot_size - 2.0).max(26.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            for _ in 0..4 {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(armor_slot, armor_slot), egui::Sense::hover());
                paint_minecraft_slot_frame(ui.painter(), rect, false);
                ui.add_space(2.0);
            }
        });

        ui.add_space(4.0);
        let preview_size = egui::vec2(slot_size * 2.95, slot_size * 4.05);
        let (rect, _) = ui.allocate_exact_size(preview_size, egui::Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, egui::Rounding::same(0.0), Color32::from_rgb(6, 6, 6));
        painter.rect_stroke(
            rect,
            egui::Rounding::same(0.0),
            Stroke::new(1.0, Color32::from_rgb(56, 56, 56)),
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Player",
            FontId::monospace(13.0),
            Color32::from_rgb(116, 116, 116),
        );
    });
}

fn paint_minecraft_slot_frame(painter: &egui::Painter, rect: egui::Rect, selected: bool) {
    let base = if selected {
        Color32::from_rgb(166, 166, 166)
    } else {
        Color32::from_rgb(142, 142, 142)
    };
    painter.rect_filled(rect, egui::Rounding::same(0.0), base);
    painter.rect_stroke(
        rect,
        egui::Rounding::same(0.0),
        Stroke::new(1.0, Color32::from_rgb(55, 55, 55)),
    );
    let min = rect.min;
    let max = rect.max;
    let light = Color32::from_rgb(232, 232, 232);
    let dark = Color32::from_rgb(42, 42, 42);
    painter.line_segment([min, egui::pos2(max.x, min.y)], Stroke::new(1.0, light));
    painter.line_segment([min, egui::pos2(min.x, max.y)], Stroke::new(1.0, light));
    painter.line_segment([egui::pos2(min.x, max.y), max], Stroke::new(1.0, dark));
    painter.line_segment([egui::pos2(max.x, min.y), max], Stroke::new(1.0, dark));

    if selected {
        painter.rect_stroke(
            rect.expand(1.0),
            egui::Rounding::same(0.0),
            Stroke::new(1.0, Color32::from_rgb(255, 255, 255)),
        );
    }
}

fn paint_stack_count_text(painter: &egui::Painter, rect: egui::Rect, text: &str) {
    let pos = rect.right_bottom() - egui::vec2(3.0, 3.0);
    painter.text(
        pos + egui::vec2(1.0, 1.0),
        Align2::RIGHT_BOTTOM,
        text,
        FontId::proportional(20.0),
        Color32::from_rgb(36, 36, 36),
    );
    painter.text(
        pos,
        Align2::RIGHT_BOTTOM,
        text,
        FontId::proportional(20.0),
        Color32::from_rgb(246, 246, 246),
    );
}

fn paint_craft_result_slot(
    ui: &mut egui::Ui,
    output: Option<&CraftOutput>,
    size: f32,
    block_texture: Option<egui::TextureId>,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let painter = ui.painter();
    paint_minecraft_slot_frame(painter, rect, false);

    if let Some(output) = output {
        let item_rect = rect.shrink(4.0);
        match output {
            CraftOutput::Block { block, count } => {
                paint_block_icon(&painter, item_rect, *block, block_texture);
                if *count > 1 {
                    paint_stack_count_text(&painter, rect, &count.to_string());
                }
            }
            CraftOutput::Tool(tool) => {
                if let Some(texture_id) = block_texture {
                    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    painter.image(texture_id, item_rect, uv, Color32::WHITE);
                } else {
                    let (abbr, tint) = tool_icon_style(*tool);
                    painter.rect_filled(item_rect, egui::Rounding::same(1.0), tint);
                    painter.text(
                        item_rect.center(),
                        Align2::CENTER_CENTER,
                        abbr,
                        FontId::proportional(11.0),
                        Color32::from_rgb(24, 24, 24),
                    );
                }
            }
        }
    }

    resp
}

fn paint_slot(
    ui: &mut egui::Ui,
    slot: &Slot,
    selected: bool,
    size: f32,
    texture_id: Option<egui::TextureId>,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let painter = ui.painter();
    paint_minecraft_slot_frame(painter, rect, selected);

    if !slot.is_empty() {
        let item_rect = rect.shrink(4.0);
        paint_slot_icon(painter, item_rect, slot, texture_id);
        if !slot.is_tool() && slot.count > 1 {
            paint_stack_count_text(painter, rect, &slot.count.to_string());
        }
    }

    resp
}

fn paint_block_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    block: Block,
    texture_id: Option<egui::TextureId>,
) {
    if let Some(texture_id) = texture_id {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(texture_id, rect, uv, Color32::WHITE);
    } else {
        painter.rect_filled(rect, egui::Rounding::same(1.0), block_color(block));
    }
}

fn paint_slot_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    slot: &Slot,
    texture_id: Option<egui::TextureId>,
) {
    if let Some(texture_id) = texture_id {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(texture_id, rect, uv, Color32::WHITE);
        return;
    }

    if let Some(tool) = slot.tool {
        let (abbr, tint) = tool_icon_style(tool);
        painter.rect_filled(rect, egui::Rounding::same(1.0), tint);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            abbr,
            FontId::proportional(11.0),
            Color32::from_rgb(24, 24, 24),
        );
        return;
    }

    painter.rect_filled(rect, egui::Rounding::same(1.0), block_color(slot.block));
}

fn tool_icon_style(tool: WoodenTool) -> (&'static str, Color32) {
    match tool {
        WoodenTool::Pickaxe => ("WP", Color32::from_rgb(150, 124, 84)),
        WoodenTool::Axe => ("WA", Color32::from_rgb(160, 116, 78)),
        WoodenTool::Shovel => ("WS", Color32::from_rgb(144, 128, 92)),
        WoodenTool::Hoe => ("WH", Color32::from_rgb(154, 122, 86)),
        WoodenTool::Sword => ("SW", Color32::from_rgb(162, 132, 98)),
        WoodenTool::StonePickaxe => ("SP", Color32::from_rgb(150, 150, 150)),
        WoodenTool::StoneAxe => ("SA", Color32::from_rgb(158, 158, 158)),
        WoodenTool::StoneShovel => ("SS", Color32::from_rgb(144, 144, 144)),
        WoodenTool::StoneHoe => ("SH", Color32::from_rgb(154, 154, 154)),
        WoodenTool::StoneSword => ("SX", Color32::from_rgb(166, 166, 166)),
        WoodenTool::IronPickaxe => ("IP", Color32::from_rgb(191, 191, 210)),
        WoodenTool::IronAxe => ("IA", Color32::from_rgb(198, 198, 216)),
        WoodenTool::IronShovel => ("IS", Color32::from_rgb(184, 184, 204)),
        WoodenTool::IronHoe => ("IH", Color32::from_rgb(194, 194, 212)),
        WoodenTool::IronSword => ("IX", Color32::from_rgb(206, 206, 222)),
    }
}

fn load_inventory_block_texture(block: Block) -> Option<egui::ColorImage> {
    if let Some(path) = item_registry::block_texture_path(block) {
        if let Some(rgba) = decode_rgba_image_from_path(&path) {
            let size = [rgba.width() as usize, rgba.height() as usize];
            let raw = rgba.into_raw();
            return Some(egui::ColorImage::from_rgba_unmultiplied(size, &raw));
        }
    }

    let path = resolve_inventory_texture_path(block)?;
    let rgba = decode_rgba_image_from_path(&path)?;
    let size = [rgba.width() as usize, rgba.height() as usize];
    let raw = rgba.into_raw();
    Some(egui::ColorImage::from_rgba_unmultiplied(size, &raw))
}

fn load_inventory_tool_texture(tool: WoodenTool) -> Option<egui::ColorImage> {
    let path = item_registry::tool_texture_path(tool)?;
    let rgba = decode_rgba_image_from_path(&path)?;
    let size = [rgba.width() as usize, rgba.height() as usize];
    let raw = rgba.into_raw();
    Some(egui::ColorImage::from_rgba_unmultiplied(size, &raw))
}

fn decode_rgba_image_from_path(path: &Path) -> Option<image::RgbaImage> {
    if let Ok(img) = image::open(path) {
        return Some(img.to_rgba8());
    }
    let bytes = std::fs::read(path).ok()?;
    image::load_from_memory(&bytes).ok().map(|img| img.to_rgba8())
}

fn resolve_inventory_texture_path(block: Block) -> Option<PathBuf> {
    let bases = paths::base_roots();
    let names = inventory_texture_aliases(block);
    let mut candidates = Vec::new();
    for base in &bases {
        for name in names {
            candidates.push(base.join("src").join("assets").join("blocks").join(format!("{name}.png")));
            candidates.push(base.join("src").join("assets").join("items").join(format!("{name}.png")));
            candidates.push(base.join("src").join("assets").join("blocks").join(format!("{name}.jpg")));
            candidates.push(base.join("src").join("assets").join("items").join(format!("{name}.jpg")));
            candidates.push(base.join("src").join("assets").join("blocks").join(format!("{name}.jpeg")));
            candidates.push(base.join("src").join("assets").join("items").join(format!("{name}.jpeg")));
            candidates.push(
                base.join("src")
                    .join("assets")
                    .join("minecraft")
                    .join("textures")
                    .join("block")
                    .join(format!("{name}.png")),
            );
            candidates.push(
                base.join("src")
                    .join("assets")
                    .join("minecraft")
                    .join("textures")
                    .join("block")
                    .join(format!("{name}.jpg")),
            );
            candidates.push(
                base.join("src")
                    .join("assets")
                    .join("minecraft")
                    .join("textures")
                    .join("block")
                    .join(format!("{name}.jpeg")),
            );
            candidates.push(base.join("assets").join("blocks").join(format!("{name}.png")));
            candidates.push(base.join("assets").join("items").join(format!("{name}.png")));
            candidates.push(base.join("assets").join("blocks").join(format!("{name}.jpg")));
            candidates.push(base.join("assets").join("items").join(format!("{name}.jpg")));
            candidates.push(base.join("assets").join("blocks").join(format!("{name}.jpeg")));
            candidates.push(base.join("assets").join("items").join(format!("{name}.jpeg")));
            candidates.push(
                base.join("assets")
                    .join("minecraft")
                    .join("textures")
                    .join("block")
                    .join(format!("{name}.png")),
            );
            candidates.push(
                base.join("assets")
                    .join("minecraft")
                    .join("textures")
                    .join("block")
                    .join(format!("{name}.jpg")),
            );
            candidates.push(
                base.join("assets")
                    .join("minecraft")
                    .join("textures")
                    .join("block")
                    .join(format!("{name}.jpeg")),
            );
        }
    }

    if let Some(path) = candidates.into_iter().find(|p| p.exists()) {
        return Some(path);
    }

    let mut recursive_roots = Vec::new();
    for base in &bases {
        recursive_roots.push(base.join("src").join("assets").join("blocks"));
        recursive_roots.push(base.join("src").join("assets").join("items"));
        recursive_roots.push(base.join("assets").join("blocks"));
        recursive_roots.push(base.join("assets").join("items"));
    }

    for root in recursive_roots {
        if !root.exists() {
            continue;
        }
        if let Some(path) = find_inventory_texture_recursive(&root, names) {
            return Some(path);
        }
    }

    None
}

fn find_inventory_texture_recursive(root: &Path, names: &[&str]) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut matches = Vec::new();

    while let Some(dir) = stack.pop() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();

        for path in entries {
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if !path.is_file() {
                continue;
            }
            let ext_ok = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|ext| {
                    matches!(
                        ext.to_ascii_lowercase().as_str(),
                        "png" | "jpg" | "jpeg" | "webp" | "avif"
                    )
                })
                .unwrap_or(false);
            if !ext_ok {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if names.iter().any(|n| stem.eq_ignore_ascii_case(n)) {
                matches.push(path);
            }
        }
    }

    matches.sort();
    matches.into_iter().next()
}

fn inventory_texture_aliases(block: Block) -> &'static [&'static str] {
    match block {
        Block::Air | Block::CaveAir => &[],
        Block::Workbench => &[
            "workbench_front",
            "workbench1",
            "workbench_front1",
            "workbench_side",
            "workbench",
            "crafting_table",
            "planks",
            "wood",
        ],
        Block::Furnace => &[
            "furnace_front",
            "furnace_face",
            "furnace_face_active",
            "furnace",
            "furnace_side",
            "stone",
        ],
        Block::Coal => &["coal", "charcoal", "coal_item"],
        Block::IronIngot => &["iron_ingot", "iron", "iron_nugget"],
        Block::Torch => &["torch", "wall_torch"],
        Block::Wood => &["wood", "planks", "oak_planks"],
        Block::Stick => &["stick"],
        Block::Grass => &["grass", "grass_block_side"],
        Block::Dirt => &["dirt"],
        Block::FarmlandDry => &["farmland_dry", "farmland", "dirt"],
        Block::FarmlandWet => &["farmland_wet", "farmland", "dirt"],
        Block::Stone => &["stone"],
        Block::Sand => &["sand"],
        Block::Water => &["water"],
        Block::Bedrock => &["bedrock"],
        Block::Log => &["log", "oak_log"],
        Block::LogBottom => &["logBottom", "log_top_down", "oak_log_top", "oak_log"],
        Block::Leaves => &["leaves", "oak_leaves"],
        Block::CoalOre => &["coal_ore", "coal_ore_deepslate", "coal_ore_stone"],
        Block::IronOre => &["iron_ore", "iron_ore_deepslate", "iron_ore_stone"],
        Block::CopperOre => &["copper_ore", "copper_ore_deepslate", "copper_ore_stone"],
    }
}

fn block_color(block: Block) -> Color32 {
    match block {
        Block::Air => Color32::from_rgba_unmultiplied(0, 0, 0, 0),
        Block::CaveAir => Color32::from_rgba_unmultiplied(0, 0, 0, 0),
        Block::Workbench => Color32::from_rgb(130, 94, 58),
        Block::Furnace => Color32::from_rgb(116, 116, 116),
        Block::Coal => Color32::from_rgb(48, 48, 48),
        Block::IronIngot => Color32::from_rgb(206, 206, 214),
        Block::Torch => Color32::from_rgb(238, 186, 86),
        Block::Wood => Color32::from_rgb(166, 132, 89),
        Block::Stick => Color32::from_rgb(174, 142, 104),
        Block::Grass => Color32::from_rgb(72, 148, 46),
        Block::Dirt => Color32::from_rgb(122, 84, 46),
        Block::FarmlandDry => Color32::from_rgb(104, 72, 42),
        Block::FarmlandWet => Color32::from_rgb(78, 58, 36),
        Block::Stone => Color32::from_rgb(120, 120, 120),
        Block::Sand => Color32::from_rgb(217, 204, 128),
        Block::Water => Color32::from_rgb(46, 107, 199),
        Block::Bedrock => Color32::from_rgb(38, 32, 32),
        Block::Log => Color32::from_rgb(115, 77, 46),
        Block::LogBottom => Color32::from_rgb(145, 112, 72),
        Block::Leaves => Color32::from_rgb(46, 140, 56),
        Block::CoalOre => Color32::from_rgb(84, 84, 84),
        Block::IronOre => Color32::from_rgb(184, 135, 98),
        Block::CopperOre => Color32::from_rgb(168, 100, 66),
    }
}

fn block_display_name(block: Block) -> &'static str {
    match block {
        Block::Air | Block::CaveAir => "Air",
        Block::Workbench => "Crafting Table",
        Block::Furnace => "Furnace",
        Block::Coal => "Coal",
        Block::IronIngot => "Iron Ingot",
        Block::Torch => "Torch",
        Block::Wood => "Oak Planks",
        Block::Stick => "Stick",
        Block::Grass => "Grass Block",
        Block::Dirt => "Dirt",
        Block::FarmlandDry => "Farmland",
        Block::FarmlandWet => "Farmland (Wet)",
        Block::Stone => "Stone",
        Block::Sand => "Sand",
        Block::Water => "Water",
        Block::Bedrock => "Bedrock",
        Block::Log => "Oak Log",
        Block::LogBottom => "Oak Log Top",
        Block::Leaves => "Oak Leaves",
        Block::CoalOre => "Coal Ore",
        Block::IronOre => "Iron Ore",
        Block::CopperOre => "Copper Ore",
    }
}

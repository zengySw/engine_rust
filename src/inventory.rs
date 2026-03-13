use egui::{self, Align2, Color32, FontId, RichText, Stroke};
use crate::world::block::Block;

#[derive(Clone, Copy)]
pub struct Slot {
    pub block: Block,
    pub count: u16,
}

impl Slot {
    pub fn empty() -> Self {
        Self { block: Block::Air, count: 0 }
    }

    pub fn new(block: Block, count: u16) -> Self {
        Self { block, count }
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0 || self.block == Block::Air
    }
}

pub struct Inventory {
    pub open: bool,
    pub selected: usize,
    pub hotbar: [Slot; 9],
    pub grid:   [Slot; 27],
}

impl Inventory {
    pub fn new() -> Self {
        let mut inv = Self {
            open: false,
            selected: 0,
            hotbar: [Slot::empty(); 9],
            grid:   [Slot::empty(); 27],
        };

        inv.hotbar[0] = Slot::new(Block::Grass, 64);
        inv.hotbar[1] = Slot::new(Block::Dirt, 64);
        inv.hotbar[2] = Slot::new(Block::Stone, 64);
        inv.hotbar[3] = Slot::new(Block::Sand, 64);
        inv.hotbar[4] = Slot::new(Block::Water, 64);
        inv.hotbar[5] = Slot::new(Block::Log, 32);
        inv.hotbar[6] = Slot::new(Block::Leaves, 32);
        inv.hotbar[7] = Slot::new(Block::Bedrock, 64);

        inv.grid[0] = Slot::new(Block::Stone, 64);
        inv.grid[1] = Slot::new(Block::Dirt, 64);
        inv.grid[2] = Slot::new(Block::Sand, 64);
        inv.grid[3] = Slot::new(Block::Log, 32);
        inv.grid[4] = Slot::new(Block::Leaves, 32);

        inv
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    pub fn selected_block(&self) -> Option<Block> {
        let slot = self.hotbar[self.selected];
        if slot.is_empty() {
            None
        } else {
            Some(slot.block)
        }
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

    pub fn draw_hotbar(&mut self, ctx: &egui::Context) {
        let screen = ctx.screen_rect();
        let slot_size = 28.0;
        let spacing = 4.0;
        let total_w = slot_size * 9.0 + spacing * 8.0 + 10.0 * 2.0;
        let total_h = slot_size + 10.0 * 2.0;
        let pos = egui::pos2(
            screen.center().x - total_w / 2.0,
            screen.bottom() - total_h - 12.0,
        );

        egui::Area::new("hotbar".into())
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                let frame = egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(30, 30, 30, 220))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(90, 90, 90)))
                    .inner_margin(egui::Margin::same(10.0));

                frame.show(ui, |ui| {
                    let mut style = (**ui.style()).clone();
                    style.spacing.item_spacing = egui::vec2(spacing, 0.0);
                    ui.set_style(style);

                    ui.horizontal(|ui| {
                        for i in 0..9 {
                            let resp = paint_slot(ui, &self.hotbar[i], i == self.selected, slot_size);
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
        let bg_layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("inv_bg"));
        ctx.layer_painter(bg_layer)
            .rect_filled(screen, 0.0, Color32::from_black_alpha(170));

        let panel_size = egui::vec2(420.0, 320.0);
        let panel_rect = egui::Rect::from_center_size(screen.center(), panel_size);

        egui::Area::new("inventory_panel".into())
            .order(egui::Order::Foreground)
            .fixed_pos(panel_rect.min)
            .show(ctx, |ui| {
                ui.set_min_size(panel_size);
                let frame = egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(30, 30, 30, 230))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(90, 90, 90)))
                    .inner_margin(egui::Margin::same(16.0));

                frame.show(ui, |ui| {
                    let mut style = (**ui.style()).clone();
                    style.spacing.item_spacing = egui::vec2(4.0, 4.0);
                    ui.set_style(style);

                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("Inventory").size(22.0).strong());
                        ui.add_space(6.0);
                    });

                    let slot_size = 26.0;
                    for row in 0..3 {
                        ui.horizontal(|ui| {
                            for col in 0..9 {
                                let idx = row * 9 + col;
                                paint_slot(ui, &self.grid[idx], false, slot_size);
                            }
                        });
                    }

                    ui.add_space(10.0);
                    ui.label(RichText::new("Hotbar").size(18.0));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        for i in 0..9 {
                            let resp = paint_slot(ui, &self.hotbar[i], i == self.selected, slot_size);
                            if resp.clicked() {
                                self.select_hotbar_slot(i);
                            }
                        }
                    });
                });
            });
    }
}

fn paint_slot(
    ui: &mut egui::Ui,
    slot: &Slot,
    selected: bool,
    size: f32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let bg = Color32::from_rgb(55, 55, 55);
    let stroke = if selected {
        Stroke::new(2.0, Color32::from_rgb(200, 200, 200))
    } else {
        Stroke::new(1.0, Color32::from_rgb(20, 20, 20))
    };
    let rounding = egui::Rounding::same(2.0);
    let painter = ui.painter();
    painter.rect_filled(rect, rounding, bg);
    painter.rect_stroke(rect, rounding, stroke);

    if !slot.is_empty() {
        let item_rect = rect.shrink(4.0);
        painter.rect_filled(item_rect, egui::Rounding::same(1.0), block_color(slot.block));
        if slot.count > 1 {
            painter.text(
                rect.right_bottom() - egui::vec2(2.0, 2.0),
                Align2::RIGHT_BOTTOM,
                slot.count.to_string(),
                FontId::proportional(12.0),
                Color32::WHITE,
            );
        }
    }

    resp
}

fn block_color(block: Block) -> Color32 {
    match block {
        Block::Air => Color32::from_rgba_unmultiplied(0, 0, 0, 0),
        Block::Grass => Color32::from_rgb(72, 148, 46),
        Block::Dirt => Color32::from_rgb(122, 84, 46),
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

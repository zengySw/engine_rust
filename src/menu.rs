use egui::{self, Align, Align2, Color32, FontId, RichText, Stroke};

#[derive(Clone, Copy)]
pub struct Settings {
    pub render_dist: i32,
    pub fly_speed: f32,
    pub mouse_sens: f32,
    pub vsync: bool,
    pub show_fps: bool,
    pub ambient_boost: f32,
    pub sun_softness: f32,
    pub fog_density: f32,
    pub exposure: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Resume,
    RegenerateWorld,
    Exit,
    None,
}

pub struct EscMenu {
    pub open: bool,
    pub settings: Settings,
    page: MenuPage,
    graphics_draft: Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuPage {
    Main,
    Graphics,
}

impl EscMenu {
    pub fn new(settings: Settings) -> Self {
        Self {
            open: false,
            settings,
            page: MenuPage::Main,
            graphics_draft: settings,
        }
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.page = MenuPage::Main;
            self.graphics_draft = self.settings;
        }
    }

    pub fn draw(&mut self, ctx: &egui::Context) -> MenuAction {
        if !self.open {
            return MenuAction::None;
        }

        let mut action = MenuAction::None;
        let screen = ctx.screen_rect();

        draw_mc_backdrop(ctx, screen);

        let panel_size = match self.page {
            MenuPage::Main => egui::vec2(460.0, 430.0),
            MenuPage::Graphics => egui::vec2(520.0, 620.0),
        };
        let panel_rect = egui::Rect::from_center_size(screen.center(), panel_size);

        egui::Area::new("pause_panel".into())
            .order(egui::Order::Foreground)
            .fixed_pos(panel_rect.min)
            .show(ctx, |ui| {
                ui.set_min_size(panel_size);

                draw_title(ui, panel_size.x, self.page);
                ui.add_space(12.0);

                match self.page {
                    MenuPage::Main => {
                        let button_w = 360.0;
                        if mc_button(ui, "Back to Game", button_w).clicked() {
                            action = MenuAction::Resume;
                        }
                        ui.add_space(6.0);
                        if mc_button(ui, "Graphics Settings...", button_w).clicked() {
                            self.graphics_draft = self.settings;
                            self.page = MenuPage::Graphics;
                        }
                        ui.add_space(6.0);
                        if mc_button(ui, "Regenerate World", button_w).clicked() {
                            action = MenuAction::RegenerateWorld;
                        }
                        ui.add_space(6.0);
                        if mc_button(ui, "Exit", button_w).clicked() {
                            action = MenuAction::Exit;
                        }

                        ui.add_space(16.0);
                        draw_options_header(ui, button_w, "Controls");
                        ui.add_space(8.0);
                        mc_slider_f32(
                            ui,
                            button_w,
                            "Fly Speed",
                            &mut self.settings.fly_speed,
                            1.0..=40.0,
                            1,
                        );
                        ui.add_space(6.0);
                        mc_slider_f32(
                            ui,
                            button_w,
                            "Mouse Sensitivity",
                            &mut self.settings.mouse_sens,
                            0.05..=1.0,
                            2,
                        );
                    }
                    MenuPage::Graphics => {
                        let button_w = 420.0;
                        draw_options_header(ui, button_w, "Graphics");
                        ui.add_space(8.0);
                        mc_slider_i32(
                            ui,
                            button_w,
                            "Render Distance",
                            &mut self.graphics_draft.render_dist,
                            2..=32,
                        );
                        ui.add_space(6.0);
                        mc_slider_f32(
                            ui,
                            button_w,
                            "Ambient Light",
                            &mut self.graphics_draft.ambient_boost,
                            0.70..=1.60,
                            2,
                        );
                        ui.add_space(6.0);
                        mc_slider_f32(
                            ui,
                            button_w,
                            "Sun Softness",
                            &mut self.graphics_draft.sun_softness,
                            0.00..=0.80,
                            2,
                        );
                        ui.add_space(6.0);
                        mc_slider_f32(
                            ui,
                            button_w,
                            "Fog Density",
                            &mut self.graphics_draft.fog_density,
                            0.40..=1.60,
                            2,
                        );
                        ui.add_space(6.0);
                        mc_slider_f32(
                            ui,
                            button_w,
                            "Exposure",
                            &mut self.graphics_draft.exposure,
                            0.80..=1.40,
                            2,
                        );
                        ui.add_space(6.0);
                        mc_toggle(ui, "VSync", &mut self.graphics_draft.vsync, button_w);
                        ui.add_space(6.0);
                        mc_toggle(ui, "Show FPS", &mut self.graphics_draft.show_fps, button_w);
                        ui.add_space(12.0);
                        if mc_button(ui, "Подтвердить", button_w).clicked() {
                            self.settings = self.graphics_draft;
                            self.page = MenuPage::Main;
                        }
                        ui.add_space(6.0);
                        if mc_button(ui, "Отмена", button_w).clicked() {
                            self.graphics_draft = self.settings;
                            self.page = MenuPage::Main;
                        }
                    }
                }
            });

        action
    }
}

fn draw_mc_backdrop(ctx: &egui::Context, screen: egui::Rect) {
    let bg_layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("pause_bg"));
    let painter = ctx.layer_painter(bg_layer);

    painter.rect_filled(screen, 0.0, Color32::from_rgb(36, 31, 24));

    let tile = 24.0;
    let x0 = (screen.left() / tile).floor() as i32 - 1;
    let x1 = (screen.right() / tile).ceil() as i32 + 1;
    let y0 = (screen.top() / tile).floor() as i32 - 1;
    let y1 = (screen.bottom() / tile).ceil() as i32 + 1;

    for ty in y0..=y1 {
        for tx in x0..=x1 {
            let h = hash2(tx, ty);
            let n = (h & 0x0f) as u8;
            let c = Color32::from_rgb(60 + n / 2, 47 + n / 3, 31 + n / 4);
            let min = egui::pos2(tx as f32 * tile, ty as f32 * tile);
            let max = egui::pos2(min.x + tile + 1.0, min.y + tile + 1.0);
            painter.rect_filled(egui::Rect::from_min_max(min, max), 0.0, c);

            if ((h >> 5) & 0b11) == 0 {
                let spot = egui::Rect::from_min_max(
                    min + egui::vec2(6.0, 6.0),
                    min + egui::vec2(11.0, 11.0),
                );
                painter.rect_filled(spot, 0.0, Color32::from_rgba_unmultiplied(25, 20, 15, 120));
            }
        }
    }

    let top_h = screen.height() * 0.28;
    let top = egui::Rect::from_min_max(screen.min, egui::pos2(screen.max.x, screen.min.y + top_h));
    painter.rect_filled(top, 0.0, Color32::from_rgba_unmultiplied(140, 180, 220, 26));

    painter.rect_filled(screen, 0.0, Color32::from_black_alpha(126));
}

fn draw_title(ui: &mut egui::Ui, width: f32, page: MenuPage) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 96.0), egui::Sense::hover());
    let painter = ui.painter();

    let title = "MY ENGINE";
    let title_font = FontId::proportional(46.0);

    painter.text(
        rect.center() + egui::vec2(3.0, 4.0),
        Align2::CENTER_CENTER,
        title,
        title_font.clone(),
        Color32::from_rgba_unmultiplied(0, 0, 0, 185),
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        title,
        title_font,
        Color32::from_rgb(238, 238, 238),
    );

    painter.text(
        egui::pos2(rect.center().x + 132.0, rect.center().y - 24.0),
        Align2::CENTER_CENTER,
        match page {
            MenuPage::Main => "PAUSED",
            MenuPage::Graphics => "GRAPHICS",
        },
        FontId::proportional(18.0),
        Color32::from_rgb(255, 220, 85),
    );
}

fn draw_options_header(ui: &mut egui::Ui, width: f32, text: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, Color32::from_rgb(52, 52, 52));
    p.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_rgb(16, 16, 16)));
    p.line_segment(
        [rect.left_top() + egui::vec2(1.0, 1.0), rect.right_top() + egui::vec2(-1.0, 1.0)],
        Stroke::new(1.0, Color32::from_rgb(106, 106, 106)),
    );
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(20.0),
        Color32::from_rgb(240, 240, 240),
    );
}

fn mc_button(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    let size = egui::vec2(width, 38.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let p = ui.painter();

    let (bg, top, bottom, text_color) = if response.is_pointer_button_down_on() {
        (
            Color32::from_rgb(86, 86, 86),
            Color32::from_rgb(72, 72, 72),
            Color32::from_rgb(32, 32, 32),
            Color32::from_rgb(236, 236, 120),
        )
    } else if response.hovered() {
        (
            Color32::from_rgb(118, 118, 118),
            Color32::from_rgb(162, 162, 162),
            Color32::from_rgb(38, 38, 38),
            Color32::from_rgb(255, 255, 175),
        )
    } else {
        (
            Color32::from_rgb(98, 98, 98),
            Color32::from_rgb(142, 142, 142),
            Color32::from_rgb(34, 34, 34),
            Color32::from_rgb(232, 232, 232),
        )
    };

    p.rect_filled(rect, 0.0, bg);
    p.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_rgb(18, 18, 18)));
    p.line_segment(
        [rect.left_top() + egui::vec2(1.0, 1.0), rect.right_top() + egui::vec2(-1.0, 1.0)],
        Stroke::new(1.0, top),
    );
    p.line_segment(
        [rect.left_bottom() + egui::vec2(1.0, -1.0), rect.right_bottom() + egui::vec2(-1.0, -1.0)],
        Stroke::new(1.0, bottom),
    );

    let text_pos = if response.is_pointer_button_down_on() {
        rect.center() + egui::vec2(0.0, 1.0)
    } else {
        rect.center()
    };

    p.text(
        text_pos,
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(21.0),
        text_color,
    );

    response
}

fn mc_option_row(
    ui: &mut egui::Ui,
    width: f32,
    label: &str,
    add_content: impl FnOnce(&mut egui::Ui),
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 38.0), egui::Sense::hover());
    let p = ui.painter();

    p.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(28, 28, 28, 180));
    p.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_rgb(12, 12, 12)));
    p.line_segment(
        [rect.left_top() + egui::vec2(1.0, 1.0), rect.right_top() + egui::vec2(-1.0, 1.0)],
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 28)),
    );

    ui.allocate_ui_at_rect(rect.shrink2(egui::vec2(8.0, 7.0)), |row| {
        row.horizontal(|row| {
            row.with_layout(egui::Layout::left_to_right(Align::Center), |row| {
                row.label(RichText::new(label).size(16.0).color(Color32::from_rgb(220, 220, 220)));
                row.add_space(8.0);
                add_content(row);
            });
        });
    });
}

fn mc_slider_i32(
    ui: &mut egui::Ui,
    width: f32,
    label: &str,
    value: &mut i32,
    range: std::ops::RangeInclusive<i32>,
) {
    mc_option_row(ui, width, &format!("{label}: {value}"), |row| {
        row.add_sized([180.0, 18.0], egui::Slider::new(value, range).show_value(false));
    });
}

fn mc_slider_f32(
    ui: &mut egui::Ui,
    width: f32,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    precision: usize,
) {
    let text = format!("{label}: {value:.prec$}", prec = precision);
    mc_option_row(ui, width, &text, |row| {
        row.add_sized([180.0, 18.0], egui::Slider::new(value, range).show_value(false));
    });
}

fn mc_toggle(ui: &mut egui::Ui, label: &str, value: &mut bool, width: f32) {
    let state = if *value { "ON" } else { "OFF" };
    if mc_button(ui, &format!("{label}: {state}"), width).clicked() {
        *value = !*value;
    }
}

#[inline]
fn hash2(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^ (h >> 16)
}

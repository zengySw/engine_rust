use egui::{self, Color32, RichText, Stroke};

pub struct Settings {
    pub render_dist: i32,
    pub fly_speed:   f32,
    pub mouse_sens:  f32,
    pub vsync:       bool,
    pub show_fps:    bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Resume,
    RegenerateWorld,
    Exit,
    None,
}

pub struct EscMenu {
    pub open:     bool,
    pub settings: Settings,
}

impl EscMenu {
    pub fn new(settings: Settings) -> Self {
        Self { open: false, settings }
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    pub fn draw(&mut self, ctx: &egui::Context) -> MenuAction {
        if !self.open {
            return MenuAction::None;
        }

        let mut action = MenuAction::None;

        let screen = ctx.screen_rect();
        let bg_layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("pause_bg"));
        ctx.layer_painter(bg_layer)
            .rect_filled(screen, 0.0, Color32::from_black_alpha(170));

        let panel_size = egui::vec2(360.0, 360.0);
        let panel_rect = egui::Rect::from_center_size(screen.center(), panel_size);

        egui::Area::new("pause_panel".into())
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
                    style.spacing.item_spacing = egui::vec2(0.0, 10.0);
                    style.spacing.button_padding = egui::vec2(12.0, 6.0);
                    style.visuals.window_rounding = egui::Rounding::same(2.0);
                    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(60, 60, 60);
                    style.visuals.widgets.inactive.bg_stroke =
                        Stroke::new(1.0, Color32::from_rgb(20, 20, 20));
                    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(80, 80, 80);
                    style.visuals.widgets.hovered.bg_stroke =
                        Stroke::new(1.0, Color32::from_rgb(120, 120, 120));
                    style.visuals.widgets.active.bg_fill = Color32::from_rgb(100, 100, 100);
                    style.visuals.widgets.active.bg_stroke =
                        Stroke::new(1.0, Color32::from_rgb(140, 140, 140));
                    style.visuals.widgets.inactive.fg_stroke.color = Color32::from_rgb(230, 230, 230);
                    style.visuals.widgets.hovered.fg_stroke.color = Color32::from_rgb(255, 255, 255);
                    style.visuals.widgets.active.fg_stroke.color = Color32::from_rgb(255, 255, 255);
                    ui.set_style(style);

                    ui.vertical_centered(|ui| {
                        ui.add_space(4.0);
                        ui.label(RichText::new("Game Menu").size(24.0).strong());
                        ui.add_space(8.0);
                    });

                    let button_w = 280.0;
                    if mc_button(ui, "Back to Game", button_w).clicked() {
                        action = MenuAction::Resume;
                    }
                    if mc_button(ui, "Regenerate World", button_w).clicked() {
                        action = MenuAction::RegenerateWorld;
                    }
                    if mc_button(ui, "Exit", button_w).clicked() {
                        action = MenuAction::Exit;
                    }

                    ui.add_space(8.0);
                    ui.separator();

                    ui.label(RichText::new("Options").size(18.0));
                    ui.add_space(4.0);

                    ui.add_sized(
                        [button_w, 24.0],
                        egui::Slider::new(&mut self.settings.render_dist, 2..=32)
                            .text("Render distance"),
                    );
                    ui.add_sized(
                        [button_w, 24.0],
                        egui::Slider::new(&mut self.settings.fly_speed, 1.0..=40.0)
                            .text("Fly speed"),
                    );
                    ui.add_sized(
                        [button_w, 24.0],
                        egui::Slider::new(&mut self.settings.mouse_sens, 0.05..=1.0)
                            .text("Mouse sensitivity"),
                    );
                    ui.checkbox(&mut self.settings.vsync, "VSync (restart)");
                    ui.checkbox(&mut self.settings.show_fps, "Show FPS");
                });
            });

        action
    }
}

fn mc_button(ui: &mut egui::Ui, text: &str, width: f32) -> egui::Response {
    ui.add_sized(
        [width, 32.0],
        egui::Button::new(RichText::new(text).size(18.0)),
    )
}

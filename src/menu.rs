use egui::{self, Align, Align2, Color32, FontId, RichText, Stroke};

#[derive(Clone, Copy)]
pub struct Settings {
    pub render_dist: i32,
    pub mouse_sens: f32,
    pub fov: f32,
    pub vsync: bool,
    pub ray_tracing: bool,
    pub show_fps: bool,
    pub master_volume: f32,
    pub music_volume: f32,
    pub ambient_volume: f32,
    pub sfx_volume: f32,
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
    difficulty: Difficulty,
    language: UiLanguage,
    controls_invert_mouse: bool,
    controls_auto_jump: bool,
    chat_colors: bool,
    chat_links: bool,
    chat_opacity: f32,
    skin_hat: bool,
    skin_jacket: bool,
    skin_left_sleeve: bool,
    skin_right_sleeve: bool,
    resource_pack_override: bool,
    accessibility_high_contrast: bool,
    accessibility_screen_effects: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuPage {
    Main,
    Options,
    MusicAndSounds,
    VideoSettings,
    Controls,
    Language,
    ChatSettings,
    ResourcePacks,
    Accessibility,
    SkinCustomization,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

impl Difficulty {
    fn next(self) -> Self {
        match self {
            Difficulty::Peaceful => Difficulty::Easy,
            Difficulty::Easy => Difficulty::Normal,
            Difficulty::Normal => Difficulty::Hard,
            Difficulty::Hard => Difficulty::Peaceful,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Difficulty::Peaceful => "Peaceful",
            Difficulty::Easy => "Easy",
            Difficulty::Normal => "Normal",
            Difficulty::Hard => "Hard",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UiLanguage {
    English,
    Russian,
    Ukrainian,
}

impl UiLanguage {
    fn next(self) -> Self {
        match self {
            UiLanguage::English => UiLanguage::Russian,
            UiLanguage::Russian => UiLanguage::Ukrainian,
            UiLanguage::Ukrainian => UiLanguage::English,
        }
    }

    fn label(self) -> &'static str {
        match self {
            UiLanguage::English => "English (US)",
            UiLanguage::Russian => "Russian",
            UiLanguage::Ukrainian => "Ukrainian",
        }
    }
}

impl EscMenu {
    pub fn new(settings: Settings) -> Self {
        Self {
            open: false,
            settings,
            page: MenuPage::Main,
            difficulty: Difficulty::Normal,
            language: UiLanguage::English,
            controls_invert_mouse: false,
            controls_auto_jump: false,
            chat_colors: true,
            chat_links: true,
            chat_opacity: 0.90,
            skin_hat: true,
            skin_jacket: true,
            skin_left_sleeve: true,
            skin_right_sleeve: true,
            resource_pack_override: false,
            accessibility_high_contrast: false,
            accessibility_screen_effects: 1.0,
        }
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.page = MenuPage::Main;
        }
    }

    pub fn draw(&mut self, ctx: &egui::Context) -> MenuAction {
        if !self.open {
            return MenuAction::None;
        }

        let mut action = MenuAction::None;
        let screen = ctx.screen_rect();
        draw_pause_overlay(ctx, screen);

        let panel_size = panel_size_for_page(self.page);
        let panel_rect = egui::Rect::from_center_size(screen.center(), panel_size);

        egui::Area::new("pause_panel".into())
            .order(egui::Order::Foreground)
            .fixed_pos(panel_rect.min)
            .show(ctx, |ui| {
                ui.set_min_size(panel_size);
                draw_title(ui, panel_size.x, self.page);
                ui.add_space(10.0);

                match self.page {
                    MenuPage::Main => self.draw_main(ui, &mut action),
                    MenuPage::Options => self.draw_options(ui),
                    MenuPage::MusicAndSounds => self.draw_music_and_sounds(ui),
                    MenuPage::VideoSettings => self.draw_video_settings(ui),
                    MenuPage::Controls => self.draw_controls(ui),
                    MenuPage::Language => self.draw_language(ui),
                    MenuPage::ChatSettings => self.draw_chat(ui),
                    MenuPage::ResourcePacks => self.draw_resource_packs(ui),
                    MenuPage::Accessibility => self.draw_accessibility(ui),
                    MenuPage::SkinCustomization => self.draw_skin(ui),
                }
            });

        action
    }

    fn draw_main(&mut self, ui: &mut egui::Ui, action: &mut MenuAction) {
        let button_w = 360.0;
        if mc_button(ui, "Back to Game", button_w).clicked() {
            *action = MenuAction::Resume;
        }
        ui.add_space(6.0);
        if mc_button(ui, "Options...", button_w).clicked() {
            self.page = MenuPage::Options;
        }
        ui.add_space(6.0);
        if mc_button(ui, "Regenerate World", button_w).clicked() {
            *action = MenuAction::RegenerateWorld;
        }
        ui.add_space(6.0);
        if mc_button(ui, "Exit", button_w).clicked() {
            *action = MenuAction::Exit;
        }
    }

    fn draw_options(&mut self, ui: &mut egui::Ui) {
        let wide_w = 432.0;
        draw_options_header(ui, wide_w, "Options");
        ui.add_space(8.0);

        mc_slider_f32(ui, wide_w, "FOV", &mut self.settings.fov, 50.0..=110.0, 0);
        ui.add_space(6.0);
        if mc_button(
            ui,
            &format!("Difficulty: {}", self.difficulty.label()),
            wide_w,
        )
        .clicked()
        {
            self.difficulty = self.difficulty.next();
        }
        ui.add_space(10.0);

        let pair_w = (wide_w - 8.0) * 0.5;
        ui.horizontal(|ui| {
            if mc_button(ui, "Skin Customization...", pair_w).clicked() {
                self.page = MenuPage::SkinCustomization;
            }
            ui.add_space(8.0);
            if mc_button(ui, "Music & Sounds...", pair_w).clicked() {
                self.page = MenuPage::MusicAndSounds;
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if mc_button(ui, "Video Settings...", pair_w).clicked() {
                self.page = MenuPage::VideoSettings;
            }
            ui.add_space(8.0);
            if mc_button(ui, "Controls...", pair_w).clicked() {
                self.page = MenuPage::Controls;
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if mc_button(ui, "Language...", pair_w).clicked() {
                self.page = MenuPage::Language;
            }
            ui.add_space(8.0);
            if mc_button(ui, "Chat Settings...", pair_w).clicked() {
                self.page = MenuPage::ChatSettings;
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if mc_button(ui, "Resource Packs...", pair_w).clicked() {
                self.page = MenuPage::ResourcePacks;
            }
            ui.add_space(8.0);
            if mc_button(ui, "Accessibility Settings...", pair_w).clicked() {
                self.page = MenuPage::Accessibility;
            }
        });
        ui.add_space(12.0);
        if mc_button(ui, "Done", wide_w).clicked() {
            self.page = MenuPage::Main;
        }
    }

    fn draw_music_and_sounds(&mut self, ui: &mut egui::Ui) {
        let button_w = 432.0;
        draw_options_header(ui, button_w, "Music & Sounds");
        ui.add_space(8.0);
        mc_slider_percent(ui, button_w, "Master Volume", &mut self.settings.master_volume);
        ui.add_space(6.0);
        mc_slider_percent(ui, button_w, "Music", &mut self.settings.music_volume);
        ui.add_space(6.0);
        mc_slider_percent(ui, button_w, "Ambient", &mut self.settings.ambient_volume);
        ui.add_space(6.0);
        mc_slider_percent(ui, button_w, "Effects", &mut self.settings.sfx_volume);
        ui.add_space(12.0);
        if mc_button(ui, "Done", button_w).clicked() {
            self.page = MenuPage::Options;
        }
    }

    fn draw_video_settings(&mut self, ui: &mut egui::Ui) {
        let button_w = 432.0;
        draw_options_header(ui, button_w, "Video Settings");
        ui.add_space(8.0);
        mc_slider_i32(
            ui,
            button_w,
            "Render Distance",
            &mut self.settings.render_dist,
            2..=32,
        );
        ui.add_space(6.0);
        mc_slider_f32(
            ui,
            button_w,
            "Ambient Light",
            &mut self.settings.ambient_boost,
            0.70..=1.60,
            2,
        );
        ui.add_space(6.0);
        mc_slider_f32(
            ui,
            button_w,
            "Sun Softness",
            &mut self.settings.sun_softness,
            0.00..=0.80,
            2,
        );
        ui.add_space(6.0);
        mc_slider_f32(
            ui,
            button_w,
            "Fog Density",
            &mut self.settings.fog_density,
            0.40..=1.60,
            2,
        );
        ui.add_space(6.0);
        mc_slider_f32(
            ui,
            button_w,
            "Exposure",
            &mut self.settings.exposure,
            0.80..=1.40,
            2,
        );
        ui.add_space(6.0);
        mc_toggle(ui, "VSync", &mut self.settings.vsync, button_w);
        ui.add_space(6.0);
        mc_toggle(ui, "RTX", &mut self.settings.ray_tracing, button_w);
        ui.add_space(6.0);
        mc_toggle(ui, "Show FPS", &mut self.settings.show_fps, button_w);
        ui.add_space(12.0);
        if mc_button(ui, "Done", button_w).clicked() {
            self.page = MenuPage::Options;
        }
    }

    fn draw_controls(&mut self, ui: &mut egui::Ui) {
        let button_w = 432.0;
        draw_options_header(ui, button_w, "Controls");
        ui.add_space(8.0);
        mc_slider_f32(
            ui,
            button_w,
            "Mouse Sensitivity",
            &mut self.settings.mouse_sens,
            0.05..=1.00,
            2,
        );
        ui.add_space(6.0);
        mc_toggle(
            ui,
            "Invert Mouse",
            &mut self.controls_invert_mouse,
            button_w,
        );
        ui.add_space(6.0);
        mc_toggle(ui, "Auto Jump", &mut self.controls_auto_jump, button_w);
        ui.add_space(12.0);
        if mc_button(ui, "Done", button_w).clicked() {
            self.page = MenuPage::Options;
        }
    }

    fn draw_language(&mut self, ui: &mut egui::Ui) {
        let button_w = 432.0;
        draw_options_header(ui, button_w, "Language");
        ui.add_space(8.0);
        if mc_button(
            ui,
            &format!("Language: {}", self.language.label()),
            button_w,
        )
        .clicked()
        {
            self.language = self.language.next();
        }
        ui.add_space(6.0);
        draw_hint_row(
            ui,
            button_w,
            "Language switch currently affects menu selection only.",
        );
        ui.add_space(12.0);
        if mc_button(ui, "Done", button_w).clicked() {
            self.page = MenuPage::Options;
        }
    }

    fn draw_chat(&mut self, ui: &mut egui::Ui) {
        let button_w = 432.0;
        draw_options_header(ui, button_w, "Chat Settings");
        ui.add_space(8.0);
        mc_toggle(ui, "Chat Colors", &mut self.chat_colors, button_w);
        ui.add_space(6.0);
        mc_toggle(ui, "Web Links", &mut self.chat_links, button_w);
        ui.add_space(6.0);
        mc_slider_f32(
            ui,
            button_w,
            "Chat Opacity",
            &mut self.chat_opacity,
            0.10..=1.00,
            2,
        );
        ui.add_space(12.0);
        if mc_button(ui, "Done", button_w).clicked() {
            self.page = MenuPage::Options;
        }
    }

    fn draw_resource_packs(&mut self, ui: &mut egui::Ui) {
        let button_w = 432.0;
        draw_options_header(ui, button_w, "Resource Packs");
        ui.add_space(8.0);
        mc_toggle(
            ui,
            "Pack Override",
            &mut self.resource_pack_override,
            button_w,
        );
        ui.add_space(6.0);
        draw_hint_row(ui, button_w, "Drop packs into src/assets for now.");
        ui.add_space(12.0);
        if mc_button(ui, "Done", button_w).clicked() {
            self.page = MenuPage::Options;
        }
    }

    fn draw_accessibility(&mut self, ui: &mut egui::Ui) {
        let button_w = 432.0;
        draw_options_header(ui, button_w, "Accessibility");
        ui.add_space(8.0);
        mc_toggle(
            ui,
            "High Contrast",
            &mut self.accessibility_high_contrast,
            button_w,
        );
        ui.add_space(6.0);
        mc_slider_f32(
            ui,
            button_w,
            "Screen Effects",
            &mut self.accessibility_screen_effects,
            0.00..=1.00,
            2,
        );
        ui.add_space(12.0);
        if mc_button(ui, "Done", button_w).clicked() {
            self.page = MenuPage::Options;
        }
    }

    fn draw_skin(&mut self, ui: &mut egui::Ui) {
        let button_w = 432.0;
        draw_options_header(ui, button_w, "Skin Customization");
        ui.add_space(8.0);
        mc_toggle(ui, "Hat", &mut self.skin_hat, button_w);
        ui.add_space(6.0);
        mc_toggle(ui, "Jacket", &mut self.skin_jacket, button_w);
        ui.add_space(6.0);
        mc_toggle(ui, "Left Sleeve", &mut self.skin_left_sleeve, button_w);
        ui.add_space(6.0);
        mc_toggle(ui, "Right Sleeve", &mut self.skin_right_sleeve, button_w);
        ui.add_space(12.0);
        if mc_button(ui, "Done", button_w).clicked() {
            self.page = MenuPage::Options;
        }
    }
}

fn panel_size_for_page(page: MenuPage) -> egui::Vec2 {
    match page {
        MenuPage::Main => egui::vec2(460.0, 310.0),
        MenuPage::Options => egui::vec2(560.0, 560.0),
        MenuPage::MusicAndSounds => egui::vec2(560.0, 430.0),
        MenuPage::VideoSettings => egui::vec2(560.0, 650.0),
        MenuPage::Controls => egui::vec2(560.0, 380.0),
        MenuPage::Language => egui::vec2(560.0, 320.0),
        MenuPage::ChatSettings => egui::vec2(560.0, 400.0),
        MenuPage::ResourcePacks => egui::vec2(560.0, 320.0),
        MenuPage::Accessibility => egui::vec2(560.0, 360.0),
        MenuPage::SkinCustomization => egui::vec2(560.0, 430.0),
    }
}

fn draw_pause_overlay(ctx: &egui::Context, screen: egui::Rect) {
    let bg_layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("pause_bg"));
    let painter = ctx.layer_painter(bg_layer);
    painter.rect_filled(screen, 0.0, Color32::from_black_alpha(72));
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
        page_tag(page),
        FontId::proportional(18.0),
        Color32::from_rgb(255, 220, 85),
    );
}

fn page_tag(page: MenuPage) -> &'static str {
    match page {
        MenuPage::Main => "PAUSED",
        MenuPage::Options => "OPTIONS",
        MenuPage::MusicAndSounds => "MUSIC & SOUNDS",
        MenuPage::VideoSettings => "VIDEO",
        MenuPage::Controls => "CONTROLS",
        MenuPage::Language => "LANGUAGE",
        MenuPage::ChatSettings => "CHAT",
        MenuPage::ResourcePacks => "PACKS",
        MenuPage::Accessibility => "ACCESSIBILITY",
        MenuPage::SkinCustomization => "SKIN",
    }
}

fn draw_options_header(ui: &mut egui::Ui, width: f32, text: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 32.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, Color32::from_rgb(52, 52, 52));
    p.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_rgb(16, 16, 16)));
    p.line_segment(
        [
            rect.left_top() + egui::vec2(1.0, 1.0),
            rect.right_top() + egui::vec2(-1.0, 1.0),
        ],
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

fn draw_hint_row(ui: &mut egui::Ui, width: f32, text: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 34.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(24, 24, 24, 180));
    p.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_rgb(12, 12, 12)));
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(14.0),
        Color32::from_rgb(200, 200, 200),
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
        [
            rect.left_top() + egui::vec2(1.0, 1.0),
            rect.right_top() + egui::vec2(-1.0, 1.0),
        ],
        Stroke::new(1.0, top),
    );
    p.line_segment(
        [
            rect.left_bottom() + egui::vec2(1.0, -1.0),
            rect.right_bottom() + egui::vec2(-1.0, -1.0),
        ],
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
        [
            rect.left_top() + egui::vec2(1.0, 1.0),
            rect.right_top() + egui::vec2(-1.0, 1.0),
        ],
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 28)),
    );

    ui.allocate_ui_at_rect(rect.shrink2(egui::vec2(8.0, 7.0)), |row| {
        row.horizontal(|row| {
            row.with_layout(egui::Layout::left_to_right(Align::Center), |row| {
                row.label(
                    RichText::new(label)
                        .size(16.0)
                        .color(Color32::from_rgb(220, 220, 220)),
                );
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
        let min = *range.start();
        let max = *range.end();
        let mut vf = (*value).clamp(min, max) as f32;
        let resp = mc_rect_slider(row, &mut vf, min as f32..=max as f32, egui::vec2(180.0, 18.0));
        if resp.changed() {
            *value = vf.round().clamp(min as f32, max as f32) as i32;
        }
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
        let _ = mc_rect_slider(row, value, range, egui::vec2(180.0, 18.0));
    });
}

fn mc_slider_percent(ui: &mut egui::Ui, width: f32, label: &str, value: &mut f32) {
    let pct = (*value * 100.0).round().clamp(0.0, 100.0) as i32;
    mc_option_row(ui, width, &format!("{label}: {pct}%"), |row| {
        let _ = mc_rect_slider(row, value, 0.0..=1.0, egui::vec2(180.0, 18.0));
    });
}

fn mc_toggle(ui: &mut egui::Ui, label: &str, value: &mut bool, width: f32) {
    let state = if *value { "ON" } else { "OFF" };
    if mc_button(ui, &format!("{label}: {state}"), width).clicked() {
        *value = !*value;
    }
}

fn mc_rect_slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    size: egui::Vec2,
) -> egui::Response {
    let (min, max) = (*range.start(), *range.end());
    let span = (max - min).max(0.0001);
    *value = (*value).clamp(min, max);

    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let p = ui.painter();

    if response.clicked() || response.dragged() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let t = ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            let new_value = min + t * span;
            if (*value - new_value).abs() > f32::EPSILON {
                *value = new_value;
                response.mark_changed();
            }
        }
    }

    // Rectangular track.
    let track_bg = if response.dragged() {
        Color32::from_rgb(84, 84, 84)
    } else if response.hovered() {
        Color32::from_rgb(98, 98, 98)
    } else {
        Color32::from_rgb(72, 72, 72)
    };
    p.rect_filled(rect, 0.0, track_bg);
    p.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_rgb(16, 16, 16)));
    p.line_segment(
        [
            rect.left_top() + egui::vec2(1.0, 1.0),
            rect.right_top() + egui::vec2(-1.0, 1.0),
        ],
        Stroke::new(1.0, Color32::from_rgb(136, 136, 136)),
    );
    p.line_segment(
        [
            rect.left_bottom() + egui::vec2(1.0, -1.0),
            rect.right_bottom() + egui::vec2(-1.0, -1.0),
        ],
        Stroke::new(1.0, Color32::from_rgb(34, 34, 34)),
    );

    // Filled progress on track.
    let t = ((*value - min) / span).clamp(0.0, 1.0);
    let fill_w = (rect.width() * t).round();
    if fill_w > 0.5 {
        let fill = egui::Rect::from_min_size(rect.min, egui::vec2(fill_w, rect.height()));
        p.rect_filled(fill, 0.0, Color32::from_rgb(122, 122, 122));
    }

    // Rectangular draggable knob.
    let knob_w = 10.0;
    let knob_h = (rect.height() - 2.0).max(8.0);
    let knob_x = (rect.left() + rect.width() * t).clamp(rect.left() + knob_w * 0.5, rect.right() - knob_w * 0.5);
    let knob_rect = egui::Rect::from_center_size(
        egui::pos2(knob_x, rect.center().y),
        egui::vec2(knob_w, knob_h),
    );
    let knob_bg = if response.dragged() {
        Color32::from_rgb(188, 188, 188)
    } else if response.hovered() {
        Color32::from_rgb(172, 172, 172)
    } else {
        Color32::from_rgb(148, 148, 148)
    };
    p.rect_filled(knob_rect, 0.0, knob_bg);
    p.rect_stroke(knob_rect, 0.0, Stroke::new(1.0, Color32::from_rgb(24, 24, 24)));
    p.line_segment(
        [
            knob_rect.left_top() + egui::vec2(1.0, 1.0),
            knob_rect.right_top() + egui::vec2(-1.0, 1.0),
        ],
        Stroke::new(1.0, Color32::from_rgb(232, 232, 232)),
    );
    p.line_segment(
        [
            knob_rect.left_bottom() + egui::vec2(1.0, -1.0),
            knob_rect.right_bottom() + egui::vec2(-1.0, -1.0),
        ],
        Stroke::new(1.0, Color32::from_rgb(70, 70, 70)),
    );

    response
}

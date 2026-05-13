use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use egui::{self, Align2, Color32, FontId, RichText};

use crate::menu::Settings;
use crate::save::{self, GameMode, WorldDifficulty, WorldListEntry, WorldMeta, WorldType};

#[derive(Clone, Debug)]
pub enum StartMenuAction {
    None,
    LoadWorld { seed: u32, path: PathBuf },
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StartMenuPage {
    Main,
    Worlds,
    Multiplayer,
    Settings,
    CreateWorld,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CreateWorldTab {
    Game,
    World,
}

pub struct StartMenu {
    pub open: bool,
    page: StartMenuPage,
    worlds: Vec<WorldListEntry>,
    selected_world: Option<usize>,
    create_tab: CreateWorldTab,
    create_world_name: String,
    create_seed: String,
    create_game_mode: GameMode,
    create_difficulty: WorldDifficulty,
    create_allow_commands: bool,
    create_world_type: WorldType,
    create_error: Option<String>,
}

impl StartMenu {
    pub fn new(open: bool) -> Self {
        let mut s = Self {
            open,
            page: StartMenuPage::Main,
            worlds: Vec::new(),
            selected_world: None,
            create_tab: CreateWorldTab::Game,
            create_world_name: "Новый мир".to_string(),
            create_seed: String::new(),
            create_game_mode: GameMode::Survival,
            create_difficulty: WorldDifficulty::Normal,
            create_allow_commands: false,
            create_world_type: WorldType::Normal,
            create_error: None,
        };
        s.refresh_worlds();
        s
    }

    pub fn draw(&mut self, ctx: &egui::Context, settings: &mut Settings) -> StartMenuAction {
        if !self.open {
            return StartMenuAction::None;
        }

        let mut action = StartMenuAction::None;
        let screen = ctx.screen_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("start_menu_bg"),
        ));
        painter.rect_filled(screen, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 150));

        let panel = egui::vec2(760.0, 540.0);
        let panel_rect = egui::Rect::from_center_size(screen.center(), panel);

        egui::Area::new("start_menu_panel".into())
            .order(egui::Order::Foreground)
            .fixed_pos(panel_rect.min)
            .show(ctx, |ui| {
                ui.set_min_size(panel);
                draw_title(ui, panel.x);
                ui.add_space(8.0);

                match self.page {
                    StartMenuPage::Main => self.draw_main(ui, &mut action),
                    StartMenuPage::Worlds => self.draw_worlds(ui, &mut action),
                    StartMenuPage::Multiplayer => self.draw_multiplayer_stub(ui),
                    StartMenuPage::Settings => self.draw_settings(ui, settings),
                    StartMenuPage::CreateWorld => self.draw_create_world(ui, &mut action),
                }
            });

        action
    }

    fn draw_main(&mut self, ui: &mut egui::Ui, action: &mut StartMenuAction) {
        let button_w = 360.0;
        ui.vertical_centered(|ui| {
            ui.add_space(36.0);
            if ui
                .add_sized([button_w, 44.0], egui::Button::new("Одиночная игра"))
                .clicked()
            {
                self.page = StartMenuPage::Worlds;
                self.refresh_worlds();
            }
            ui.add_space(10.0);
            if ui
                .add_sized([button_w, 44.0], egui::Button::new("Сетевая игра"))
                .clicked()
            {
                self.page = StartMenuPage::Multiplayer;
            }
            ui.add_space(10.0);
            if ui
                .add_sized([button_w, 44.0], egui::Button::new("Настройки"))
                .clicked()
            {
                self.page = StartMenuPage::Settings;
            }
            ui.add_space(10.0);
            if ui
                .add_sized([button_w, 44.0], egui::Button::new("Выход"))
                .clicked()
            {
                *action = StartMenuAction::Exit;
            }
        });
    }

    fn draw_worlds(&mut self, ui: &mut egui::Ui, action: &mut StartMenuAction) {
        ui.horizontal(|ui| {
            if ui.button("Обновить список").clicked() {
                self.refresh_worlds();
            }
            if ui.button("Создать новый мир").clicked() {
                self.page = StartMenuPage::CreateWorld;
                self.create_error = None;
            }
            if ui.button("Назад").clicked() {
                self.page = StartMenuPage::Main;
            }
        });
        ui.add_space(8.0);

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_height(380.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                if self.worlds.is_empty() {
                    ui.label("Сохранений пока нет. Нажми \"Создать новый мир\".");
                    return;
                }

                for (i, world) in self.worlds.iter().enumerate() {
                    let selected = self.selected_world == Some(i);
                    let label = format!(
                        "{}  |  Seed {}  |  {}  |  {}  |  {}  |  Команды: {}",
                        world.name,
                        world.seed,
                        world.world_type.label(),
                        world.game_mode.label(),
                        world.difficulty.label(),
                        if world.allow_commands { "Да" } else { "Нет" },
                    );
                    let resp = ui.selectable_label(selected, label);
                    if resp.clicked() {
                        self.selected_world = Some(i);
                    }
                }
            });
        });

        ui.add_space(8.0);
        let can_play = self
            .selected_world
            .is_some_and(|idx| idx < self.worlds.len());
        if ui
            .add_enabled(can_play, egui::Button::new("Играть выбранный мир"))
            .clicked()
        {
            if let Some(idx) = self.selected_world {
                if let Some(world) = self.worlds.get(idx) {
                    *action = StartMenuAction::LoadWorld {
                        seed: world.seed,
                        path: world.path.clone(),
                    };
                    self.open = false;
                }
            }
        }
    }

    fn draw_multiplayer_stub(&mut self, ui: &mut egui::Ui) {
        ui.add_space(36.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("Сетевая игра")
                    .font(FontId::proportional(32.0))
                    .color(Color32::from_rgb(240, 240, 240)),
            );
            ui.add_space(14.0);
            ui.label(
                RichText::new("Пока это заглушка. Тут будет отдельное меню подключения.")
                    .size(18.0)
                    .color(Color32::from_rgb(210, 210, 210)),
            );
            ui.add_space(22.0);
            if ui.add_sized([220.0, 40.0], egui::Button::new("Назад")).clicked() {
                self.page = StartMenuPage::Main;
            }
        });
    }

    fn draw_settings(&mut self, ui: &mut egui::Ui, settings: &mut Settings) {
        ui.label(
            RichText::new("Настройки")
                .font(FontId::proportional(28.0))
                .color(Color32::from_rgb(240, 240, 240)),
        );
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            ui.label("Дальность прорисовки");
            ui.add(egui::Slider::new(&mut settings.render_dist, 2..=32));
        });
        ui.horizontal(|ui| {
            ui.label("Чувствительность мыши");
            ui.add(egui::Slider::new(&mut settings.mouse_sens, 0.02..=0.45));
        });
        ui.horizontal(|ui| {
            ui.label("FOV");
            ui.add(egui::Slider::new(&mut settings.fov, 50.0..=110.0));
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.checkbox(&mut settings.vsync, "VSync");
            ui.checkbox(&mut settings.ray_tracing, "RTX");
            ui.checkbox(&mut settings.show_fps, "Показывать FPS");
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Master Volume");
            ui.add(egui::Slider::new(&mut settings.master_volume, 0.0..=1.0));
        });
        ui.horizontal(|ui| {
            ui.label("Music");
            ui.add(egui::Slider::new(&mut settings.music_volume, 0.0..=1.0));
        });
        ui.horizontal(|ui| {
            ui.label("Ambient");
            ui.add(egui::Slider::new(&mut settings.ambient_volume, 0.0..=1.0));
        });
        ui.horizontal(|ui| {
            ui.label("Effects");
            ui.add(egui::Slider::new(&mut settings.sfx_volume, 0.0..=1.0));
        });

        ui.add_space(16.0);
        if ui.add_sized([180.0, 40.0], egui::Button::new("Назад")).clicked() {
            self.page = StartMenuPage::Main;
        }
    }

    fn draw_create_world(&mut self, ui: &mut egui::Ui, action: &mut StartMenuAction) {
        ui.horizontal(|ui| {
            let game_selected = self.create_tab == CreateWorldTab::Game;
            if ui
                .selectable_label(game_selected, "Игра")
                .on_hover_text("Имя мира, режим, сложность, команды")
                .clicked()
            {
                self.create_tab = CreateWorldTab::Game;
            }
            let world_selected = self.create_tab == CreateWorldTab::World;
            if ui
                .selectable_label(world_selected, "Мир")
                .on_hover_text("Тип мира и сид")
                .clicked()
            {
                self.create_tab = CreateWorldTab::World;
            }
        });
        ui.separator();

        match self.create_tab {
            CreateWorldTab::Game => self.draw_create_world_game_tab(ui),
            CreateWorldTab::World => self.draw_create_world_world_tab(ui),
        }

        if let Some(err) = self.create_error.as_ref() {
            ui.add_space(8.0);
            ui.colored_label(Color32::from_rgb(230, 80, 80), err);
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add_sized([220.0, 42.0], egui::Button::new("Создать мир"))
                    .clicked()
                {
                    if let Some(load) = self.try_create_world() {
                        *action = load;
                    }
                }
                if ui
                    .add_sized([220.0, 42.0], egui::Button::new("Отмена"))
                    .clicked()
                {
                    self.page = StartMenuPage::Worlds;
                }
            });
        });
    }

    fn draw_create_world_game_tab(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Имя мира");
            ui.text_edit_singleline(&mut self.create_world_name);
        });
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label("Режим игры");
            if ui.button(self.create_game_mode.label()).clicked() {
                self.create_game_mode = match self.create_game_mode {
                    GameMode::Survival => GameMode::Creative,
                    GameMode::Creative => GameMode::Hardcore,
                    GameMode::Hardcore => GameMode::Survival,
                };
            }
        });

        ui.horizontal(|ui| {
            ui.label("Сложность");
            if ui.button(self.create_difficulty.label()).clicked() {
                self.create_difficulty = match self.create_difficulty {
                    WorldDifficulty::Peaceful => WorldDifficulty::Easy,
                    WorldDifficulty::Easy => WorldDifficulty::Normal,
                    WorldDifficulty::Normal => WorldDifficulty::Hard,
                    WorldDifficulty::Hard => WorldDifficulty::Peaceful,
                };
            }
        });

        ui.horizontal(|ui| {
            ui.label("Использование команд");
            let txt = if self.create_allow_commands { "Да" } else { "Нет" };
            if ui.button(txt).clicked() {
                self.create_allow_commands = !self.create_allow_commands;
            }
        });
    }

    fn draw_create_world_world_tab(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Тип мира");
            if ui.button(self.create_world_type.label()).clicked() {
                self.create_world_type = match self.create_world_type {
                    WorldType::Normal => WorldType::Flat,
                    WorldType::Flat => WorldType::Broken,
                    WorldType::Broken => WorldType::Normal,
                };
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Сид");
            ui.text_edit_singleline(&mut self.create_seed);
        });
    }

    fn try_create_world(&mut self) -> Option<StartMenuAction> {
        self.create_error = None;
        let seed = parse_seed_field(&self.create_seed, &self.create_world_name);
        let name = if self.create_world_name.trim().is_empty() {
            format!("Мир {seed}")
        } else {
            self.create_world_name.trim().to_string()
        };

        let meta = WorldMeta {
            seed,
            name: name.clone(),
            game_mode: self.create_game_mode,
            difficulty: self.create_difficulty,
            allow_commands: self.create_allow_commands,
            world_type: self.create_world_type,
        };

        let Some(path) = save::create_world_container(seed, &name, &meta) else {
            self.create_error = Some("Не удалось создать папку мира".to_string());
            return None;
        };
        Some(StartMenuAction::LoadWorld { seed, path })
    }

    fn refresh_worlds(&mut self) {
        self.worlds = save::list_worlds();
        if self.worlds.is_empty() {
            self.selected_world = None;
        } else if let Some(idx) = self.selected_world {
            if idx >= self.worlds.len() {
                self.selected_world = Some(0);
            }
        } else {
            self.selected_world = Some(0);
        }
    }
}

fn draw_title(ui: &mut egui::Ui, width: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 82.0), egui::Sense::hover());
    let p = ui.painter();
    p.text(
        rect.center() + egui::vec2(3.0, 4.0),
        Align2::CENTER_CENTER,
        "RUSTYCRAFT",
        FontId::proportional(46.0),
        Color32::from_rgba_unmultiplied(0, 0, 0, 175),
    );
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        "RUSTYCRAFT",
        FontId::proportional(46.0),
        Color32::from_rgb(240, 240, 240),
    );
}

fn parse_seed_field(seed_input: &str, world_name: &str) -> u32 {
    let text = seed_input.trim();
    if text.is_empty() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        return fnv1a_seed(format!("{world_name}:{now}").as_bytes());
    }
    if let Ok(v) = text.parse::<u32>() {
        return if v == 0 { 1 } else { v };
    }
    let h = fnv1a_seed(text.as_bytes());
    if h == 0 { 1 } else { h }
}

fn fnv1a_seed(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

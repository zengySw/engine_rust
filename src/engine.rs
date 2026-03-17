use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use winit::{
    dpi::LogicalSize,
    event::{DeviceEvent, ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{EventLoop, ControlFlow},
    keyboard::{KeyCode, PhysicalKey},
    window::{WindowBuilder, CursorGrabMode},
};

use crate::args::Args;
use crate::camera::Camera;
use crate::inventory::{Inventory, WoodenTool};
use crate::item_registry;
use crate::menu::{EscMenu, MenuAction, Settings};
use crate::modding::{self, ApiCommand, ApiSnapshot, ModApiRuntime};
use crate::player::Player;
use crate::renderer::{
    BlockOutlineVisual, BreakOverlayVisual, DroppedBlockVisual, FirstPersonHandVisual, PlayerVisual, RainDropVisual, Renderer,
};
use crate::save;
use crate::sound::SoundSystem;
use crate::world::block::Block;
use crate::world::biome::{self, Biome};
use crate::world::chunk::{spawn_point, CHUNK_D, CHUNK_W};
use crate::world::world::World;

const DROP_PICKUP_RADIUS: f32 = 0.45;
const DROP_PICKUP_DELAY: f32 = 0.22;
const DROP_DESPAWN_SECS: f32 = 180.0;
const DROP_MAGNET_RADIUS: f32 = 1.25;
const DROP_MAGNET_MAX_SPEED: f32 = 10.0;
const DROP_MAGNET_SNAP_RADIUS: f32 = 0.65;
const DROP_MAGNET_SNAP_SPEED: f32 = 28.0;
const DEFAULT_WALK_SPEED: f32 = 7.2;
const BLOCK_BREAK_RANGE: f32 = 6.0;
const THIRD_PERSON_CAMERA_DIST: f32 = 3.2;
const WEATHER_MIN_CLEAR: f32 = 70.0;
const WEATHER_MAX_CLEAR: f32 = 150.0;
const WEATHER_MIN_RAIN: f32 = 45.0;
const WEATHER_MAX_RAIN: f32 = 95.0;
const FARMLAND_SCAN_RADIUS: i32 = 10;
const FARMLAND_SCAN_INTERVAL: f32 = 0.90;
const FARMLAND_WATER_RADIUS: i32 = 4;
const FARMLAND_WATER_VERTICAL: i32 = 1;
const RAIN_RENDER_RADIUS: f32 = 13.5;
const RAIN_RENDER_COUNT_MAX: usize = 240;
const AMBIENT_INTERVAL_MIN: f32 = 3.2;
const AMBIENT_INTERVAL_MAX: f32 = 7.4;
const BG_MUSIC_INTERVAL_MIN: f32 = 70.0;
const BG_MUSIC_INTERVAL_MAX: f32 = 140.0;
const THUNDER_INTERVAL_MIN: f32 = 9.0;
const THUNDER_INTERVAL_MAX: f32 = 19.0;
const PLAYER_MAX_HEALTH: f32 = 20.0;
const PLAYER_MAX_HUNGER: f32 = 20.0;
const PLAYER_MAX_STAMINA: f32 = 100.0;
const PLAYER_HURT_IFRAMES: f32 = 0.65;
const PLAYER_ATTACK_COOLDOWN: f32 = 0.22;
const PLAYER_SPRINT_MULT: f32 = 1.30;
const STAMINA_SPRINT_DRAIN: f32 = 26.0;
const STAMINA_RECOVERY: f32 = 22.0;
const HUNGER_IDLE_DRAIN: f32 = 0.002;
const HUNGER_MOVE_DRAIN: f32 = 0.010;
const HUNGER_SPRINT_DRAIN: f32 = 0.070;
const HUNGER_REGEN_THRESHOLD: f32 = 12.0;
const HUNGER_REGEN_INTERVAL: f32 = 2.3;
const HUNGER_REGEN_COST: f32 = 1.0;
const STARVATION_INTERVAL: f32 = 3.0;
const FALL_DAMAGE_FREE: f32 = 3.0;
const FALL_DAMAGE_PER_BLOCK: f32 = 1.0;
const MOB_MAX_COUNT: usize = 10;
const MOB_SPAWN_RANGE_MIN: f32 = 10.0;
const MOB_SPAWN_RANGE_MAX: f32 = 21.0;
const MOB_DESPAWN_RANGE: f32 = 52.0;
const MOB_CHASE_RANGE: f32 = 18.0;
const MOB_ATTACK_RANGE: f32 = 1.25;
const MOB_ATTACK_DAMAGE: f32 = 2.0;
const MOB_HIT_DAMAGE_HAND: f32 = 5.0;
const MOB_HIT_DAMAGE_WOOD_SWORD: f32 = 8.0;
const MOB_HIT_DAMAGE_STONE_SWORD: f32 = 10.0;
const MOB_HIT_DAMAGE_IRON_SWORD: f32 = 12.0;
const MOB_ATTACK_COOLDOWN: f32 = 1.15;
const MOB_SPAWN_INTERVAL_MIN: f32 = 4.0;
const MOB_SPAWN_INTERVAL_MAX: f32 = 8.2;
const CAVE_MOOD_TRIGGER: f32 = 100.0;
const CAVE_MOOD_INCREASE_MIN: f32 = 5.0;
const CAVE_MOOD_INCREASE_MAX: f32 = 24.0;
const CAVE_MOOD_DECAY: f32 = 16.0;
const CAVE_MOOD_COOLDOWN_MIN: f32 = 8.0;
const CAVE_MOOD_COOLDOWN_MAX: f32 = 18.0;
const CAVE_MOOD_SURFACE_HOLD_SECS: f32 = 5.0;
const VISIBILITY_CULL_INTERVAL_SECS: f32 = 1.0 / 75.0;
const RTX_UPDATE_INTERVAL_SECS: f32 = 1.0 / 30.0;
const MAX_FPS_NO_VSYNC: f32 = 240.0;
struct DroppedItem {
    block: Block,
    count: u16,
    pos: glam::Vec3,
    vel: glam::Vec3,
    age: f32,
    pickup_delay: f32,
}

struct BreakingState {
    block: (i32, i32, i32),
    face: (i32, i32, i32),
    elapsed: f32,
    required: f32,
}

struct MobEntity {
    pos: glam::Vec3,
    vel: glam::Vec3,
    hp: f32,
    attack_cooldown: f32,
    wander_phase: f32,
    age: f32,
}

#[derive(Clone)]
struct HeartHudTextures {
    full: egui::TextureHandle,
    half: egui::TextureHandle,
}

pub struct Engine {
    args:     Args,
    world_seed: u32,
    renderer: Renderer,
    camera:   Camera,
    world:    World,
    keys:     HashSet<KeyCode>,
    focused:  bool,
    menu:     EscMenu,
    inventory: Inventory,
    day_time: f32,
    player: Player,
    third_person: bool,
    equipped_tool: Option<WoodenTool>,
    wood_tool_durability: [u16; 15],
    rain_active: bool,
    rain_strength: f32,
    surface_wetness: f32,
    weather_timer: f32,
    weather_rng: u32,
    rain_anim_time: f32,
    ambient_timer: f32,
    bg_music_timer: f32,
    thunder_timer: f32,
    farmland_scan_timer: f32,
    walk_speed: f32,
    footstep_timer: f32,
    hand_phase_rad: f32,
    hand_swing: f32,
    hand_action_phase_rad: f32,
    hand_action_strength: f32,
    jump_was_down: bool,
    debug_overlay: bool,
    mod_api_runtime: ModApiRuntime,
    applied_render_dist: i32,
    applied_vsync: bool,
    applied_ray_tracing: bool,
    applied_lighting: [f32; 4],
    left_mouse_down: bool,
    left_mouse_was_down: bool,
    place_block_requested: bool,
    breaking: Option<BreakingState>,
    dropped_items: Vec<DroppedItem>,
    mobs: Vec<MobEntity>,
    mob_spawn_timer: f32,
    player_health: f32,
    player_hunger: f32,
    player_stamina: f32,
    player_hurt_timer: f32,
    player_attack_cooldown: f32,
    health_regen_timer: f32,
    starvation_timer: f32,
    fall_peak_y: f32,
    sprinting: bool,
    cave_mood_percent: f32,
    cave_mood_cooldown: f32,
    cave_mood_surface_hold: f32,
    sound_system: Option<SoundSystem>,
    heart_hud_textures: Option<HeartHudTextures>,
    heart_hud_load_failed: bool,
    tool_hud_textures: HashMap<WoodenTool, egui::TextureHandle>,
    tool_hud_missing: HashSet<WoodenTool>,
    inventory_save_timer: f32,
    visibility_cull_timer: f32,
    rt_update_timer: f32,
    last_saved_settings: Settings,
}

impl Engine {
    pub async fn new(args: Args) -> (Self, EventLoop<()>) {
        let world_seed: u32 = args
            .world_file
            .as_ref()
            .map(|p| save::resolve_world_seed_from_path(p))
            .unwrap_or(42);
        if let Some(path) = args.world_file.as_ref() {
            save::set_world_file_override(world_seed, path.clone());
            log::info!(
                "Opening world file: {:?} (seed {})",
                path,
                world_seed
            );
        }
        let (mod_api, mod_api_runtime) = modding::create_api(world_seed);
        modding::install_global_api(mod_api);

        let event_loop = EventLoop::new().expect("event loop");
        event_loop.set_control_flow(ControlFlow::Poll);

        let window = WindowBuilder::new()
            .with_title("MyEngine")
            .with_inner_size(LogicalSize::new(1280u32, 720u32))
            .with_resizable(true)
            .build(&event_loop)
            .expect("window");

        let mut effective_args = args.clone();
        let loaded_settings = save::load_settings();
        if let Some(saved) = loaded_settings {
            effective_args.render_dist = saved.render_dist.clamp(2, 32);
            effective_args.vsync = saved.vsync;
        }

        let renderer = Renderer::new(window, &effective_args).await;
        let (spawn_x, spawn_y, spawn_z) = spawn_point(world_seed);
        let player   = Player::new(glam::Vec3::new(spawn_x, spawn_y, spawn_z));
        let mut camera = Camera::new(player.eye_pos());
        let world    = World::new(world_seed, &effective_args);

        let default_settings = Settings {
            render_dist: effective_args.render_dist,
            mouse_sens:  camera.sensitivity,
            fov:         camera.fov,
            vsync:       effective_args.vsync,
            ray_tracing: false,
            show_fps:    true,
            master_volume: 1.0,
            music_volume: 0.70,
            ambient_volume: 0.90,
            sfx_volume: 1.0,
            ambient_boost: 1.06,
            sun_softness: 0.26,
            fog_density: 0.90,
            exposure: 1.03,
        };
        let settings = loaded_settings.unwrap_or(default_settings);
        camera.sensitivity = settings.mouse_sens;
        camera.fov = settings.fov.clamp(50.0, 110.0);
        let menu = EscMenu::new(settings);
        let inventory = Inventory::new();

        let mut engine = Self {
            args: effective_args,
            world_seed,
            renderer,
            camera,
            world,
            keys: HashSet::new(),
            focused: false,
            menu,
            inventory,
            day_time: 0.25,
            player,
            third_person: false,
            equipped_tool: None,
            wood_tool_durability: [0; 15],
            rain_active: false,
            rain_strength: 0.0,
            surface_wetness: 0.0,
            weather_timer: 38.0,
            weather_rng: world_seed ^ 0xA53C_9E17,
            rain_anim_time: 0.0,
            ambient_timer: 0.8,
            bg_music_timer: 15.0,
            thunder_timer: 11.0,
            farmland_scan_timer: 0.0,
            walk_speed: DEFAULT_WALK_SPEED,
            footstep_timer: 0.0,
            hand_phase_rad: 0.0,
            hand_swing: 0.0,
            hand_action_phase_rad: 0.0,
            hand_action_strength: 0.0,
            jump_was_down: false,
            debug_overlay: false,
            mod_api_runtime,
            applied_render_dist: settings.render_dist,
            applied_vsync: settings.vsync,
            applied_ray_tracing: false,
            // Force first runtime apply to push lighting into renderer immediately on startup.
            applied_lighting: [f32::INFINITY; 4],
            left_mouse_down: false,
            left_mouse_was_down: false,
            place_block_requested: false,
            breaking: None,
            dropped_items: Vec::new(),
            mobs: Vec::new(),
            mob_spawn_timer: 3.5,
            player_health: PLAYER_MAX_HEALTH,
            player_hunger: PLAYER_MAX_HUNGER,
            player_stamina: PLAYER_MAX_STAMINA,
            player_hurt_timer: 0.0,
            player_attack_cooldown: 0.0,
            health_regen_timer: 0.0,
            starvation_timer: 0.0,
            fall_peak_y: spawn_y,
            sprinting: false,
            cave_mood_percent: 0.0,
            cave_mood_cooldown: 0.0,
            cave_mood_surface_hold: 0.0,
            sound_system: SoundSystem::new(),
            heart_hud_textures: None,
            heart_hud_load_failed: false,
            tool_hud_textures: HashMap::new(),
            tool_hud_missing: HashSet::new(),
            inventory_save_timer: 0.25,
            visibility_cull_timer: 0.0,
            rt_update_timer: 0.0,
            last_saved_settings: settings,
        };

        // Apply saved RTX state once renderer and menu are initialized.
        let desired_rt = engine.menu.settings.ray_tracing;
        engine.renderer.set_ray_tracing_enabled(desired_rt);
        engine.menu.settings.ray_tracing = engine.renderer.ray_tracing_enabled();
        engine.applied_ray_tracing = engine.menu.settings.ray_tracing;
        engine.load_player_state();
        engine.load_inventory_state();
        // Apply all loaded settings right now (before first rendered frame).
        engine.apply_runtime_graphics_settings();

        engine.update_api_snapshot();
        (engine, event_loop)
    }

    pub fn run(mut self, event_loop: EventLoop<()>) {
        let mut last_time   = Instant::now();
        let mut frame_count = 0u64;
        let mut fps_timer   = Instant::now();
        let mut fps_display = 0.0f32;

        event_loop.run(move |event, elwt| {
            match &event {
                Event::WindowEvent { event: we, .. } => {
                    // Сначала отдаём событие egui
                    let window = self.renderer.window_arc();
                    let resp = self.renderer.egui.handle_event(&window, we);

                    if let WindowEvent::KeyboardInput { event: ke, .. } = we {
                        if let PhysicalKey::Code(KeyCode::F3) = ke.physical_key {
                            if ke.state == ElementState::Pressed && !ke.repeat {
                                self.debug_overlay = !self.debug_overlay;
                            }
                        }
                        if let PhysicalKey::Code(KeyCode::F5) = ke.physical_key {
                            if ke.state == ElementState::Pressed && !ke.repeat {
                                self.third_person = !self.third_person;
                                self.update_camera_transform();
                            }
                        }
                        if let PhysicalKey::Code(KeyCode::F6) = ke.physical_key {
                            if ke.state == ElementState::Pressed && !ke.repeat {
                                let next = !self.renderer.ray_tracing_enabled();
                                self.renderer.set_ray_tracing_enabled(next);
                                self.menu.settings.ray_tracing = self.renderer.ray_tracing_enabled();
                                log::info!(
                                    "Ray tracing: {}",
                                    if self.renderer.ray_tracing_enabled() { "ON" } else { "OFF" }
                                );
                            }
                        }
                    }

                    // Если egui не потребил — обрабатываем сами
                    if !resp.consumed {
                        match we {
                            WindowEvent::CloseRequested => {
                                self.save_player_state();
                                self.save_inventory_state();
                                self.world.save_all();
                                save::save_settings(&self.menu.settings);
                                elwt.exit();
                            }

                            WindowEvent::Focused(f) => {
                                self.focused = *f;
                                if !*f {
                                    self.left_mouse_down = false;
                                    self.left_mouse_was_down = false;
                                    self.breaking = None;
                                }
                                if *f && !self.menu.open && !self.inventory.open { self.grab_cursor(true); }
                            }

                            WindowEvent::KeyboardInput { event: ke, .. } => {
                                if let PhysicalKey::Code(key) = ke.physical_key {
                                    match ke.state {
                                        ElementState::Pressed  => { self.keys.insert(key); }
                                        ElementState::Released => { self.keys.remove(&key); }
                                    }

                                    if ke.state == ElementState::Pressed && !ke.repeat {
                                        if let Some(slot) = digit_key_to_hotbar_slot(key) {
                                            self.inventory.select_hotbar_slot(slot);
                                        }
                                        if key == KeyCode::KeyQ
                                            && !self.menu.open
                                            && !self.inventory.open
                                        {
                                            self.cycle_wooden_tool();
                                        }
                                        if key == KeyCode::KeyC
                                            && self.inventory.open
                                            && !self.menu.open
                                        {
                                            if let Some(name) = self.try_quick_craft() {
                                                log::info!("Crafted: {name}");
                                                if let Some(sound) = self.sound_system.as_mut() {
                                                    sound.play_ambient_family(&["stone", "wood", "gravel"], 0.24);
                                                }
                                            }
                                        }
                                    }

                                    if key == KeyCode::Escape
                                        && ke.state == ElementState::Pressed
                                    {
                                        if self.inventory.open {
                                            self.inventory.close();
                                            self.left_mouse_down = false;
                                            self.left_mouse_was_down = false;
                                            self.breaking = None;
                                            self.grab_cursor(true);
                                        } else {
                                            self.menu.toggle();
                                            if self.menu.open {
                                                self.inventory.close();
                                                self.left_mouse_down = false;
                                                self.left_mouse_was_down = false;
                                                self.breaking = None;
                                                self.grab_cursor(false);
                                            } else {
                                                self.grab_cursor(true);
                                            }
                                        }
                                    }

                                    if key == KeyCode::KeyE
                                        && ke.state == ElementState::Pressed
                                        && !self.menu.open
                                    {
                                        self.inventory.toggle();
                                        if self.inventory.open {
                                            self.left_mouse_down = false;
                                            self.left_mouse_was_down = false;
                                            self.breaking = None;
                                            self.grab_cursor(false);
                                        } else {
                                            self.grab_cursor(true);
                                        }
                                    }
                                }
                            }

                            WindowEvent::MouseInput { state, button, .. } => {
                                if *button == MouseButton::Left {
                                    if !self.menu.open && !self.inventory.open {
                                        self.left_mouse_down = *state == ElementState::Pressed;
                                        if *state == ElementState::Released {
                                            self.breaking = None;
                                        }
                                    } else {
                                        self.left_mouse_down = false;
                                        self.left_mouse_was_down = false;
                                        self.breaking = None;
                                    }
                                }

                                if *state == ElementState::Pressed
                                    && *button == MouseButton::Right
                                    && !self.menu.open
                                    && !self.inventory.open
                                {
                                    self.place_block_requested = true;
                                }
                            }

                            WindowEvent::MouseWheel { delta, .. } => {
                                if !self.menu.open {
                                    let amount = match delta {
                                        MouseScrollDelta::LineDelta(_, y) => *y,
                                        MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 35.0,
                                    };
                                    if amount.abs() > 0.01 {
                                        let step = if amount > 0.0 { -1 } else { 1 };
                                        self.inventory.cycle_hotbar(step);
                                    }
                                }
                            }

                            WindowEvent::Resized(s) => {
                                self.renderer.resize(s.width, s.height);
                            }
                            _ => {}
                        }
                    }
                }

                Event::DeviceEvent {
                    event: DeviceEvent::MouseMotion { delta: (dx, dy) }, ..
                } => {
                    if self.focused && !self.menu.open && !self.inventory.open {
                        self.camera.rotate(*dx as f32, *dy as f32);
                    }
                }

                Event::AboutToWait => {
                    if !self.menu.settings.vsync {
                        let min_frame = 1.0 / MAX_FPS_NO_VSYNC;
                        let elapsed = Instant::now().duration_since(last_time).as_secs_f32();
                        if elapsed < min_frame {
                            std::thread::sleep(Duration::from_secs_f32(min_frame - elapsed));
                        }
                    }
                    let now = Instant::now();
                    let dt  = now.duration_since(last_time).as_secs_f32().min(0.05);
                    last_time = now;

                    // day/night cycle (0..1)
                    let day_len = 300.0;
                    self.day_time += dt / day_len;
                    if self.day_time >= 1.0 { self.day_time -= 1.0; }
                    self.update_weather(dt);
                    self.process_api_commands();

                    frame_count += 1;
                    if fps_timer.elapsed().as_secs_f32() >= 1.0 {
                        fps_display = frame_count as f32 / fps_timer.elapsed().as_secs_f32();
                        if self.menu.settings.show_fps {
                            self.renderer.window().set_title(&format!(
                                "MyEngine  |  {:.0} fps  |  {:.2} ms  |  xyz: {:.0} {:.0} {:.0}  |  chunks: {}",
                                fps_display, dt * 1000.0,
                                self.camera.pos.x, self.camera.pos.y, self.camera.pos.z,
                                self.world.chunk_count(),
                            ));
                        }
                        fps_timer   = Instant::now();
                        frame_count = 0;
                    }

                    // Игровой update только когда меню закрыто
                    if !self.menu.open {
                        self.inventory.tick_furnace(dt);
                    }
                    if !self.menu.open && !self.inventory.open {
                        self.update(dt);
                    }

                    // Применяем настройки из меню к камере
                    self.camera.sensitivity = self.menu.settings.mouse_sens;
                    self.apply_runtime_graphics_settings();
                    self.save_settings_if_changed();
                    self.apply_crafted_tools_from_inventory();

                    self.visibility_cull_timer -= dt;
                    if self.visibility_cull_timer <= 0.0 {
                        self.renderer.update_visibility(&self.camera);
                        self.visibility_cull_timer = VISIBILITY_CULL_INTERVAL_SECS;
                    }
                    self.renderer.update_camera(&self.camera, self.day_time);
                    if self.renderer.ray_tracing_enabled() {
                        self.rt_update_timer -= dt;
                        if self.rt_update_timer <= 0.0 {
                            self.renderer.update_ray_tracing(
                                &self.camera,
                                &self.world,
                                self.day_time,
                                self.rain_strength,
                                self.rain_anim_time,
                                self.surface_wetness,
                            );
                            self.rt_update_timer = RTX_UPDATE_INTERVAL_SECS;
                        }
                    } else {
                        self.rt_update_timer = 0.0;
                    }
                    let drop_visuals = self.collect_drop_visuals();
                    self.renderer.update_dropped_blocks(&drop_visuals);
                    let rain_visuals = self.collect_rain_visuals();
                    self.renderer.update_rain_drops(&rain_visuals);
                    let break_overlay = self.collect_break_overlay_visual();
                    self.renderer.update_break_overlay(break_overlay);
                    let block_outline = if !self.menu.open && !self.inventory.open {
                        self.collect_block_outline_visual()
                    } else {
                        None
                    };
                    self.renderer.update_block_outline(block_outline);
                    let hand_visual = if !self.third_person && !self.menu.open && !self.inventory.open {
                        Some(self.collect_hand_visual())
                    } else {
                        None
                    };
                    self.renderer.update_first_person_hand(&self.camera, hand_visual);
                    let player_visual = if self.third_person {
                        Some(self.collect_player_visual())
                    } else {
                        None
                    };
                    self.renderer.update_player_visual(player_visual);

                    let show_debug = self.debug_overlay;
                    let debug_data = if show_debug {
                        Some(self.collect_debug_overlay(fps_display, dt))
                    } else {
                        None
                    };
                    let heart_hud = self.ensure_heart_hud_textures();
                    let equipped_tool = self.equipped_tool;
                    let equipped_tool_hud = equipped_tool.and_then(|tool| self.ensure_tool_hud_texture(tool));
                    let equipped_tool_durability = equipped_tool
                        .map(|tool| self.tool_durability(tool))
                        .unwrap_or(0);
                    let equipped_tool_max = equipped_tool
                        .map(|tool| item_registry::tool_max_durability(tool))
                        .unwrap_or(1);
                    let show_fps = self.menu.settings.show_fps;
                    let target_hud = if !self.menu.open && !self.inventory.open {
                        self.collect_target_hud_info()
                    } else {
                        None
                    };
                    self.inventory
                        .set_tool_durability_values(self.wood_tool_durability);
                    let menu = &mut self.menu;
                    let inventory = &mut self.inventory;
                    let fps = fps_display;
                    let player_health = self.player_health;
                    let player_hunger = self.player_hunger;
                    let player_stamina = self.player_stamina;
                    let sprinting = self.sprinting;
                    let cave_mood = self.cave_mood_percent;
                    let mut menu_action = MenuAction::None;

                    match self.renderer.render(&self.camera, |ctx| {
                        // FPS оверлей (когда меню закрыто)
                        if show_debug && !menu.open && !inventory.open {
                            if let Some(data) = debug_data.as_ref() {
                                draw_f3_overlay(ctx, data);
                            }
                        } else if show_fps && !menu.open {
                            egui::Area::new("fps".into())
                                .fixed_pos(egui::pos2(10.0, 10.0))
                                .show(ctx, |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{:.0} FPS", fps))
                                            .size(14.0)
                                            .color(egui::Color32::WHITE)
                                            .background_color(egui::Color32::from_black_alpha(120)),
                                    );
                                });
                        }

                        if !menu.open {
                            if inventory.open {
                                inventory.draw(ctx);
                            } else {
                                inventory.draw_hotbar(ctx);
                                if let Some(textures) = heart_hud.as_ref() {
                                    draw_health_hud(ctx, textures, player_health);
                                }
                                draw_survival_hud(ctx, player_hunger, player_stamina, sprinting);
                                if let Some(tool_tex) = equipped_tool_hud.as_ref() {
                                    draw_equipped_tool_hud(
                                        ctx,
                                        tool_tex,
                                        equipped_tool_durability,
                                        equipped_tool_max,
                                    );
                                }
                                draw_cave_mood_hud(ctx, cave_mood);
                                if let Some(target) = target_hud.as_ref() {
                                    draw_target_hud(ctx, target, heart_hud.as_ref());
                                }
                            }
                        }

                        // Меню паузы
                        menu_action = menu.draw(ctx);
                    }) {
                        Ok(_) => {}
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated)
                            => self.renderer.reconfigure(),
                        Err(e) => log::error!("{e}"),
                    }

                    match menu_action {
                        MenuAction::Resume => {
                            self.menu.open = false;
                        }
                        MenuAction::RegenerateWorld => {
                            self.regenerate_world(None);
                        }
                        MenuAction::Exit => {
                            self.save_player_state();
                            self.save_inventory_state();
                            self.world.save_all();
                            save::save_settings(&self.menu.settings);
                            std::process::exit(0);
                        }
                        MenuAction::None => {}
                    }

                    // Применяем Resume после рендера (чтобы не было borrow conflict)
                    if !self.menu.open && !self.focused {
                        self.grab_cursor(true);
                        self.focused = true;
                    }
                    self.update_api_snapshot();
                }

                _ => {}
            }
        }).expect("event loop error");
    }

    fn update(&mut self, dt: f32) {
        self.world.update(self.player.pos.x, self.player.pos.z);
        let prev_feet = self.player.pos;
        let was_on_ground = self.player.on_ground;

        let mut input = glam::Vec2::ZERO;
        if self.keys.contains(&KeyCode::KeyD) { input.x += 1.0; }
        if self.keys.contains(&KeyCode::KeyA) { input.x -= 1.0; }
        if self.keys.contains(&KeyCode::KeyW) { input.y += 1.0; }
        if self.keys.contains(&KeyCode::KeyS) { input.y -= 1.0; }
        let jump_down = self.keys.contains(&KeyCode::Space);
        let jump_pressed = jump_down && !self.jump_was_down;
        self.jump_was_down = jump_down;

        let mut look = glam::Vec2::ZERO;
        if self.keys.contains(&KeyCode::ArrowLeft) { look.x -= 1.0; }
        if self.keys.contains(&KeyCode::ArrowRight) { look.x += 1.0; }
        if self.keys.contains(&KeyCode::ArrowUp) { look.y += 1.0; }
        if self.keys.contains(&KeyCode::ArrowDown) { look.y -= 1.0; }
        if look.length_squared() > 0.0 {
            let look_speed_deg = 140.0;
            self.camera.yaw += look.x * look_speed_deg * dt;
            self.camera.pitch = (self.camera.pitch + look.y * look_speed_deg * dt).clamp(-89.0, 89.0);
        }

        let sprint_key = self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
        let has_move_input = input.length_squared() > 0.0001;
        let wants_sprint = sprint_key && has_move_input && input.y > 0.05;
        let can_sprint = wants_sprint && self.player_hunger > 0.5 && self.player_stamina > 1.0;
        let current_walk_speed = if can_sprint {
            self.walk_speed * PLAYER_SPRINT_MULT
        } else {
            self.walk_speed
        };

        self.player.simulate(
            dt,
            input,
            jump_pressed,
            self.camera.yaw,
            current_walk_speed,
            &self.world,
        );
        let moved = glam::Vec2::new(self.player.pos.x - prev_feet.x, self.player.pos.z - prev_feet.z);
        let horizontal_speed = if dt > f32::EPSILON {
            moved.length() / dt
        } else {
            0.0
        };
        let moving = horizontal_speed > 0.12;
        self.sprinting = can_sprint && moving && self.player.on_ground;
        self.update_survival_stats(dt, moving);
        self.update_fall_damage(was_on_ground);
        self.update_camera_transform();
        self.sync_equipped_tool_from_hotbar();
        self.update_hand_animation(dt, prev_feet);
        self.update_footstep_audio(dt, prev_feet);
        self.player_hurt_timer = (self.player_hurt_timer - dt).max(0.0);
        self.player_attack_cooldown = (self.player_attack_cooldown - dt).max(0.0);
        self.update_mobs(dt);
        self.handle_block_interaction(dt);
        self.update_dropped_items(dt);
        self.update_cave_mood(dt);
        self.update_ambient_audio(dt);

        if !self.world.removed.is_empty() {
            self.renderer.remove_chunks(&self.world.removed);
            self.world.removed.clear();
        }

        let mesh_budget = 6usize + (self.world.ready_meshes.len() / 18).min(20);
        let batch = self.world.drain_ready_meshes(mesh_budget);
        if !batch.is_empty() {
            self.renderer.update_chunks(batch);
        }
        self.world.save_if_dirty();
        self.inventory_save_timer -= dt;
        if self.inventory_save_timer <= 0.0 {
            self.save_player_state();
            self.save_inventory_state();
            self.inventory_save_timer = 1.0;
        }

    }

    fn update_camera_transform(&mut self) {
        let eye = self.player.eye_pos();
        if !self.third_person {
            self.camera.pos = eye;
            return;
        }

        let forward = self.camera.forward();
        let mut dist = 0.0f32;
        let mut max_dist = THIRD_PERSON_CAMERA_DIST;
        let step = 0.10f32;

        while dist < THIRD_PERSON_CAMERA_DIST {
            let next = (dist + step).min(THIRD_PERSON_CAMERA_DIST);
            let probe = eye - forward * next + glam::Vec3::new(0.0, 0.22, 0.0);
            let hit = self.world.is_solid_at_world(
                probe.x.floor() as i32,
                probe.y.floor() as i32,
                probe.z.floor() as i32,
            );
            if hit {
                max_dist = dist;
                break;
            }
            dist = next;
        }

        let applied_dist = max_dist.clamp(0.30, THIRD_PERSON_CAMERA_DIST);
        self.camera.pos = eye - forward * applied_dist + glam::Vec3::new(0.0, 0.24, 0.0);
    }

    fn regenerate_world(&mut self, seed_override: Option<u32>) {
        self.save_player_state();
        self.save_inventory_state();
        self.world.save_all();
        save::clear_world_file_override(self.world_seed);
        self.world_seed = seed_override.unwrap_or_else(|| next_seed(self.world_seed));
        log::info!("Regenerating world with seed {}", self.world_seed);

        self.renderer.clear_chunks();
        self.world = World::new(self.world_seed, &self.args);

        let (spawn_x, spawn_y, spawn_z) = spawn_point(self.world_seed);
        self.player = Player::new(glam::Vec3::new(spawn_x, spawn_y, spawn_z));
        self.update_camera_transform();
        self.day_time = 0.25;
        self.jump_was_down = false;
        self.equipped_tool = None;
        self.wood_tool_durability = [0; 15];
        self.footstep_timer = 0.0;
        self.hand_phase_rad = 0.0;
        self.hand_swing = 0.0;
        self.hand_action_phase_rad = 0.0;
        self.hand_action_strength = 0.0;
        self.rain_active = false;
        self.rain_strength = 0.0;
        self.surface_wetness = 0.0;
        self.weather_timer = 38.0;
        self.weather_rng = self.world_seed ^ 0xA53C_9E17;
        self.rain_anim_time = 0.0;
        self.ambient_timer = 0.8;
        self.bg_music_timer = 15.0;
        self.thunder_timer = 11.0;
        self.farmland_scan_timer = 0.0;
        self.left_mouse_down = false;
        self.left_mouse_was_down = false;
        self.breaking = None;
        self.keys.clear();
        self.debug_overlay = false;
        self.dropped_items.clear();
        self.mobs.clear();
        self.mob_spawn_timer = 3.5;
        self.player_health = PLAYER_MAX_HEALTH;
        self.player_hunger = PLAYER_MAX_HUNGER;
        self.player_stamina = PLAYER_MAX_STAMINA;
        self.player_hurt_timer = 0.0;
        self.player_attack_cooldown = 0.0;
        self.health_regen_timer = 0.0;
        self.starvation_timer = 0.0;
        self.fall_peak_y = self.player.pos.y;
        self.sprinting = false;
        self.cave_mood_percent = 0.0;
        self.cave_mood_cooldown = 0.0;
        self.cave_mood_surface_hold = 0.0;
        self.inventory = Inventory::new();
        self.inventory_save_timer = 0.25;
        self.visibility_cull_timer = 0.0;
        self.rt_update_timer = 0.0;
        self.load_player_state();
        self.load_inventory_state();

        self.menu.open = false;
        self.inventory.close();
        self.grab_cursor(true);
    }

    fn apply_runtime_graphics_settings(&mut self) {
        self.camera.fov = self.menu.settings.fov.clamp(50.0, 110.0);

        let rd = self.menu.settings.render_dist.clamp(2, 32);
        if rd != self.applied_render_dist {
            self.world.set_render_distance(rd);
            self.applied_render_dist = rd;
            self.args.render_dist = rd;
        }

        let vsync = self.menu.settings.vsync;
        if vsync != self.applied_vsync {
            self.renderer.set_vsync_enabled(vsync);
            self.applied_vsync = vsync;
            self.args.vsync = vsync;
        }

        let rt = self.menu.settings.ray_tracing;
        if rt != self.applied_ray_tracing {
            self.renderer.set_ray_tracing_enabled(rt);
            self.applied_ray_tracing = self.renderer.ray_tracing_enabled();
            self.menu.settings.ray_tracing = self.applied_ray_tracing;
            self.rt_update_timer = 0.0;
        }

        let lighting = [
            self.menu.settings.ambient_boost,
            self.menu.settings.sun_softness,
            self.menu.settings.fog_density,
            self.menu.settings.exposure,
        ];
        let changed = (lighting[0] - self.applied_lighting[0]).abs() > 0.0001
            || (lighting[1] - self.applied_lighting[1]).abs() > 0.0001
            || (lighting[2] - self.applied_lighting[2]).abs() > 0.0001
            || (lighting[3] - self.applied_lighting[3]).abs() > 0.0001;
        if changed {
            self.renderer.set_lighting_params(
                lighting[0],
                lighting[1],
                lighting[2],
                lighting[3],
            );
            self.applied_lighting = lighting;
        }

        if let Some(sound) = self.sound_system.as_mut() {
            sound.set_mix(
                self.menu.settings.master_volume,
                self.menu.settings.music_volume,
                self.menu.settings.ambient_volume,
                self.menu.settings.sfx_volume,
            );
        }
    }

    fn save_settings_if_changed(&mut self) {
        if !settings_changed(&self.last_saved_settings, &self.menu.settings) {
            return;
        }
        save::save_settings(&self.menu.settings);
        self.last_saved_settings = self.menu.settings;
    }

    fn save_player_state(&self) {
        let state = save::PlayerState {
            pos: [self.player.pos.x, self.player.pos.y, self.player.pos.z],
            yaw: self.camera.yaw,
            pitch: self.camera.pitch,
            day_time: self.day_time,
            health: self.player_health,
            hunger: self.player_hunger,
            stamina: self.player_stamina,
            rain_active: self.rain_active,
            rain_strength: self.rain_strength,
            surface_wetness: self.surface_wetness,
        };
        save::save_player_state(self.world_seed, &state);
    }

    fn load_player_state(&mut self) {
        let Some(state) = save::load_player_state(self.world_seed) else {
            return;
        };

        if state.pos.iter().all(|v| v.is_finite()) {
            self.player
                .teleport(glam::Vec3::new(state.pos[0], state.pos[1], state.pos[2]));
        }
        if state.yaw.is_finite() {
            self.camera.yaw = state.yaw;
        }
        if state.pitch.is_finite() {
            self.camera.pitch = state.pitch.clamp(-89.0, 89.0);
        }
        if state.day_time.is_finite() {
            self.day_time = state.day_time.rem_euclid(1.0);
        }
        if state.health.is_finite() {
            self.player_health = state.health.clamp(0.0, PLAYER_MAX_HEALTH);
        }
        if state.hunger.is_finite() {
            self.player_hunger = state.hunger.clamp(0.0, PLAYER_MAX_HUNGER);
        }
        if state.stamina.is_finite() {
            self.player_stamina = state.stamina.clamp(0.0, PLAYER_MAX_STAMINA);
        }
        if state.rain_strength.is_finite() {
            self.rain_strength = state.rain_strength.clamp(0.0, 1.0);
        }
        if state.surface_wetness.is_finite() {
            self.surface_wetness = state.surface_wetness.clamp(0.0, 1.0);
        }
        self.rain_active = state.rain_active || self.rain_strength > 0.01;
        self.fall_peak_y = self.player.pos.y;
        self.player_hurt_timer = 0.0;
        self.health_regen_timer = 0.0;
        self.update_camera_transform();
    }

    fn save_inventory_state(&self) {
        save::save_inventory_state(self.world_seed, &self.inventory, self.wood_tool_durability);
    }

    fn load_inventory_state(&mut self) {
        let Some(state) = save::load_inventory_state(self.world_seed) else {
            return;
        };
        self.inventory.hotbar = state.hotbar;
        self.inventory.grid = state.grid;
        self.inventory.select_hotbar_slot(state.selected);
        self.wood_tool_durability = state.tool_durability;
        self.repair_missing_tool_durability();
        self.sync_equipped_tool_from_hotbar();
    }

    fn repair_missing_tool_durability(&mut self) {
        let all_tools = [
            WoodenTool::Pickaxe,
            WoodenTool::Axe,
            WoodenTool::Shovel,
            WoodenTool::Hoe,
            WoodenTool::Sword,
            WoodenTool::StonePickaxe,
            WoodenTool::StoneAxe,
            WoodenTool::StoneShovel,
            WoodenTool::StoneHoe,
            WoodenTool::StoneSword,
            WoodenTool::IronPickaxe,
            WoodenTool::IronAxe,
            WoodenTool::IronShovel,
            WoodenTool::IronHoe,
            WoodenTool::IronSword,
        ];
        for tool in all_tools {
            let idx = tool.idx();
            if self.wood_tool_durability[idx] == 0 && self.inventory.has_tool(tool) {
                self.wood_tool_durability[idx] = item_registry::tool_max_durability(tool);
            }
        }
    }

    fn apply_crafted_tools_from_inventory(&mut self) {
        while let Some(tool) = self.inventory.take_crafted_tool() {
            self.wood_tool_durability[tool.idx()] = item_registry::tool_max_durability(tool);
            if self.equipped_tool.is_none() {
                self.equipped_tool = Some(tool);
            }
            let tool_id = item_registry::tool_item_id(tool);
            let tool_key = item_registry::item_key_from_id(tool_id).unwrap_or("unknown");
            log::info!(
                "Crafted: {} [id={} key={}]",
                tool.display_name(),
                tool_id,
                tool_key
            );
        }
    }

    fn try_quick_craft(&mut self) -> Option<String> {
        let has_workbench = self.inventory.count_block(Block::Workbench) > 0;
        if !has_workbench {
            // First craft the workbench from wood.
            if self.inventory.count_block(Block::Wood) >= 4
                && self.inventory.can_add_block_exact(Block::Workbench, 1)
                && self.inventory.consume_blocks(Block::Wood, 4)
            {
                let _ = self.inventory.add_block(Block::Workbench, 1);
                return Some("Workbench".to_string());
            }
            // Convert one log into wooden planks when needed.
            if self.inventory.count_block(Block::Log) >= 1
                && self.inventory.can_add_block_exact(Block::Wood, 4)
                && self.inventory.consume_blocks(Block::Log, 1)
            {
                let _ = self.inventory.add_block(Block::Wood, 4);
                return Some("Wood x4".to_string());
            }
            // Fallback to existing simple recipes if no workbench yet.
            if let Some(name) = self.inventory.quick_craft() {
                return Some(name.to_string());
            }
            return None;
        }

        // Craft tools at the workbench (workbench itself is only required, not consumed).
        const TOOL_RECIPES: [(WoodenTool, u16); 5] = [
            (WoodenTool::Pickaxe, 3),
            (WoodenTool::Axe, 3),
            (WoodenTool::Shovel, 1),
            (WoodenTool::Hoe, 2),
            (WoodenTool::Sword, 2),
        ];

        for (tool, logs_required) in TOOL_RECIPES {
            if self.tool_durability(tool) > 0 {
                continue;
            }
            if self.inventory.count_block(Block::Log) < logs_required {
                continue;
            }
            if !self.inventory.consume_blocks(Block::Log, logs_required) {
                continue;
            }

            self.wood_tool_durability[tool.idx()] = item_registry::tool_max_durability(tool);
            if self.equipped_tool.is_none() {
                self.equipped_tool = Some(tool);
            }
            return Some(tool.display_name().to_string());
        }

        self.inventory.quick_craft().map(|s| s.to_string())
    }

    fn cycle_wooden_tool(&mut self) {
        let start = self.inventory.selected;
        let len = self.inventory.hotbar.len();
        let mut selected: Option<(usize, WoodenTool)> = None;

        for step in 1..=len {
            let idx = (start + step) % len;
            let Some(tool) = self.inventory.hotbar[idx].tool else {
                continue;
            };
            if self.tool_durability(tool) == 0 {
                let _ = self.inventory.remove_one_tool(tool);
                continue;
            }
            selected = Some((idx, tool));
            break;
        }

        self.equipped_tool = selected.map(|(idx, tool)| {
            self.inventory.select_hotbar_slot(idx);
            tool
        });

        match self.equipped_tool {
            Some(tool) => log::info!(
                "Selected tool: {} ({}/{})",
                tool.display_name(),
                self.tool_durability(tool),
                item_registry::tool_max_durability(tool)
            ),
            None => log::info!("Selected tool: Block placement"),
        }
    }

    fn tool_durability(&self, tool: WoodenTool) -> u16 {
        self.wood_tool_durability[tool.idx()]
    }

    fn sync_equipped_tool_from_hotbar(&mut self) {
        let Some(tool) = self.inventory.selected_tool() else {
            self.equipped_tool = None;
            return;
        };

        if self.tool_durability(tool) == 0 {
            let _ = self.inventory.remove_one_tool(tool);
            self.equipped_tool = None;
            return;
        }
        self.equipped_tool = Some(tool);
    }

    fn consume_equipped_tool_durability(&mut self, amount: u16) {
        let Some(tool) = self.equipped_tool else {
            return;
        };
        let slot = &mut self.wood_tool_durability[tool.idx()];
        if *slot == 0 {
            self.equipped_tool = None;
            return;
        }
        *slot = slot.saturating_sub(amount);
        if *slot == 0 {
            let _ = self.inventory.remove_one_tool(tool);
            self.equipped_tool = None;
            if let Some(sound) = self.sound_system.as_mut() {
                sound.play_ambient_family(&["wood", "break"], 0.30);
            }
        }
    }

    fn handle_block_interaction(&mut self, dt: f32) {
        let place_req = std::mem::take(&mut self.place_block_requested);
        let hit = self.raycast_block_target(BLOCK_BREAK_RANGE);
        let mut changed = false;
        let attack_pressed = self.left_mouse_down && !self.left_mouse_was_down;

        if attack_pressed && self.try_attack_mob() {
            self.breaking = None;
            self.left_mouse_was_down = self.left_mouse_down;
            return;
        }

        if self.left_mouse_down {
            if let Some(hit_data) = hit.as_ref() {
                changed |= self.update_block_break_progress(hit_data.block, hit_data.normal, dt);
            } else {
                self.breaking = None;
            }
        } else {
            self.breaking = None;
        }

        if place_req {
            if let Some(hit_data) = hit.as_ref() {
                let target_block =
                    self.world
                        .block_at_world(hit_data.block.0, hit_data.block.1, hit_data.block.2);
                if target_block == Block::Workbench {
                    self.inventory.open_workbench();
                    self.left_mouse_down = false;
                    self.left_mouse_was_down = false;
                    self.breaking = None;
                    self.grab_cursor(false);
                    self.left_mouse_was_down = self.left_mouse_down;
                    return;
                }
                if target_block == Block::Furnace {
                    self.inventory.open_furnace();
                    self.left_mouse_down = false;
                    self.left_mouse_was_down = false;
                    self.breaking = None;
                    self.grab_cursor(false);
                    self.left_mouse_was_down = self.left_mouse_down;
                    return;
                }
            }

            if self.equipped_tool.is_some_and(|tool| tool.is_hoe()) {
                changed |= self.try_use_hoe(hit.as_ref());
            } else if let Some(hit_data) = hit.as_ref() {
                if let Some((px, py, pz)) = hit_data.place {
                    if let Some(block) = self.inventory.selected_block() {
                        let support_ok = if block == Block::Torch {
                            py > 0 && self.world.block_at_world(px, py - 1, pz).is_solid()
                        } else {
                            true
                        };
                        if !block.is_air()
                            && support_ok
                            && self.world.block_at_world(px, py, pz).is_air()
                            && !self.player.overlaps_block(px, py, pz)
                        {
                            if self.world.set_block_at_world(px, py, pz, block) {
                                changed = true;
                                let _ = self.inventory.consume_selected_one();
                            }
                        }
                    }
                }
            }
        }

        if changed {
            self.world.save_if_dirty();
        }
        self.left_mouse_was_down = self.left_mouse_down;
    }

    fn try_use_hoe(&mut self, hit: Option<&BlockRayHit>) -> bool {
        let Some(hit_data) = hit else {
            return false;
        };
        let (bx, by, bz) = hit_data.block;
        let target = self.world.block_at_world(bx, by, bz);
        if !matches!(
            target,
            Block::Dirt | Block::Grass | Block::FarmlandDry | Block::FarmlandWet
        ) {
            return false;
        }
        if !self.world.block_at_world(bx, by + 1, bz).is_air() {
            return false;
        }

        let tilled = if self.rain_strength > 0.20 {
            Block::FarmlandWet
        } else {
            Block::FarmlandDry
        };
        if target == tilled {
            return false;
        }

        let changed = self.world.set_block_at_world(bx, by, bz, tilled);
        if changed {
            if let Some(sound) = self.sound_system.as_mut() {
                sound.play_block_break(target);
            }
            self.consume_equipped_tool_durability(1);
        }
        changed
    }

    fn update_weather(&mut self, dt: f32) {
        if dt <= f32::EPSILON {
            return;
        }

        self.rain_anim_time += dt;
        self.weather_timer -= dt;
        if self.weather_timer <= 0.0 {
            self.rain_active = !self.rain_active;
            self.weather_timer = self.random_weather_duration(self.rain_active);
        }

        let target = if self.rain_active { 1.0 } else { 0.0 };
        let blend = (dt * if self.rain_active { 0.42 } else { 0.24 }).clamp(0.0, 1.0);
        self.rain_strength += (target - self.rain_strength) * blend;

        let wet_target = if self.rain_strength > 0.12 { 1.0 } else { 0.0 };
        let wet_rate = if wet_target > self.surface_wetness { 0.40 } else { 0.065 };
        let wet_blend = (dt * wet_rate).clamp(0.0, 1.0);
        self.surface_wetness += (wet_target - self.surface_wetness) * wet_blend;

        self.farmland_scan_timer -= dt;
        if self.farmland_scan_timer <= 0.0 {
            self.farmland_scan_timer = FARMLAND_SCAN_INTERVAL;
            self.apply_environment_moisture();
        }
    }

    fn random_weather_duration(&mut self, raining: bool) -> f32 {
        let t = self.next_weather_rand();
        if raining {
            WEATHER_MIN_RAIN + (WEATHER_MAX_RAIN - WEATHER_MIN_RAIN) * t
        } else {
            WEATHER_MIN_CLEAR + (WEATHER_MAX_CLEAR - WEATHER_MIN_CLEAR) * t
        }
    }

    fn next_weather_rand(&mut self) -> f32 {
        let mut x = self.weather_rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        if x == 0 {
            x = 0x6D2B_79F5;
        }
        self.weather_rng = x;
        (x as f32) / (u32::MAX as f32)
    }

    fn apply_environment_moisture(&mut self) {
        let px = self.player.pos.x.floor() as i32;
        let pz = self.player.pos.z.floor() as i32;
        let rain_wet = self.rain_strength > 0.20;

        for wx in (px - FARMLAND_SCAN_RADIUS)..=(px + FARMLAND_SCAN_RADIUS) {
            for wz in (pz - FARMLAND_SCAN_RADIUS)..=(pz + FARMLAND_SCAN_RADIUS) {
                let dx = wx - px;
                let dz = wz - pz;
                if dx * dx + dz * dz > FARMLAND_SCAN_RADIUS * FARMLAND_SCAN_RADIUS {
                    continue;
                }

                let Some(surface) = self.world.surface_at_world(wx, wz) else {
                    continue;
                };
                let sy = surface as i32;
                let ys = [sy - 1, sy, sy + 1];
                for wy in ys {
                    if wy < 0 {
                        continue;
                    }
                    if self.world.block_at_world(wx, wy, wz) == Block::FarmlandDry
                        && (rain_wet || self.farmland_has_nearby_water(wx, wy, wz))
                    {
                        let _ = self.world.set_block_at_world(wx, wy, wz, Block::FarmlandWet);
                    }
                }
            }
        }
    }

    fn farmland_has_nearby_water(&self, wx: i32, wy: i32, wz: i32) -> bool {
        let rr = FARMLAND_WATER_RADIUS * FARMLAND_WATER_RADIUS;
        for ny in (wy - FARMLAND_WATER_VERTICAL)..=(wy + FARMLAND_WATER_VERTICAL) {
            if ny < 0 {
                continue;
            }
            for nx in (wx - FARMLAND_WATER_RADIUS)..=(wx + FARMLAND_WATER_RADIUS) {
                for nz in (wz - FARMLAND_WATER_RADIUS)..=(wz + FARMLAND_WATER_RADIUS) {
                    let dx = nx - wx;
                    let dz = nz - wz;
                    if dx * dx + dz * dz > rr {
                        continue;
                    }
                    if self.world.block_at_world(nx, ny, nz) == Block::Water {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn update_block_break_progress(&mut self, target: (i32, i32, i32), face: (i32, i32, i32), dt: f32) -> bool {
        let block = self.world.block_at_world(target.0, target.1, target.2);
        if !block.is_breakable() {
            self.breaking = None;
            return false;
        }

        let required = block_break_seconds(block, self.equipped_tool);
        match self.breaking.as_mut() {
            Some(state) if state.block == target => {
                state.elapsed += dt;
                state.required = required;
                state.face = face;
            }
            _ => {
                self.breaking = Some(BreakingState {
                    block: target,
                    face,
                    elapsed: 0.0,
                    required,
                });
                return false;
            }
        }

        let should_break = self
            .breaking
            .as_ref()
            .map(|s| s.elapsed >= s.required)
            .unwrap_or(false);
        if !should_break {
            return false;
        }

        let broken = self.world.block_at_world(target.0, target.1, target.2);
        self.breaking = None;
        if !broken.is_breakable() {
            return false;
        }
        if !self.world.set_block_at_world(target.0, target.1, target.2, Block::Air) {
            return false;
        }

        if let Some(sound) = self.sound_system.as_mut() {
            sound.play_block_break(broken);
        }
        if let Some(drop) = broken.drop_item() {
            self.spawn_drop(drop, 1, target);
        }
        self.consume_equipped_tool_durability(1);
        true
    }

    fn spawn_drop(&mut self, block: Block, mut count: u16, at: (i32, i32, i32)) {
        if block.is_air() || count == 0 {
            return;
        }

        while count > 0 {
            let stack = count.min(64);
            count -= stack;

            let entropy = drop_entropy(
                at.0,
                at.1,
                at.2,
                item_registry::block_item_id(block),
                self.dropped_items.len() as u32,
            );
            let rx = (((entropy & 0xff) as f32 / 255.0) - 0.5) * 0.28;
            let rz = ((((entropy >> 8) & 0xff) as f32 / 255.0) - 0.5) * 0.28;
            let up = 2.6 + (((entropy >> 16) & 0xff) as f32 / 255.0) * 1.1;

            self.dropped_items.push(DroppedItem {
                block,
                count: stack,
                pos: glam::Vec3::new(at.0 as f32 + 0.5 + rx, at.1 as f32 + 0.62, at.2 as f32 + 0.5 + rz),
                vel: glam::Vec3::new(rx * 4.0, up, rz * 4.0),
                age: 0.0,
                pickup_delay: DROP_PICKUP_DELAY,
            });
        }
    }

    fn update_dropped_items(&mut self, dt: f32) {
        let pickup_origin = self.player.pos + glam::Vec3::new(0.0, self.player.height * 0.5, 0.0);
        let pickup_radius_sq = DROP_PICKUP_RADIUS * DROP_PICKUP_RADIUS;

        let mut i = 0usize;
        while i < self.dropped_items.len() {
            {
                let drop = &mut self.dropped_items[i];
                drop.age += dt;
                drop.pickup_delay = (drop.pickup_delay - dt).max(0.0);
                let magnetized = if drop.pickup_delay <= 0.0 {
                    apply_drop_magnet(drop, pickup_origin, dt)
                } else {
                    false
                };
                if !magnetized {
                    simulate_drop_physics(drop, dt, &self.world);
                }
            }

            let mut remove = false;
            if self.dropped_items[i].age >= DROP_DESPAWN_SECS || self.dropped_items[i].count == 0 {
                remove = true;
            } else if self.dropped_items[i].pickup_delay <= 0.0
                && self.dropped_items[i].pos.distance_squared(pickup_origin) <= pickup_radius_sq
            {
                let block = self.dropped_items[i].block;
                let count = self.dropped_items[i].count;
                let remaining = self.inventory.add_block(block, count);
                if remaining < count {
                    if let Some(sound) = self.sound_system.as_mut() {
                        sound.play_item_pickup();
                    }
                }
                if remaining == 0 {
                    remove = true;
                } else {
                    self.dropped_items[i].count = remaining;
                    self.dropped_items[i].pickup_delay = 0.08;
                }
            }

            if remove {
                self.dropped_items.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    fn update_mobs(&mut self, dt: f32) {
        if dt <= f32::EPSILON {
            return;
        }

        self.mob_spawn_timer -= dt;
        if self.mob_spawn_timer <= 0.0 {
            self.try_spawn_mob();
            let t = self.next_weather_rand();
            self.mob_spawn_timer =
                MOB_SPAWN_INTERVAL_MIN + (MOB_SPAWN_INTERVAL_MAX - MOB_SPAWN_INTERVAL_MIN) * t;
        }

        let player_center = self.player.pos + glam::Vec3::new(0.0, self.player.height * 0.5, 0.0);
        let mut pending_damage = 0.0f32;

        for mob in &mut self.mobs {
            mob.age += dt;
            mob.attack_cooldown = (mob.attack_cooldown - dt).max(0.0);

            let mob_center = mob.pos + glam::Vec3::new(0.0, 0.46, 0.0);
            let to_player = player_center - mob_center;
            let dist = to_player.length();

            let speed = if dist <= MOB_CHASE_RANGE { 3.1 } else { 1.1 };
            let target_dir = if dist <= MOB_CHASE_RANGE {
                let dir = glam::Vec2::new(to_player.x, to_player.z).normalize_or_zero();
                let wiggle = glam::Vec2::new(
                    (mob.age * 1.31 + mob.wander_phase).sin(),
                    (mob.age * 1.06 + mob.wander_phase * 1.19).cos(),
                ) * 0.13;
                (dir + wiggle).normalize_or_zero()
            } else {
                glam::Vec2::new(
                    (mob.age * 0.41 + mob.wander_phase).sin(),
                    (mob.age * 0.35 + mob.wander_phase * 0.73).cos(),
                )
                .normalize_or_zero()
            };

            let vel_lerp = (dt * 6.0).clamp(0.0, 1.0);
            mob.vel.x += (target_dir.x * speed - mob.vel.x) * vel_lerp;
            mob.vel.z += (target_dir.y * speed - mob.vel.z) * vel_lerp;
            mob.vel.y = (mob.vel.y - 24.0 * dt).max(-21.0);

            simulate_mob_physics(mob, dt, &self.world);

            let mob_center = mob.pos + glam::Vec3::new(0.0, 0.46, 0.0);
            let hit_dist = (player_center - mob_center).length();
            let vertical_ok = (player_center.y - mob_center.y).abs() <= 1.15;
            let los_ok = line_of_sight_clear(
                &self.world,
                mob_center + glam::Vec3::new(0.0, 0.10, 0.0),
                player_center + glam::Vec3::new(0.0, 0.10, 0.0),
            );
            if hit_dist <= MOB_ATTACK_RANGE
                && vertical_ok
                && los_ok
                && mob.attack_cooldown <= 0.0
                && self.player_hurt_timer <= 0.0
            {
                pending_damage += MOB_ATTACK_DAMAGE;
                mob.attack_cooldown = MOB_ATTACK_COOLDOWN;
                let knock_dir =
                    glam::Vec2::new(self.player.pos.x - mob.pos.x, self.player.pos.z - mob.pos.z)
                        .normalize_or_zero();
                mob.vel.x -= knock_dir.x * 1.4;
                mob.vel.z -= knock_dir.y * 1.4;
            }
        }

        if pending_damage > 0.0 {
            self.apply_player_damage(pending_damage);
        }

        let mut i = 0usize;
        while i < self.mobs.len() {
            let mob = &self.mobs[i];
            let dist = self.camera.pos.distance(mob.pos);
            let dead = mob.hp <= 0.0;
            let despawn = dist > MOB_DESPAWN_RANGE || mob.pos.y < -40.0;
            if dead || despawn {
                if dead {
                    let h = hash32(
                        (mob.pos.x.floor() as i32 as u32).wrapping_mul(0x9E37_79B9)
                            ^ (mob.pos.z.floor() as i32 as u32).wrapping_mul(0x85EB_CA6B)
                            ^ (mob.age as u32).wrapping_mul(0xC2B2_AE35),
                    );
                    let count = 1 + ((h >> 3) & 1) as u16;
                    self.spawn_drop(
                        Block::Coal,
                        count,
                        (
                            mob.pos.x.floor() as i32,
                            mob.pos.y.floor() as i32,
                            mob.pos.z.floor() as i32,
                        ),
                    );
                }
                self.mobs.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    fn try_spawn_mob(&mut self) {
        if self.mobs.len() >= MOB_MAX_COUNT {
            return;
        }

        let center = self.player.pos;
        for _ in 0..9 {
            let ang = self.next_weather_rand() * std::f32::consts::TAU;
            let dist = MOB_SPAWN_RANGE_MIN
                + (MOB_SPAWN_RANGE_MAX - MOB_SPAWN_RANGE_MIN) * self.next_weather_rand();
            let wx = (center.x + ang.cos() * dist).floor() as i32;
            let wz = (center.z + ang.sin() * dist).floor() as i32;
            let Some(surface) = self.world.surface_at_world(wx, wz) else {
                continue;
            };
            let sy = surface as i32;
            if sy <= 1 || sy >= 250 {
                continue;
            }

            let ground = self.world.block_at_world(wx, sy, wz);
            if matches!(ground, Block::Water | Block::Bedrock) || ground.is_air() {
                continue;
            }
            if !self.world.block_at_world(wx, sy + 1, wz).is_air()
                || !self.world.block_at_world(wx, sy + 2, wz).is_air()
            {
                continue;
            }

            let pos = glam::Vec3::new(wx as f32 + 0.5, sy as f32 + 1.0, wz as f32 + 0.5);
            if pos.distance(center) < MOB_SPAWN_RANGE_MIN * 0.8 {
                continue;
            }

            let wander_phase = self.next_weather_rand() * std::f32::consts::TAU;
            self.mobs.push(MobEntity {
                pos,
                vel: glam::Vec3::ZERO,
                hp: 10.0,
                attack_cooldown: 0.0,
                wander_phase,
                age: 0.0,
            });
            break;
        }
    }

    fn try_attack_mob(&mut self) -> bool {
        if self.player_attack_cooldown > 0.0 {
            return false;
        }
        let Some((idx, _)) = self.raycast_mob_target(3.15) else {
            return false;
        };
        if idx >= self.mobs.len() {
            return false;
        }

        let knock = self.camera.forward();
        let damage = match self.equipped_tool {
            Some(WoodenTool::IronSword) => MOB_HIT_DAMAGE_IRON_SWORD,
            Some(WoodenTool::StoneSword) => MOB_HIT_DAMAGE_STONE_SWORD,
            Some(tool) if tool.is_sword() => MOB_HIT_DAMAGE_WOOD_SWORD,
            _ => MOB_HIT_DAMAGE_HAND,
        };
        if let Some(mob) = self.mobs.get_mut(idx) {
            mob.hp -= damage;
            mob.vel += knock * 4.2 + glam::Vec3::Y * 1.6;
        }
        if self.equipped_tool.is_some_and(|tool| tool.is_sword()) {
            self.consume_equipped_tool_durability(1);
        }
        self.player_attack_cooldown = PLAYER_ATTACK_COOLDOWN;
        if let Some(sound) = self.sound_system.as_mut() {
            sound.play_ambient_family(&["classic_hurt", "stone", "gravel"], 0.30);
        }
        true
    }

    fn raycast_mob_target(&self, max_dist: f32) -> Option<(usize, f32)> {
        let ro = self.camera.pos;
        let rd = self.camera.forward().normalize_or_zero();
        if rd.length_squared() <= 0.0001 {
            return None;
        }

        let mut best: Option<(usize, f32)> = None;
        for i in 0..self.mobs.len() {
            let center = self.mobs[i].pos + glam::Vec3::new(0.0, 0.46, 0.0);
            if let Some(t) = ray_sphere_intersection(ro, rd, center, 0.46) {
                if t < 0.0 || t > max_dist {
                    continue;
                }
                match best {
                    Some((_, best_t)) if t >= best_t => {}
                    _ => best = Some((i, t)),
                }
            }
        }
        best
    }

    fn update_ambient_audio(&mut self, dt: f32) {
        if dt <= f32::EPSILON {
            return;
        }
        self.ambient_timer -= dt;
        self.bg_music_timer -= dt;
        self.thunder_timer -= dt;

        if self.bg_music_timer <= 0.0 {
            let cave_factor = self.cave_mood_environment_factor();
            let music_volume = if self.rain_strength > 0.35 {
                0.09
            } else if cave_factor > 0.22 {
                0.08
            } else {
                0.15
            };
            if let Some(sound) = self.sound_system.as_mut() {
                sound.play_background_music(music_volume);
            }
            self.bg_music_timer = BG_MUSIC_INTERVAL_MIN
                + (BG_MUSIC_INTERVAL_MAX - BG_MUSIC_INTERVAL_MIN) * self.next_weather_rand();
        }

        if self.ambient_timer <= 0.0 {
            if let Some(sound) = self.sound_system.as_mut() {
                if self.rain_strength > 0.24 {
                    let volume = (0.14 + self.rain_strength * 0.20).clamp(0.12, 0.42);
                    sound.play_ambient_family(&["rain", "water", "cave", "stone"], volume);
                } else {
                    let wx = self.player.pos.x.floor() as i32;
                    let wz = self.player.pos.z.floor() as i32;
                    if let Some(biome) = self.world.biome_at_world(wx, wz) {
                        let families = ambient_families_for_biome(biome);
                        sound.play_ambient_family(families, 0.12);
                    }
                }
            }
            self.ambient_timer = AMBIENT_INTERVAL_MIN
                + (AMBIENT_INTERVAL_MAX - AMBIENT_INTERVAL_MIN) * self.next_weather_rand();
        }

        if self.rain_strength > 0.58 && self.thunder_timer <= 0.0 {
            if let Some(sound) = self.sound_system.as_mut() {
                sound.play_ambient_family(&["thunder", "cave", "stone"], 0.24);
            }
            self.thunder_timer = THUNDER_INTERVAL_MIN
                + (THUNDER_INTERVAL_MAX - THUNDER_INTERVAL_MIN) * self.next_weather_rand();
        }
    }

    fn update_cave_mood(&mut self, dt: f32) {
        if dt <= f32::EPSILON {
            return;
        }

        self.cave_mood_cooldown = (self.cave_mood_cooldown - dt).max(0.0);
        let cave_factor = self.cave_mood_environment_factor();
        if cave_factor > 0.04 {
            self.cave_mood_surface_hold = CAVE_MOOD_SURFACE_HOLD_SECS;
            let gain = CAVE_MOOD_INCREASE_MIN
                + (CAVE_MOOD_INCREASE_MAX - CAVE_MOOD_INCREASE_MIN) * cave_factor;
            self.cave_mood_percent = (self.cave_mood_percent + gain * dt).min(CAVE_MOOD_TRIGGER);
        } else {
            if self.cave_mood_percent > 0.0 && self.cave_mood_surface_hold > 0.0 {
                self.cave_mood_surface_hold = (self.cave_mood_surface_hold - dt).max(0.0);
                return;
            }
            self.cave_mood_percent = (self.cave_mood_percent - CAVE_MOOD_DECAY * dt).max(0.0);
            return;
        }

        if self.cave_mood_percent < CAVE_MOOD_TRIGGER || self.cave_mood_cooldown > 0.0 {
            return;
        }

        let volume = (0.14 + cave_factor * 0.22).clamp(0.12, 0.42);
        let next_cooldown = CAVE_MOOD_COOLDOWN_MIN
            + (CAVE_MOOD_COOLDOWN_MAX - CAVE_MOOD_COOLDOWN_MIN) * self.next_weather_rand();
        if let Some(sound) = self.sound_system.as_mut() {
            sound.play_cave_mood(volume);
        }

        self.cave_mood_percent = 0.0;
        self.cave_mood_surface_hold = 0.0;
        self.cave_mood_cooldown = next_cooldown;
    }

    fn cave_mood_environment_factor(&self) -> f32 {
        let eye = self.player.eye_pos();
        let wx = eye.x.floor() as i32;
        let wy = eye.y.floor() as i32;
        let wz = eye.z.floor() as i32;

        let here = self.world.block_at_world(wx, wy, wz);
        if here.is_solid() {
            return 0.0;
        }

        let Some(surface) = self.world.surface_at_world(wx, wz) else {
            return 0.0;
        };
        let depth = (surface as i32 - wy).max(0) as f32;
        if depth < 3.0 {
            return 0.0;
        }
        let depth_factor = ((depth - 3.0) / 32.0).clamp(0.0, 1.0);

        let mut cave_air_hits = 0usize;
        let mut sample_count = 0usize;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    sample_count += 1;
                    if self.world.block_at_world(wx + dx, wy + dy, wz + dz) == Block::CaveAir {
                        cave_air_hits += 1;
                    }
                }
            }
        }
        let cave_density = cave_air_hits as f32 / sample_count as f32;
        if cave_density <= 0.01 {
            return 0.0;
        }

        let mut roof_blocks = 0usize;
        for dy in 1..=8 {
            if self.world.is_solid_at_world(wx, wy + dy, wz) {
                roof_blocks += 1;
            }
        }
        let roof_factor = roof_blocks as f32 / 8.0;
        (depth_factor * (cave_density * 0.82 + roof_factor * 0.18)).clamp(0.0, 1.0)
    }

    fn update_survival_stats(&mut self, dt: f32, moving: bool) {
        if dt <= f32::EPSILON {
            return;
        }

        let mut hunger_drain = HUNGER_IDLE_DRAIN;
        if moving {
            hunger_drain += HUNGER_MOVE_DRAIN;
        }
        if self.sprinting {
            hunger_drain += HUNGER_SPRINT_DRAIN;
        }
        self.player_hunger = (self.player_hunger - hunger_drain * dt).clamp(0.0, PLAYER_MAX_HUNGER);

        if self.sprinting {
            self.player_stamina = (self.player_stamina - STAMINA_SPRINT_DRAIN * dt).max(0.0);
            if self.player_stamina <= 0.01 {
                self.sprinting = false;
            }
        } else {
            let mut recovery = STAMINA_RECOVERY;
            if moving {
                recovery *= 0.68;
            }
            if self.player_hunger <= 0.0 {
                recovery *= 0.45;
            }
            self.player_stamina = (self.player_stamina + recovery * dt).min(PLAYER_MAX_STAMINA);
        }

        if self.player_health < PLAYER_MAX_HEALTH && self.player_hunger >= HUNGER_REGEN_THRESHOLD {
            self.health_regen_timer += dt;
            if self.health_regen_timer >= HUNGER_REGEN_INTERVAL {
                self.health_regen_timer -= HUNGER_REGEN_INTERVAL;
                self.player_health = (self.player_health + 1.0).min(PLAYER_MAX_HEALTH);
                self.player_hunger = (self.player_hunger - HUNGER_REGEN_COST).max(0.0);
            }
        } else {
            self.health_regen_timer = 0.0;
        }

        if self.player_hunger <= 0.0 {
            self.starvation_timer += dt;
            if self.starvation_timer >= STARVATION_INTERVAL {
                self.starvation_timer -= STARVATION_INTERVAL;
                if self.player_hurt_timer <= 0.0 {
                    self.apply_player_damage(1.0);
                }
            }
        } else {
            self.starvation_timer = 0.0;
        }
    }

    fn update_fall_damage(&mut self, was_on_ground: bool) {
        if self.player.on_ground {
            if !was_on_ground {
                let fall_distance = (self.fall_peak_y - self.player.pos.y).max(0.0);
                let blocks_fallen = fall_distance.floor();
                let blocks_over_free = blocks_fallen - FALL_DAMAGE_FREE;
                if blocks_over_free >= 1.0 && self.player_hurt_timer <= 0.0 {
                    let damage = (blocks_over_free * FALL_DAMAGE_PER_BLOCK).max(1.0);
                    self.apply_player_damage(damage);
                }
            }
            self.fall_peak_y = self.player.pos.y;
            return;
        }

        if self.player.pos.y > self.fall_peak_y {
            self.fall_peak_y = self.player.pos.y;
        }
    }

    fn apply_player_damage(&mut self, damage: f32) {
        if damage <= 0.0 {
            return;
        }
        self.player_health = (self.player_health - damage).max(0.0);
        self.player_hurt_timer = PLAYER_HURT_IFRAMES;
        if let Some(sound) = self.sound_system.as_mut() {
            sound.play_player_hurt();
        }
        if self.player_health <= 0.0 {
            self.respawn_player();
        }
    }

    fn respawn_player(&mut self) {
        let (sx, sy, sz) = spawn_point(self.world_seed);
        self.player.teleport(glam::Vec3::new(sx, sy, sz));
        self.update_camera_transform();
        self.player_health = PLAYER_MAX_HEALTH;
        self.player_hunger = PLAYER_MAX_HUNGER;
        self.player_stamina = PLAYER_MAX_STAMINA;
        self.player_hurt_timer = 0.75;
        self.health_regen_timer = 0.0;
        self.starvation_timer = 0.0;
        self.fall_peak_y = self.player.pos.y;
        self.sprinting = false;
        self.cave_mood_surface_hold = 0.0;
        self.left_mouse_down = false;
        self.left_mouse_was_down = false;
        self.breaking = None;
    }

    fn update_footstep_audio(&mut self, dt: f32, prev_feet: glam::Vec3) {
        if dt <= f32::EPSILON {
            return;
        }

        let moved = glam::Vec2::new(self.player.pos.x - prev_feet.x, self.player.pos.z - prev_feet.z);
        let horizontal_speed = moved.length() / dt;
        let is_moving = horizontal_speed > 0.25;

        if !self.player.on_ground || !is_moving {
            self.footstep_timer = 0.0;
            return;
        }

        self.footstep_timer -= dt;
        if self.footstep_timer > 0.0 {
            return;
        }

        let wx = self.player.pos.x.floor() as i32;
        let wy = (self.player.pos.y - 0.05).floor() as i32;
        let wz = self.player.pos.z.floor() as i32;
        let surface = self.world.block_at_world(wx, wy, wz);

        if let Some(sound) = self.sound_system.as_mut() {
            sound.play_footstep(surface);
        }

        let pace = (horizontal_speed / self.walk_speed.max(0.1)).clamp(0.55, 1.45);
        self.footstep_timer = (0.42 / pace).clamp(0.20, 0.58);
    }

    fn update_hand_animation(&mut self, dt: f32, prev_feet: glam::Vec3) {
        if dt <= f32::EPSILON {
            return;
        }

        let moved = glam::Vec2::new(self.player.pos.x - prev_feet.x, self.player.pos.z - prev_feet.z);
        let horizontal_speed = moved.length() / dt;
        let moving = self.player.on_ground && horizontal_speed > 0.08;
        let target_swing = if moving {
            (horizontal_speed / self.walk_speed.max(0.1)).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let blend = (dt * 8.0).clamp(0.0, 1.0);
        self.hand_swing += (target_swing - self.hand_swing) * blend;

        let phase_speed = if moving {
            6.0 + self.hand_swing * 7.5
        } else {
            1.2
        };
        self.hand_phase_rad += dt * phase_speed;
        if self.hand_phase_rad > std::f32::consts::TAU * 2.0 {
            self.hand_phase_rad -= std::f32::consts::TAU * 2.0;
        }

        let attacking = self.left_mouse_down && !self.menu.open && !self.inventory.open;
        let target_action = if attacking { 1.0 } else { 0.0 };
        let action_blend = (dt * 11.0).clamp(0.0, 1.0);
        self.hand_action_strength += (target_action - self.hand_action_strength) * action_blend;
        self.hand_action_phase_rad += dt * (9.0 + self.hand_action_strength * 8.0);
        if self.hand_action_phase_rad > std::f32::consts::TAU * 2.0 {
            self.hand_action_phase_rad -= std::f32::consts::TAU * 2.0;
        }
    }

    fn collect_hand_visual(&self) -> FirstPersonHandVisual {
        let held_tool = self.equipped_tool;
        let held_block = if held_tool.is_some() {
            None
        } else {
            self.inventory.selected_block()
        };
        FirstPersonHandVisual {
            phase_rad: self.hand_phase_rad,
            swing: self.hand_swing,
            action_phase_rad: self.hand_action_phase_rad,
            action_strength: self.hand_action_strength,
            held_tool,
            held_block,
        }
    }

    fn collect_player_visual(&self) -> PlayerVisual {
        let held_tool = self.equipped_tool;
        let held_block = if held_tool.is_some() {
            None
        } else {
            self.inventory.selected_block()
        };
        PlayerVisual {
            feet_pos: self.player.pos,
            yaw_rad: self.camera.yaw.to_radians(),
            phase_rad: self.hand_phase_rad,
            swing: self.hand_swing,
            action_phase_rad: self.hand_action_phase_rad,
            action_strength: self.hand_action_strength,
            held_tool,
            held_block,
        }
    }

    fn collect_break_overlay_visual(&self) -> Option<BreakOverlayVisual> {
        let state = self.breaking.as_ref()?;
        if state.required <= f32::EPSILON {
            return None;
        }
        let progress = (state.elapsed / state.required).clamp(0.0, 1.0);
        if progress <= 0.0 {
            return None;
        }
        Some(BreakOverlayVisual {
            block: state.block,
            face_normal: state.face,
            progress,
        })
    }

    fn collect_block_outline_visual(&self) -> Option<BlockOutlineVisual> {
        let hit = self.raycast_block_target(BLOCK_BREAK_RANGE)?;
        Some(BlockOutlineVisual { block: hit.block })
    }

    fn collect_target_hud_info(&self) -> Option<TargetHudInfo> {
        let block_hit = self.raycast_block_target(BLOCK_BREAK_RANGE);
        let mob_hit = self.raycast_mob_target(BLOCK_BREAK_RANGE);

        if let Some((mob_idx, mob_dist)) = mob_hit {
            if let Some(mob) = self.mobs.get(mob_idx) {
                let mob_center = mob.pos + glam::Vec3::new(0.0, 0.46, 0.0);
                let los_ok = line_of_sight_clear(
                    &self.world,
                    self.camera.pos + glam::Vec3::new(0.0, 0.06, 0.0),
                    mob_center + glam::Vec3::new(0.0, 0.08, 0.0),
                );
                let block_dist = block_hit
                    .as_ref()
                    .map(|h| h.distance)
                    .unwrap_or(f32::INFINITY);
                if los_ok && mob_dist <= block_dist + 0.02 {
                    return Some(TargetHudInfo::Mob {
                        name: "Cave Mob",
                        hp: mob.hp.max(0.0),
                    });
                }
            }
        }

        let hit = block_hit?;
        let block = self.world.block_at_world(hit.block.0, hit.block.1, hit.block.2);
        if !block.is_breakable() {
            return None;
        }

        let progress = self
            .breaking
            .as_ref()
            .filter(|s| s.block == hit.block && s.required > f32::EPSILON)
            .map(|s| (s.elapsed / s.required).clamp(0.0, 1.0))
            .unwrap_or(0.0);

        Some(TargetHudInfo::Block {
            name: block_display_name(block),
            item_id: item_registry::block_item_id(block),
            break_progress: progress,
        })
    }

    fn collect_drop_visuals(&self) -> Vec<DroppedBlockVisual> {
        if self.dropped_items.is_empty() && self.mobs.is_empty() {
            return Vec::new();
        }

        let mut visuals = Vec::with_capacity(self.dropped_items.len() + self.mobs.len());
        for drop in &self.dropped_items {
            let bob = (drop.age * 6.0).sin() * 0.06;
            let pos = drop.pos + glam::Vec3::new(0.0, bob, 0.0);
            if self.camera.pos.distance(pos) > 48.0 {
                continue;
            }

            visuals.push(DroppedBlockVisual {
                pos,
                yaw_rad: drop.age * 1.85,
                scale: 0.27,
                block: drop.block,
            });
        }

        for mob in &self.mobs {
            if self.camera.pos.distance(mob.pos) > 60.0 {
                continue;
            }
            let yaw = if mob.vel.x.abs() + mob.vel.z.abs() > 0.02 {
                mob.vel.x.atan2(mob.vel.z)
            } else {
                mob.wander_phase
            };
            visuals.push(DroppedBlockVisual {
                pos: mob.pos + glam::Vec3::new(0.0, 0.18 + (mob.age * 4.0).sin() * 0.02, 0.0),
                yaw_rad: yaw,
                scale: 0.62,
                block: Block::Leaves,
            });
        }

        visuals
    }

    fn collect_rain_visuals(&self) -> Vec<RainDropVisual> {
        if self.rain_strength <= 0.03 {
            return Vec::new();
        }

        let mut count = (self.rain_strength * RAIN_RENDER_COUNT_MAX as f32).round() as usize;
        count = count.clamp(24, RAIN_RENDER_COUNT_MAX);

        let mut visuals = Vec::with_capacity(count);
        let center = self.camera.pos;
        let fall_phase = self.rain_anim_time * 14.0;

        for i in 0..count {
            let h0 = hash32(self.weather_rng ^ (i as u32).wrapping_mul(0x9E37_79B9));
            let h1 = hash32(h0 ^ 0x85EB_CA6B);
            let h2 = hash32(h1 ^ 0xC2B2_AE35);

            let fx = ((h0 & 0xFFFF) as f32 / 65535.0) * 2.0 - 1.0;
            let fz = (((h0 >> 16) & 0xFFFF) as f32 / 65535.0) * 2.0 - 1.0;
            if fx * fx + fz * fz > 1.0 {
                continue;
            }

            let wx = center.x + fx * RAIN_RENDER_RADIUS;
            let wz = center.z + fz * RAIN_RENDER_RADIUS;
            let speed = 10.0 + ((h1 & 0xFF) as f32 / 255.0) * 7.0;
            let cycle = 14.0;
            let phase_offset = ((h1 >> 8) & 0xFFFF) as f32 / 65535.0 * cycle;
            let wy = center.y + 8.0 - ((fall_phase * speed * 0.07 + phase_offset) % cycle);

            let wx_i = wx.floor() as i32;
            let wz_i = wz.floor() as i32;
            if let Some(surface) = self.world.surface_at_world(wx_i, wz_i) {
                let min_y = surface as f32 + 1.1;
                if wy < min_y {
                    continue;
                }
            }

            let len = 0.18 + ((h2 & 0x7F) as f32 / 127.0) * 0.34;
            let thickness = 0.010 + (((h2 >> 7) & 0x1F) as f32 / 31.0) * 0.010;
            let yaw_rad = 0.55 + (((h2 >> 12) & 0x3F) as f32 / 63.0) * 0.28;

            visuals.push(RainDropVisual {
                pos: glam::Vec3::new(wx, wy, wz),
                length: len,
                thickness,
                yaw_rad,
            });
        }

        visuals
    }

    fn raycast_block_target(&self, max_dist: f32) -> Option<BlockRayHit> {
        let origin = self.camera.pos;
        let dir = self.camera.forward();
        let step = 0.05f32;
        let mut t = 0.0f32;
        let mut last_air: Option<(i32, i32, i32)> = None;
        let mut prev_cell: Option<(i32, i32, i32)> = None;

        while t <= max_dist {
            let p = origin + dir * t;
            let cell = (
                p.x.floor() as i32,
                p.y.floor() as i32,
                p.z.floor() as i32,
            );

            if prev_cell == Some(cell) {
                t += step;
                continue;
            }
            prev_cell = Some(cell);

            let b = self.world.block_at_world(cell.0, cell.1, cell.2);
            if b.is_solid() {
                let normal = if let Some(air) = last_air {
                    (
                        (air.0 - cell.0).clamp(-1, 1),
                        (air.1 - cell.1).clamp(-1, 1),
                        (air.2 - cell.2).clamp(-1, 1),
                    )
                } else {
                    fallback_hit_normal(dir)
                };
                return Some(BlockRayHit {
                    block: cell,
                    place: last_air,
                    normal,
                    distance: t,
                });
            }
            last_air = Some(cell);
            t += step;
        }
        None
    }

    fn process_api_commands(&mut self) {
        while let Some(cmd) = self.mod_api_runtime.try_recv() {
            match cmd {
                ApiCommand::RegenerateWorld { seed } => self.regenerate_world(seed),
                ApiCommand::SetTimeOfDay(t) => {
                    self.day_time = wrap_day_time(t);
                }
                ApiCommand::AddTimeOfDay(delta) => {
                    self.day_time = wrap_day_time(self.day_time + delta);
                }
                ApiCommand::SetPlayerPosition { x, y, z } => {
                    self.player.teleport(glam::Vec3::new(x, y, z));
                    self.update_camera_transform();
                    self.jump_was_down = false;
                }
                ApiCommand::SetDebugOverlay(enabled) => {
                    self.debug_overlay = enabled;
                }
                ApiCommand::SetMenuOpen(open) => {
                    self.menu.open = open;
                    if open {
                        self.inventory.close();
                        self.left_mouse_down = false;
                        self.left_mouse_was_down = false;
                        self.breaking = None;
                        self.grab_cursor(false);
                    } else if !self.inventory.open {
                        self.grab_cursor(true);
                    }
                }
                ApiCommand::SetInventoryOpen(open) => {
                    if open {
                        self.inventory.open_player();
                    } else {
                        self.inventory.close();
                    }
                    if open {
                        self.menu.open = false;
                        self.left_mouse_down = false;
                        self.left_mouse_was_down = false;
                        self.breaking = None;
                        self.grab_cursor(false);
                    } else if !self.menu.open {
                        self.grab_cursor(true);
                    }
                }
                ApiCommand::SetMouseSensitivity(v) => {
                    let sens = v.clamp(0.01, 2.0);
                    self.menu.settings.mouse_sens = sens;
                    self.camera.sensitivity = sens;
                }
                ApiCommand::SetMoveSpeed(v) => {
                    let speed = v.clamp(1.0, 80.0);
                    self.walk_speed = speed;
                }
                ApiCommand::SetRayTracingEnabled(enabled) => {
                    self.renderer.set_ray_tracing_enabled(enabled);
                    self.menu.settings.ray_tracing = self.renderer.ray_tracing_enabled();
                    self.applied_ray_tracing = self.menu.settings.ray_tracing;
                }
                ApiCommand::SetBlock { x, y, z, block } => {
                    if self.world.set_block_at_world(x, y, z, block) {
                        self.world.save_if_dirty();
                    }
                }
                ApiCommand::QueryBlock { x, y, z, respond_to } => {
                    let _ = respond_to.send(self.world.block_at_world(x, y, z));
                }
                ApiCommand::QueryBiome { x, z, respond_to } => {
                    let _ = respond_to.send(self.world.biome_at_world(x, z));
                }
                ApiCommand::QuerySurfaceY { x, z, respond_to } => {
                    let _ = respond_to.send(self.world.surface_at_world(x, z));
                }
            }
        }
    }

    fn update_api_snapshot(&self) {
        let feet = self.player.pos;
        let eye = self.player.eye_pos();
        self.mod_api_runtime.update_snapshot(ApiSnapshot {
            world_seed: self.world_seed,
            day_time: self.day_time,
            player_feet: [feet.x, feet.y, feet.z],
            player_eye: [eye.x, eye.y, eye.z],
            chunks_loaded: self.world.chunk_count(),
            debug_overlay: self.debug_overlay,
            menu_open: self.menu.open,
            inventory_open: self.inventory.open,
            ray_tracing_enabled: self.renderer.ray_tracing_enabled(),
        });
    }

    fn collect_debug_overlay(&self, fps: f32, frame_dt: f32) -> DebugOverlayData {
        let feet = self.player.pos;
        let eye = self.camera.pos;

        let bx = feet.x.floor() as i32;
        let by = feet.y.floor() as i32;
        let bz = feet.z.floor() as i32;

        let cx = bx.div_euclid(CHUNK_W as i32);
        let cz = bz.div_euclid(CHUNK_D as i32);
        let lx = bx.rem_euclid(CHUNK_W as i32);
        let lz = bz.rem_euclid(CHUNK_D as i32);

        let (facing_name, axis_text) = facing_from_yaw(self.camera.yaw);
        let biome = self
            .world
            .biome_at_world(bx, bz)
            .map(biome_name)
            .unwrap_or_else(|| "Unloaded".to_string());
        let surface_y = self
            .world
            .surface_at_world(bx, bz)
            .map(|y| y.to_string())
            .unwrap_or_else(|| "--".to_string());

        let size = self.renderer.window().inner_size();
        let day_pct = self.day_time * 100.0;
        let mood_line = if self.cave_mood_surface_hold > 0.0 && self.cave_mood_percent > 0.0 {
            format!(
                "Cave mood: {:.0}% (hold {:.1}s)",
                self.cave_mood_percent,
                self.cave_mood_surface_hold
            )
        } else {
            format!("Cave mood: {:.0}%", self.cave_mood_percent)
        };
        let tool_line = match self.equipped_tool {
            Some(tool) => format!(
                "Tool: {} ({}/{})",
                tool.display_name(),
                self.tool_durability(tool),
                item_registry::tool_max_durability(tool)
            ),
            None => "Tool: Block".to_string(),
        };
        let held_item_line = if let Some(tool) = self.equipped_tool {
            let id = item_registry::tool_item_id(tool);
            let key = item_registry::item_key_from_id(id).unwrap_or("unknown");
            format!("Held item: {key} (id={id})")
        } else if let Some(block) = self.inventory.selected_block() {
            let id = item_registry::block_item_id(block);
            let key = item_registry::item_key_from_id(id).unwrap_or(block.item_key());
            format!("Held item: {key} (id={id})")
        } else {
            "Held item: none".to_string()
        };

        let left = vec![
            format!("MyEngine {:.0} fps ({:.2} ms)", fps, frame_dt * 1000.0),
            format!("XYZ: {:.3} / {:.3} / {:.3}", feet.x, eye.y, feet.z),
            format!("Block: {bx} {by} {bz}"),
            format!("Chunk: {cx} {cz} in {lx} {lz}"),
            format!("Facing: {facing_name} ({axis_text})"),
            format!("Perspective: {}", if self.third_person { "Third person" } else { "First person" }),
            format!(
                "Weather: {} ({:.0}%) wet {:.0}%",
                if self.rain_strength > 0.18 { "Rain" } else { "Clear" },
                self.rain_strength * 100.0,
                self.surface_wetness * 100.0
            ),
            mood_line,
            format!("HP: {:.0}/{}", self.player_health, PLAYER_MAX_HEALTH as i32),
            format!(
                "Food: {:.1}/{} | Stamina: {:.0}/{}{}",
                self.player_hunger,
                PLAYER_MAX_HUNGER as i32,
                self.player_stamina,
                PLAYER_MAX_STAMINA as i32,
                if self.sprinting { " (sprinting)" } else { "" }
            ),
            tool_line,
            held_item_line,
            format!("Rotation: {:.1} / {:.1}", self.camera.yaw, self.camera.pitch),
            format!("Biome: {biome}"),
            format!("Surface Y: {surface_y}"),
            format!("Day cycle: {:.0}%", day_pct),
            format!(
                "Chunks: {} loaded | RD {}",
                self.world.chunk_count(),
                self.menu.settings.render_dist
            ),
            format!("Drops: {} | Mobs: {}", self.dropped_items.len(), self.mobs.len()),
        ];

        let right = vec![
            format!("Display: {}x{}", size.width, size.height),
            format!("GPU: {}", self.renderer.gpu_name()),
            format!("API: {}", self.renderer.graphics_api()),
            format!("Seed: {}", self.world_seed),
            format!(
                "Threads: total {} | hmap {} gen {} mesh {} cull {}",
                self.args.threads,
                self.args.hmap_threads(),
                self.args.gen_threads(),
                self.args.mesh_threads(),
                self.args.cull_threads()
            ),
            format!("VSync: {}", if self.menu.settings.vsync { "On" } else { "Off" }),
            format!(
                "Ray tracing: {}",
                if self.renderer.ray_tracing_enabled() { "On" } else { "Off" }
            ),
        ];

        DebugOverlayData { left, right }
    }

    fn grab_cursor(&self, grab: bool) {
        let w = self.renderer.window();
        if grab {
            let _ = w.set_cursor_grab(CursorGrabMode::Confined)
                .or_else(|_| w.set_cursor_grab(CursorGrabMode::Locked));
            w.set_cursor_visible(false);
        } else {
            let _ = w.set_cursor_grab(CursorGrabMode::None);
            w.set_cursor_visible(true);
        }
    }

    fn ensure_heart_hud_textures(&mut self) -> Option<HeartHudTextures> {
        if let Some(textures) = &self.heart_hud_textures {
            return Some(textures.clone());
        }
        if self.heart_hud_load_failed {
            return None;
        }

        let ctx = self.renderer.egui.ctx.clone();
        let full = load_gui_texture(&ctx, "hud_heart_full", "heart.png");
        let half = load_gui_texture(&ctx, "hud_heart_half", "heart_half.png");
        match (full, half) {
            (Some(full), Some(half)) => {
                let textures = HeartHudTextures { full, half };
                self.heart_hud_textures = Some(textures.clone());
                Some(textures)
            }
            _ => {
                self.heart_hud_load_failed = true;
                log::warn!("Heart HUD textures are missing: expected src/assets/gui/heart.png and heart_half.png");
                None
            }
        }
    }

    fn ensure_tool_hud_texture(&mut self, tool: WoodenTool) -> Option<egui::TextureHandle> {
        if let Some(handle) = self.tool_hud_textures.get(&tool) {
            return Some(handle.clone());
        }
        if self.tool_hud_missing.contains(&tool) {
            return None;
        }

        let Some(path) = item_registry::tool_texture_path(tool) else {
            self.tool_hud_missing.insert(tool);
            return None;
        };
        let img = match image::open(&path) {
            Ok(img) => img.to_rgba8(),
            Err(err) => {
                self.tool_hud_missing.insert(tool);
                log::warn!("Failed to load tool HUD texture {:?}: {}", path, err);
                return None;
            }
        };
        let size = [img.width() as usize, img.height() as usize];
        let rgba = img.into_raw();
        let color = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
        let tex_name = format!(
            "hud_tool_{}",
            tool.display_name().replace(' ', "_").to_ascii_lowercase()
        );
        let handle = self
            .renderer
            .egui
            .ctx
            .load_texture(tex_name, color, egui::TextureOptions::NEAREST);
        self.tool_hud_textures.insert(tool, handle.clone());
        Some(handle)
    }
}

#[derive(Clone, Copy)]
struct BlockRayHit {
    block: (i32, i32, i32),
    place: Option<(i32, i32, i32)>,
    normal: (i32, i32, i32),
    distance: f32,
}

struct DebugOverlayData {
    left: Vec<String>,
    right: Vec<String>,
}

enum TargetHudInfo {
    Block {
        name: &'static str,
        item_id: u16,
        break_progress: f32,
    },
    Mob {
        name: &'static str,
        hp: f32,
    },
}

fn draw_health_hud(ctx: &egui::Context, textures: &HeartHudTextures, health: f32) {
    let screen = ctx.screen_rect();
    let slot_size = 28.0;
    let slot_spacing = 4.0;
    let hotbar_width = slot_size * 9.0 + slot_spacing * 8.0 + 20.0;
    let hotbar_height = slot_size + 20.0;
    let hotbar_top = screen.bottom() - hotbar_height - 12.0;

    let heart_size = 16.0;
    let heart_gap = 2.0;
    let hearts_count = (PLAYER_MAX_HEALTH / 2.0).ceil() as usize;
    let start_x = screen.center().x - hotbar_width * 0.5 + 2.0;
    let y = hotbar_top - heart_size - 8.0;
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let hp = health.clamp(0.0, PLAYER_MAX_HEALTH);

    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("health_hud"),
    ));
    for i in 0..hearts_count {
        let x = start_x + i as f32 * (heart_size + heart_gap);
        let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(heart_size, heart_size));

        // Draw a dim "empty heart" base.
        painter.image(
            textures.full.id(),
            rect,
            uv,
            egui::Color32::from_rgba_unmultiplied(50, 50, 50, 200),
        );

        let heart_hp = hp - i as f32 * 2.0;
        if heart_hp >= 2.0 {
            painter.image(textures.full.id(), rect, uv, egui::Color32::WHITE);
        } else if heart_hp >= 1.0 {
            painter.image(textures.half.id(), rect, uv, egui::Color32::WHITE);
        }
    }
}

fn draw_survival_hud(ctx: &egui::Context, hunger: f32, stamina: f32, sprinting: bool) {
    let screen = ctx.screen_rect();
    let slot_size = 28.0;
    let slot_spacing = 4.0;
    let hotbar_width = slot_size * 9.0 + slot_spacing * 8.0 + 20.0;
    let hotbar_height = slot_size + 20.0;
    let hotbar_top = screen.bottom() - hotbar_height - 12.0;

    let panel = egui::Rect::from_min_size(
        egui::pos2(screen.center().x + hotbar_width * 0.5 + 10.0, hotbar_top - 48.0),
        egui::vec2(130.0, 38.0),
    );
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("survival_hud"),
    ));

    painter.rect_filled(
        panel,
        2.0,
        egui::Color32::from_rgba_unmultiplied(15, 15, 15, 190),
    );
    painter.rect_stroke(
        panel,
        2.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(78, 78, 78)),
    );

    let hunger_ratio = (hunger / PLAYER_MAX_HUNGER).clamp(0.0, 1.0);
    let stamina_ratio = (stamina / PLAYER_MAX_STAMINA).clamp(0.0, 1.0);

    let hunger_bar = egui::Rect::from_min_size(panel.min + egui::vec2(8.0, 8.0), egui::vec2(114.0, 8.0));
    let stamina_bar = egui::Rect::from_min_size(panel.min + egui::vec2(8.0, 22.0), egui::vec2(114.0, 8.0));

    painter.text(
        hunger_bar.left_center() + egui::vec2(0.0, -7.0),
        egui::Align2::LEFT_BOTTOM,
        format!("Food {:.0}/{}", hunger.clamp(0.0, PLAYER_MAX_HUNGER), PLAYER_MAX_HUNGER as i32),
        egui::FontId::proportional(10.0),
        egui::Color32::from_rgb(230, 210, 148),
    );

    painter.text(
        stamina_bar.left_center() + egui::vec2(0.0, -7.0),
        egui::Align2::LEFT_BOTTOM,
        if sprinting { "Stamina (sprint)" } else { "Stamina" },
        egui::FontId::proportional(10.0),
        egui::Color32::from_rgb(172, 218, 236),
    );

    painter.rect_filled(
        hunger_bar,
        1.0,
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200),
    );
    if hunger_ratio > 0.0 {
        let fill = egui::Rect::from_min_size(
            hunger_bar.min,
            egui::vec2(hunger_bar.width() * hunger_ratio, hunger_bar.height()),
        );
        painter.rect_filled(fill, 1.0, egui::Color32::from_rgb(210, 144, 58));
    }

    painter.rect_filled(
        stamina_bar,
        1.0,
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200),
    );
    if stamina_ratio > 0.0 {
        let fill = egui::Rect::from_min_size(
            stamina_bar.min,
            egui::vec2(stamina_bar.width() * stamina_ratio, stamina_bar.height()),
        );
        let color = if sprinting {
            egui::Color32::from_rgb(98, 205, 240)
        } else {
            egui::Color32::from_rgb(72, 162, 198)
        };
        painter.rect_filled(fill, 1.0, color);
    }
}

fn draw_equipped_tool_hud(
    ctx: &egui::Context,
    texture: &egui::TextureHandle,
    durability: u16,
    max_durability: u16,
) {
    let screen = ctx.screen_rect();
    let slot_size = 28.0;
    let slot_spacing = 4.0;
    let hotbar_width = slot_size * 9.0 + slot_spacing * 8.0 + 20.0;
    let hotbar_height = slot_size + 20.0;
    let hotbar_top = screen.bottom() - hotbar_height - 12.0;

    let panel = egui::Rect::from_min_size(
        egui::pos2(screen.center().x + hotbar_width * 0.5 + 10.0, hotbar_top + 2.0),
        egui::vec2(38.0, 38.0),
    );
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("equipped_tool_hud"),
    ));

    painter.rect_filled(
        panel,
        2.0,
        egui::Color32::from_rgba_unmultiplied(22, 22, 22, 215),
    );
    painter.rect_stroke(
        panel,
        2.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(96, 96, 96)),
    );

    let icon_rect = panel.shrink(5.0);
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    painter.image(texture.id(), icon_rect, uv, egui::Color32::WHITE);

    let max_durability = max_durability.max(1);
    let ratio = (durability as f32 / max_durability as f32).clamp(0.0, 1.0);
    let bar_rect = egui::Rect::from_min_size(
        egui::pos2(panel.left(), panel.bottom() + 2.0),
        egui::vec2(panel.width(), 4.0),
    );
    painter.rect_filled(
        bar_rect,
        1.0,
        egui::Color32::from_rgba_unmultiplied(15, 15, 15, 190),
    );
    let fill_w = (bar_rect.width() * ratio).max(0.0);
    if fill_w > 0.0 {
        let fill_rect = egui::Rect::from_min_size(bar_rect.min, egui::vec2(fill_w, bar_rect.height()));
        let r = ((1.0 - ratio) * 225.0 + 30.0) as u8;
        let g = (ratio * 215.0 + 40.0) as u8;
        painter.rect_filled(fill_rect, 1.0, egui::Color32::from_rgb(r, g, 36));
    }
}

fn draw_cave_mood_hud(ctx: &egui::Context, mood_percent: f32) {
    let screen = ctx.screen_rect();
    let slot_size = 28.0;
    let slot_spacing = 4.0;
    let hotbar_width = slot_size * 9.0 + slot_spacing * 8.0 + 20.0;
    let hotbar_height = slot_size + 20.0;
    let hotbar_top = screen.bottom() - hotbar_height - 12.0;

    let text = format!("Mood: {:.0}%", mood_percent.clamp(0.0, 100.0));
    let rect = egui::Rect::from_min_size(
        egui::pos2(screen.center().x + hotbar_width * 0.5 - 102.0, hotbar_top - 26.0),
        egui::vec2(96.0, 18.0),
    );
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("cave_mood_hud"),
    ));
    painter.rect_filled(rect, 2.0, egui::Color32::from_rgba_unmultiplied(0, 0, 0, 135));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(13.0),
        egui::Color32::from_rgb(218, 218, 218),
    );
}

fn draw_target_hud(
    ctx: &egui::Context,
    target: &TargetHudInfo,
    heart_textures: Option<&HeartHudTextures>,
) {
    let screen = ctx.screen_rect();
    let panel_w = 324.0;
    let panel_h = 62.0;
    let panel = egui::Rect::from_min_size(
        egui::pos2(screen.center().x - panel_w * 0.5, 8.0),
        egui::vec2(panel_w, panel_h),
    );
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("target_hud"),
    ));

    painter.rect_filled(
        panel,
        4.0,
        egui::Color32::from_rgba_unmultiplied(18, 18, 18, 222),
    );
    painter.rect_stroke(
        panel,
        4.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgb(66, 66, 66)),
    );

    match target {
        TargetHudInfo::Block {
            name,
            item_id,
            break_progress,
        } => {
            painter.text(
                panel.left_top() + egui::vec2(10.0, 10.0),
                egui::Align2::LEFT_TOP,
                format!("{name}  [id:{item_id}]"),
                egui::FontId::proportional(14.0),
                egui::Color32::from_rgb(236, 236, 236),
            );
            painter.text(
                panel.left_top() + egui::vec2(10.0, 28.0),
                egui::Align2::LEFT_TOP,
                "minecraft",
                egui::FontId::proportional(12.0),
                egui::Color32::from_rgb(182, 182, 182),
            );

            let bar = egui::Rect::from_min_size(
                panel.left_bottom() + egui::vec2(10.0, -16.0),
                egui::vec2(panel.width() - 20.0, 8.0),
            );
            let ratio = break_progress.clamp(0.0, 1.0);
            let percent = (ratio * 100.0).round() as i32;
            painter.rect_filled(bar, 2.0, egui::Color32::from_rgba_unmultiplied(8, 8, 8, 200));
            if ratio > 0.0 {
                let fill = egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * ratio, bar.height()));
                painter.rect_filled(fill, 2.0, egui::Color32::from_rgb(236, 232, 188));
            }
            painter.text(
                bar.right_top() + egui::vec2(0.0, -2.0),
                egui::Align2::RIGHT_BOTTOM,
                format!("{percent}%"),
                egui::FontId::proportional(11.0),
                egui::Color32::from_rgb(190, 190, 190),
            );
        }
        TargetHudInfo::Mob { name, hp } => {
            let hp = hp.max(0.0);
            painter.text(
                panel.left_top() + egui::vec2(10.0, 10.0),
                egui::Align2::LEFT_TOP,
                *name,
                egui::FontId::proportional(14.0),
                egui::Color32::from_rgb(244, 232, 232),
            );
            painter.text(
                panel.right_top() + egui::vec2(-10.0, 10.0),
                egui::Align2::RIGHT_TOP,
                format!("HP {:.0}", hp),
                egui::FontId::proportional(12.0),
                egui::Color32::from_rgb(232, 182, 182),
            );
            if let Some(textures) = heart_textures {
                draw_target_mob_hearts(&painter, panel, textures, hp);
            } else {
                painter.text(
                    panel.left_top() + egui::vec2(10.0, 28.0),
                    egui::Align2::LEFT_TOP,
                    format!("HP {:.1} | {}", hp, mob_hearts_text(hp)),
                    egui::FontId::proportional(12.0),
                    egui::Color32::from_rgb(228, 170, 170),
                );
            }
            painter.text(
                panel.left_top() + egui::vec2(10.0, 44.0),
                egui::Align2::LEFT_TOP,
                "minecraft mob",
                egui::FontId::proportional(11.0),
                egui::Color32::from_rgb(176, 176, 176),
            );
        }
    }
}

fn draw_target_mob_hearts(
    painter: &egui::Painter,
    panel: egui::Rect,
    textures: &HeartHudTextures,
    hp: f32,
) {
    let hearts_total = (hp / 2.0).ceil() as usize;
    let heart_size = 11.0;
    let gap = 2.0;
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let y = panel.top() + 28.0;
    let x0 = panel.left() + 10.0;

    if hp > 150.0 {
        let r0 = egui::Rect::from_min_size(egui::pos2(x0, y), egui::vec2(heart_size, heart_size));
        let r1 = egui::Rect::from_min_size(
            egui::pos2(x0 + heart_size + gap, y),
            egui::vec2(heart_size, heart_size),
        );
        painter.image(textures.full.id(), r0, uv, egui::Color32::WHITE);
        painter.image(textures.full.id(), r1, uv, egui::Color32::WHITE);
        let extra = (hp - 150.0).round().max(0.0) as i32;
        painter.text(
            egui::pos2(x0 + (heart_size + gap) * 2.0 + 4.0, y + heart_size * 0.5),
            egui::Align2::LEFT_CENTER,
            format!("+{extra}"),
            egui::FontId::proportional(12.0),
            egui::Color32::from_rgb(228, 170, 170),
        );
        return;
    }

    let shown = hearts_total.min(20);
    for i in 0..shown {
        let x = x0 + i as f32 * (heart_size + gap);
        let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(heart_size, heart_size));
        let heart_hp = hp - i as f32 * 2.0;
        if heart_hp >= 2.0 {
            painter.image(textures.full.id(), rect, uv, egui::Color32::WHITE);
        } else if heart_hp >= 1.0 {
            painter.image(textures.half.id(), rect, uv, egui::Color32::WHITE);
        } else {
            painter.image(
                textures.full.id(),
                rect,
                uv,
                egui::Color32::from_rgba_unmultiplied(48, 48, 48, 180),
            );
        }
    }

    if hearts_total > shown {
        let extra = hearts_total - shown;
        let text_x = x0 + shown as f32 * (heart_size + gap) + 4.0;
        painter.text(
            egui::pos2(text_x, y + heart_size * 0.5),
            egui::Align2::LEFT_CENTER,
            format!("+{extra}"),
            egui::FontId::proportional(12.0),
            egui::Color32::from_rgb(228, 170, 170),
        );
    }
}

fn mob_hearts_text(hp: f32) -> String {
    let hp = hp.max(0.0);
    let full = (hp / 2.0).floor() as usize;
    let has_half = (hp - full as f32 * 2.0) >= 1.0;
    let hearts_total = full + usize::from(has_half);

    if hp > 150.0 {
        let extra = (hp - 150.0).round().max(0.0) as i32;
        return format!("heart heart +{extra}");
    }

    if hearts_total > 20 {
        if has_half {
            return format!("heart x{full} +half-heart");
        }
        return format!("heart x{full}");
    }

    let mut chunks = Vec::with_capacity(hearts_total.max(1));
    for _ in 0..full {
        chunks.push("heart");
    }
    if has_half {
        chunks.push("half-heart");
    }
    if chunks.is_empty() {
        "no hearts".to_string()
    } else {
        chunks.join(" ")
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
        Block::Leaves => "Oak Leaves",
        Block::LogBottom => "Oak Log Top",
        Block::CoalOre => "Coal Ore",
        Block::IronOre => "Iron Ore",
        Block::CopperOre => "Copper Ore",
    }
}

fn draw_f3_overlay(ctx: &egui::Context, data: &DebugOverlayData) {
    use egui::{Align, Color32, FontId, Layout, RichText, Stroke};

    egui::Area::new("f3_left".into())
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(6.0, 6.0))
        .show(ctx, |ui| {
            let frame = egui::Frame::none()
                .fill(Color32::from_rgba_unmultiplied(0, 0, 0, 150))
                .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 35)))
                .inner_margin(egui::Margin::same(6.0));
            frame.show(ui, |ui| {
                for line in &data.left {
                    ui.label(
                        RichText::new(line)
                            .font(FontId::monospace(14.0))
                            .color(Color32::from_rgb(245, 245, 245)),
                    );
                }
            });
        });

    egui::Area::new("f3_right".into())
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-6.0, 6.0))
        .show(ctx, |ui| {
            let frame = egui::Frame::none()
                .fill(Color32::from_rgba_unmultiplied(0, 0, 0, 150))
                .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 35)))
                .inner_margin(egui::Margin::same(6.0));
            frame.show(ui, |ui| {
                ui.with_layout(Layout::top_down(Align::RIGHT), |ui| {
                    for line in &data.right {
                        ui.label(
                            RichText::new(line)
                                .font(FontId::monospace(14.0))
                                .color(Color32::from_rgb(245, 245, 245)),
                        );
                    }
                });
            });
        });
}

fn resolve_gui_asset_path(file_name: &str) -> Option<PathBuf> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        base.join("src").join("assets").join("gui").join(file_name),
        base.join("assets").join("gui").join(file_name),
    ];
    candidates.into_iter().find(|p| p.exists())
}

fn load_gui_texture(ctx: &egui::Context, texture_name: &str, file_name: &str) -> Option<egui::TextureHandle> {
    let path = resolve_gui_asset_path(file_name)?;
    let img = image::open(path).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let rgba = img.into_raw();
    let color = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
    Some(ctx.load_texture(
        texture_name.to_string(),
        color,
        egui::TextureOptions::NEAREST,
    ))
}

fn block_break_seconds(block: Block, tool: Option<WoodenTool>) -> f32 {
    item_registry::block_break_seconds(block, tool)
}

fn fallback_hit_normal(dir: glam::Vec3) -> (i32, i32, i32) {
    let ad = glam::Vec3::new(dir.x.abs(), dir.y.abs(), dir.z.abs());
    if ad.x >= ad.y && ad.x >= ad.z {
        if dir.x >= 0.0 { (-1, 0, 0) } else { (1, 0, 0) }
    } else if ad.y >= ad.x && ad.y >= ad.z {
        if dir.y >= 0.0 { (0, -1, 0) } else { (0, 1, 0) }
    } else if dir.z >= 0.0 {
        (0, 0, -1)
    } else {
        (0, 0, 1)
    }
}

fn apply_drop_magnet(drop: &mut DroppedItem, pickup_origin: glam::Vec3, dt: f32) -> bool {
    let to_player = pickup_origin - drop.pos;
    let dist = to_player.length();
    if dist <= 0.001 || dist > DROP_MAGNET_RADIUS {
        return false;
    }

    let dir = to_player / dist;
    let proximity = 1.0 - (dist / DROP_MAGNET_RADIUS);
    let speed = if dist <= DROP_MAGNET_SNAP_RADIUS {
        let snap_t = (1.0 - dist / DROP_MAGNET_SNAP_RADIUS).clamp(0.0, 1.0);
        DROP_MAGNET_SNAP_SPEED * (0.80 + snap_t * 0.40)
    } else {
        DROP_MAGNET_MAX_SPEED * (0.55 + proximity * 0.95)
    };

    let step = speed * dt;
    if step >= dist {
        drop.pos = pickup_origin;
    } else {
        drop.pos += dir * step;
    }

    drop.vel = dir * speed;
    drop.vel.y += if dist <= DROP_MAGNET_SNAP_RADIUS { 2.4 } else { 0.8 } * dt;
    true
}

fn simulate_drop_physics(drop: &mut DroppedItem, dt: f32, world: &World) {
    drop.vel.y = (drop.vel.y - 26.0 * dt).max(-24.0);

    let on_ground = {
        let below = drop.pos - glam::Vec3::new(0.0, 0.20, 0.0);
        world.is_solid_at_world(
            below.x.floor() as i32,
            below.y.floor() as i32,
            below.z.floor() as i32,
        )
    };
    let drag = if on_ground { 11.0 } else { 1.4 };
    let damp = (1.0 - drag * dt).clamp(0.0, 1.0);
    drop.vel.x *= damp;
    drop.vel.z *= damp;

    let mut moved = drop.pos;
    let next_x = moved + glam::Vec3::new(drop.vel.x * dt, 0.0, 0.0);
    if !drop_intersects_world(next_x, world) {
        moved.x = next_x.x;
    } else {
        drop.vel.x *= -0.15;
    }

    let next_z = moved + glam::Vec3::new(0.0, 0.0, drop.vel.z * dt);
    if !drop_intersects_world(next_z, world) {
        moved.z = next_z.z;
    } else {
        drop.vel.z *= -0.15;
    }

    let start_y = moved.y;
    let target_y = start_y + drop.vel.y * dt;
    let next_y = glam::Vec3::new(moved.x, target_y, moved.z);
    if !drop_intersects_world(next_y, world) {
        moved.y = target_y;
    } else {
        let mut lo = 0.0f32;
        let mut hi = 1.0f32;
        for _ in 0..8 {
            let t = (lo + hi) * 0.5;
            let probe_y = start_y + (target_y - start_y) * t;
            let probe = glam::Vec3::new(moved.x, probe_y, moved.z);
            if drop_intersects_world(probe, world) {
                hi = t;
            } else {
                lo = t;
            }
        }
        moved.y = start_y + (target_y - start_y) * lo;
        if drop.vel.y < -1.0 {
            drop.vel.y = -drop.vel.y * 0.24;
        } else {
            drop.vel.y = 0.0;
        }
    }

    drop.pos = moved;
    if drop.pos.y < -64.0 {
        drop.age = DROP_DESPAWN_SECS + 1.0;
    }
}

fn drop_intersects_world(pos: glam::Vec3, world: &World) -> bool {
    let half_w = 0.16f32;
    let half_h = 0.16f32;
    let min = pos - glam::Vec3::new(half_w, half_h, half_w);
    let max = pos + glam::Vec3::new(half_w, half_h, half_w);

    let x0 = min.x.floor() as i32;
    let y0 = min.y.floor() as i32;
    let z0 = min.z.floor() as i32;
    let x1 = max.x.floor() as i32;
    let y1 = max.y.floor() as i32;
    let z1 = max.z.floor() as i32;

    for x in x0..=x1 {
        for y in y0..=y1 {
            for z in z0..=z1 {
                if world.is_solid_at_world(x, y, z) {
                    return true;
                }
            }
        }
    }
    false
}

fn simulate_mob_physics(mob: &mut MobEntity, dt: f32, world: &World) {
    let mut moved = mob.pos;

    let next_x = moved + glam::Vec3::new(mob.vel.x * dt, 0.0, 0.0);
    if !mob_intersects_world(next_x, world) {
        moved.x = next_x.x;
    } else {
        mob.vel.x *= -0.18;
    }

    let next_z = moved + glam::Vec3::new(0.0, 0.0, mob.vel.z * dt);
    if !mob_intersects_world(next_z, world) {
        moved.z = next_z.z;
    } else {
        mob.vel.z *= -0.18;
    }

    let start_y = moved.y;
    let target_y = start_y + mob.vel.y * dt;
    let next_y = glam::Vec3::new(moved.x, target_y, moved.z);
    if !mob_intersects_world(next_y, world) {
        moved.y = target_y;
    } else {
        let mut lo = 0.0f32;
        let mut hi = 1.0f32;
        for _ in 0..8 {
            let t = (lo + hi) * 0.5;
            let probe_y = start_y + (target_y - start_y) * t;
            let probe = glam::Vec3::new(moved.x, probe_y, moved.z);
            if mob_intersects_world(probe, world) {
                hi = t;
            } else {
                lo = t;
            }
        }
        moved.y = start_y + (target_y - start_y) * lo;
        if mob.vel.y < -1.0 {
            mob.vel.y = -mob.vel.y * 0.14;
        } else {
            mob.vel.y = 0.0;
        }
    }

    mob.pos = moved;
}

fn mob_intersects_world(pos: glam::Vec3, world: &World) -> bool {
    let half_w = 0.34f32;
    let h = 0.90f32;
    let min = pos + glam::Vec3::new(-half_w, 0.0, -half_w);
    let max = pos + glam::Vec3::new(half_w, h, half_w);

    let x0 = min.x.floor() as i32;
    let y0 = min.y.floor() as i32;
    let z0 = min.z.floor() as i32;
    let x1 = max.x.floor() as i32;
    let y1 = max.y.floor() as i32;
    let z1 = max.z.floor() as i32;

    for x in x0..=x1 {
        for y in y0..=y1 {
            for z in z0..=z1 {
                if world.is_solid_at_world(x, y, z) {
                    return true;
                }
            }
        }
    }
    false
}

fn ray_sphere_intersection(
    ro: glam::Vec3,
    rd: glam::Vec3,
    center: glam::Vec3,
    radius: f32,
) -> Option<f32> {
    let oc = ro - center;
    let a = rd.dot(rd);
    if a <= 1e-6 {
        return None;
    }
    let b = 2.0 * oc.dot(rd);
    let c = oc.dot(oc) - radius * radius;
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let s = disc.sqrt();
    let inv = 0.5 / a;
    let t0 = (-b - s) * inv;
    let t1 = (-b + s) * inv;
    if t0 >= 0.0 {
        Some(t0)
    } else if t1 >= 0.0 {
        Some(t1)
    } else {
        None
    }
}

fn line_of_sight_clear(world: &World, from: glam::Vec3, to: glam::Vec3) -> bool {
    let ray = to - from;
    let len = ray.length();
    if len <= 0.001 {
        return true;
    }
    let dir = ray / len;
    let step = 0.28f32;
    let mut t = step;
    while t < len - step {
        let p = from + dir * t;
        if world.is_solid_at_world(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32) {
            return false;
        }
        t += step;
    }
    true
}

fn drop_entropy(x: i32, y: i32, z: i32, block_id: u16, salt: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B9)
        ^ (y as u32).wrapping_mul(0x85EB_CA6B)
        ^ (z as u32).wrapping_mul(0xC2B2_AE35)
        ^ (block_id as u32).wrapping_mul(0x27D4_EB2D)
        ^ salt.wrapping_mul(0x1656_67B1);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^ (h >> 16)
}

fn hash32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^ (x >> 16)
}

fn facing_from_yaw(yaw_deg: f32) -> (&'static str, &'static str) {
    let yaw = yaw_deg.to_radians();
    let x = yaw.cos();
    let z = yaw.sin();

    if x.abs() > z.abs() {
        if x >= 0.0 {
            ("east", "Towards positive X")
        } else {
            ("west", "Towards negative X")
        }
    } else if z >= 0.0 {
        ("south", "Towards positive Z")
    } else {
        ("north", "Towards negative Z")
    }
}

fn biome_name(biome: Biome) -> String {
    format!("{biome:?}")
}

fn ambient_families_for_biome(biome: Biome) -> &'static [&'static str] {
    if biome::is_ocean(biome) || matches!(biome, Biome::River | Biome::FrozenRiver | Biome::Beach) {
        return &["water", "sand", "stone", "cave"];
    }
    if biome::is_mountain(biome) {
        return &["stone", "cave", "gravel", "sand"];
    }
    if matches!(biome, Biome::Desert | Biome::Badlands | Biome::ErodedBadlands | Biome::WoodedBadlands) {
        return &["sand", "stone", "gravel", "cave"];
    }
    if matches!(
        biome,
        Biome::Jungle
            | Biome::SparseJungle
            | Biome::BambooJungle
            | Biome::OldGrowthJungle
            | Biome::Swamp
            | Biome::MangroveSwamp
    ) {
        return &["grass", "wood", "water", "stone"];
    }
    &["grass", "wood", "gravel", "stone"]
}

fn next_seed(seed: u32) -> u32 {
    // Xorshift32 step gives a new deterministic seed each regeneration.
    let mut x = seed ^ 0x9E37_79B9;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    if x == 0 { 1 } else { x }
}

fn wrap_day_time(t: f32) -> f32 {
    t.rem_euclid(1.0)
}

fn digit_key_to_hotbar_slot(key: KeyCode) -> Option<usize> {
    match key {
        KeyCode::Digit1 => Some(0),
        KeyCode::Digit2 => Some(1),
        KeyCode::Digit3 => Some(2),
        KeyCode::Digit4 => Some(3),
        KeyCode::Digit5 => Some(4),
        KeyCode::Digit6 => Some(5),
        KeyCode::Digit7 => Some(6),
        KeyCode::Digit8 => Some(7),
        KeyCode::Digit9 => Some(8),
        _ => None,
    }
}

fn settings_changed(a: &Settings, b: &Settings) -> bool {
    if a.render_dist != b.render_dist
        || a.vsync != b.vsync
        || a.ray_tracing != b.ray_tracing
        || a.show_fps != b.show_fps
    {
        return true;
    }
    (a.mouse_sens - b.mouse_sens).abs() > 0.0001
        || (a.fov - b.fov).abs() > 0.0001
        || (a.master_volume - b.master_volume).abs() > 0.0001
        || (a.music_volume - b.music_volume).abs() > 0.0001
        || (a.ambient_volume - b.ambient_volume).abs() > 0.0001
        || (a.sfx_volume - b.sfx_volume).abs() > 0.0001
        || (a.ambient_boost - b.ambient_boost).abs() > 0.0001
        || (a.sun_softness - b.sun_softness).abs() > 0.0001
        || (a.fog_density - b.fog_density).abs() > 0.0001
        || (a.exposure - b.exposure).abs() > 0.0001
}

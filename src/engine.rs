use std::collections::HashSet;
use std::time::Instant;
use winit::{
    dpi::LogicalSize,
    event::{DeviceEvent, Event, WindowEvent, ElementState},
    event_loop::{EventLoop, ControlFlow},
    keyboard::{KeyCode, PhysicalKey},
    window::{WindowBuilder, CursorGrabMode},
};

use crate::args::Args;
use crate::camera::Camera;
use crate::inventory::Inventory;
use crate::menu::{EscMenu, MenuAction, Settings};
use crate::modding::{self, ApiCommand, ApiSnapshot, ModApiRuntime};
use crate::player::Player;
use crate::renderer::Renderer;
use crate::world::chunk::{spawn_point, CHUNK_D, CHUNK_W};
use crate::world::world::World;
use crate::world::biome::Biome;

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
    jump_was_down: bool,
    debug_overlay: bool,
    mod_api_runtime: ModApiRuntime,
    applied_render_dist: i32,
    applied_vsync: bool,
    applied_lighting: [f32; 4],
}

impl Engine {
    pub async fn new(args: Args) -> (Self, EventLoop<()>) {
        let world_seed: u32 = 42;
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

        let renderer = Renderer::new(window, &args).await;
        let (spawn_x, spawn_y, spawn_z) = spawn_point(world_seed);
        let player   = Player::new(glam::Vec3::new(spawn_x, spawn_y, spawn_z));
        let camera   = Camera::new(player.eye_pos());
        let world    = World::new(world_seed, &args);

        let ambient_boost = 1.02;
        let sun_softness = 0.34;
        let fog_density = 1.0;
        let exposure = 1.00;
        let settings = Settings {
            render_dist: args.render_dist,
            fly_speed:   camera.speed,
            mouse_sens:  camera.sensitivity,
            vsync:       args.vsync,
            show_fps:    true,
            ambient_boost,
            sun_softness,
            fog_density,
            exposure,
        };
        let menu = EscMenu::new(settings);
        let inventory = Inventory::new();

        let engine = Self {
            args: args.clone(),
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
            jump_was_down: false,
            debug_overlay: false,
            mod_api_runtime,
            applied_render_dist: args.render_dist,
            applied_vsync: args.vsync,
            applied_lighting: [
                ambient_boost,
                sun_softness,
                fog_density,
                exposure,
            ],
        };

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
                        if let PhysicalKey::Code(KeyCode::F6) = ke.physical_key {
                            if ke.state == ElementState::Pressed && !ke.repeat {
                                let next = !self.renderer.ray_tracing_enabled();
                                self.renderer.set_ray_tracing_enabled(next);
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
                            WindowEvent::CloseRequested => elwt.exit(),

                            WindowEvent::Focused(f) => {
                                self.focused = *f;
                                if *f && !self.menu.open && !self.inventory.open { self.grab_cursor(true); }
                            }

                            WindowEvent::KeyboardInput { event: ke, .. } => {
                                if let PhysicalKey::Code(key) = ke.physical_key {
                                    match ke.state {
                                        ElementState::Pressed  => { self.keys.insert(key); }
                                        ElementState::Released => { self.keys.remove(&key); }
                                    }

                                    if key == KeyCode::Escape
                                        && ke.state == ElementState::Pressed
                                    {
                                        if self.inventory.open {
                                            self.inventory.open = false;
                                            self.grab_cursor(true);
                                        } else {
                                            self.menu.toggle();
                                            if self.menu.open {
                                                self.inventory.open = false;
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
                                            self.grab_cursor(false);
                                        } else {
                                            self.grab_cursor(true);
                                        }
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
                    let now = Instant::now();
                    let dt  = now.duration_since(last_time).as_secs_f32().min(0.05);
                    last_time = now;

                    // day/night cycle (0..1)
                    let day_len = 300.0;
                    self.day_time += dt / day_len;
                    if self.day_time >= 1.0 { self.day_time -= 1.0; }
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
                    if !self.menu.open && !self.inventory.open {
                        self.update(dt);
                    }

                    // Применяем настройки из меню к камере
                    self.camera.sensitivity = self.menu.settings.mouse_sens;
                    self.camera.speed       = self.menu.settings.fly_speed;
                    self.apply_runtime_graphics_settings();

                    self.renderer.update_visibility(&self.camera);
                    self.renderer.update_camera(&self.camera, self.day_time);
                    self.renderer.update_ray_tracing(&self.camera, &self.world, self.day_time);

                    let show_debug = self.debug_overlay;
                    let debug_data = if show_debug {
                        Some(self.collect_debug_overlay(fps_display, dt))
                    } else {
                        None
                    };
                    let menu = &mut self.menu;
                    let inventory = &mut self.inventory;
                    let show_fps = menu.settings.show_fps;
                    let fps = fps_display;
                    let mut menu_action = MenuAction::None;

                    match self.renderer.render(&self.camera, |ctx| {
                        // FPS оверлей (когда меню закрыто)
                        if show_debug && !menu.open {
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
                            inventory.draw_hotbar(ctx);
                            inventory.draw(ctx);
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

        let mut input = glam::Vec2::ZERO;
        if self.keys.contains(&KeyCode::KeyD) { input.x += 1.0; }
        if self.keys.contains(&KeyCode::KeyA) { input.x -= 1.0; }
        if self.keys.contains(&KeyCode::KeyW) { input.y += 1.0; }
        if self.keys.contains(&KeyCode::KeyS) { input.y -= 1.0; }
        let jump_down = self.keys.contains(&KeyCode::Space);
        let jump_pressed = jump_down && !self.jump_was_down;
        self.jump_was_down = jump_down;

        self.player.simulate(
            dt,
            input,
            jump_pressed,
            self.camera.yaw,
            self.camera.speed,
            &self.world,
        );
        self.camera.pos = self.player.eye_pos();

        if !self.world.removed.is_empty() {
            self.renderer.remove_chunks(&self.world.removed);
            self.world.removed.clear();
        }

        let batch = self.world.drain_ready_meshes(6);
        if !batch.is_empty() {
            self.renderer.update_chunks(batch);
        }

    }

    fn regenerate_world(&mut self, seed_override: Option<u32>) {
        self.world_seed = seed_override.unwrap_or_else(|| next_seed(self.world_seed));
        log::info!("Regenerating world with seed {}", self.world_seed);

        self.renderer.clear_chunks();
        self.world = World::new(self.world_seed, &self.args);

        let (spawn_x, spawn_y, spawn_z) = spawn_point(self.world_seed);
        self.player = Player::new(glam::Vec3::new(spawn_x, spawn_y, spawn_z));
        self.camera.pos = self.player.eye_pos();
        self.day_time = 0.25;
        self.jump_was_down = false;
        self.keys.clear();
        self.debug_overlay = false;

        self.menu.open = false;
        self.inventory.open = false;
        self.grab_cursor(true);
    }

    fn apply_runtime_graphics_settings(&mut self) {
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
                    self.camera.pos = self.player.eye_pos();
                    self.jump_was_down = false;
                }
                ApiCommand::SetDebugOverlay(enabled) => {
                    self.debug_overlay = enabled;
                }
                ApiCommand::SetMenuOpen(open) => {
                    self.menu.open = open;
                    if open {
                        self.inventory.open = false;
                        self.grab_cursor(false);
                    } else if !self.inventory.open {
                        self.grab_cursor(true);
                    }
                }
                ApiCommand::SetInventoryOpen(open) => {
                    self.inventory.open = open;
                    if open {
                        self.menu.open = false;
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
                    self.menu.settings.fly_speed = speed;
                    self.camera.speed = speed;
                }
                ApiCommand::SetRayTracingEnabled(enabled) => {
                    self.renderer.set_ray_tracing_enabled(enabled);
                }
                ApiCommand::SetBlock { x, y, z, block } => {
                    let _ = self.world.set_block_at_world(x, y, z, block);
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

        let left = vec![
            format!("MyEngine {:.0} fps ({:.2} ms)", fps, frame_dt * 1000.0),
            format!("XYZ: {:.3} / {:.3} / {:.3}", feet.x, eye.y, feet.z),
            format!("Block: {bx} {by} {bz}"),
            format!("Chunk: {cx} {cz} in {lx} {lz}"),
            format!("Facing: {facing_name} ({axis_text})"),
            format!("Rotation: {:.1} / {:.1}", self.camera.yaw, self.camera.pitch),
            format!("Biome: {biome}"),
            format!("Surface Y: {surface_y}"),
            format!("Day cycle: {:.0}%", day_pct),
            format!(
                "Chunks: {} loaded | RD {}",
                self.world.chunk_count(),
                self.menu.settings.render_dist
            ),
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
}

struct DebugOverlayData {
    left: Vec<String>,
    right: Vec<String>,
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

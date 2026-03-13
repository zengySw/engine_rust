use std::collections::HashSet;
use std::time::Instant;
use winit::{
    dpi::LogicalSize,
    event::{DeviceEvent, ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent},
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
use crate::save;
use crate::world::block::Block;
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
    break_block_requested: bool,
    place_block_requested: bool,
    last_saved_settings: Settings,
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
            fly_speed:   camera.speed,
            mouse_sens:  camera.sensitivity,
            vsync:       effective_args.vsync,
            show_fps:    true,
            ambient_boost: 1.02,
            sun_softness: 0.34,
            fog_density: 1.0,
            exposure: 1.00,
        };
        let settings = loaded_settings.unwrap_or(default_settings);
        camera.speed = settings.fly_speed;
        camera.sensitivity = settings.mouse_sens;
        let menu = EscMenu::new(settings);
        let inventory = Inventory::new();

        let engine = Self {
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
            jump_was_down: false,
            debug_overlay: false,
            mod_api_runtime,
            applied_render_dist: settings.render_dist,
            applied_vsync: settings.vsync,
            applied_lighting: [
                settings.ambient_boost,
                settings.sun_softness,
                settings.fog_density,
                settings.exposure,
            ],
            break_block_requested: false,
            place_block_requested: false,
            last_saved_settings: settings,
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

                                    if ke.state == ElementState::Pressed && !ke.repeat {
                                        if let Some(slot) = digit_key_to_hotbar_slot(key) {
                                            self.inventory.select_hotbar_slot(slot);
                                        }
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

                            WindowEvent::MouseInput { state, button, .. } => {
                                if *state == ElementState::Pressed
                                    && !self.menu.open
                                    && !self.inventory.open
                                {
                                    match button {
                                        MouseButton::Left => self.break_block_requested = true,
                                        MouseButton::Right => self.place_block_requested = true,
                                        _ => {}
                                    }
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
                    self.save_settings_if_changed();

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
        self.handle_block_interaction();

        if !self.world.removed.is_empty() {
            self.renderer.remove_chunks(&self.world.removed);
            self.world.removed.clear();
        }

        let batch = self.world.drain_ready_meshes(6);
        if !batch.is_empty() {
            self.renderer.update_chunks(batch);
        }
        self.world.save_if_dirty();

    }

    fn regenerate_world(&mut self, seed_override: Option<u32>) {
        self.world.save_all();
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

    fn save_settings_if_changed(&mut self) {
        if !settings_changed(&self.last_saved_settings, &self.menu.settings) {
            return;
        }
        save::save_settings(&self.menu.settings);
        self.last_saved_settings = self.menu.settings;
    }

    fn handle_block_interaction(&mut self) {
        let break_req = std::mem::take(&mut self.break_block_requested);
        let place_req = std::mem::take(&mut self.place_block_requested);
        if !break_req && !place_req {
            return;
        }

        let Some(hit) = self.raycast_block_target(6.0) else {
            return;
        };

        let mut changed = false;

        if break_req {
            changed |= self
                .world
                .set_block_at_world(hit.block.0, hit.block.1, hit.block.2, Block::Air);
        }

        if place_req {
            if let Some((px, py, pz)) = hit.place {
                if let Some(block) = self.inventory.selected_block() {
                    if block != Block::Air
                        && self.world.block_at_world(px, py, pz) == Block::Air
                        && !self.player.overlaps_block(px, py, pz)
                    {
                        changed |= self.world.set_block_at_world(px, py, pz, block);
                    }
                }
            }
        }

        if changed {
            self.world.save_if_dirty();
        }
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
                return Some(BlockRayHit {
                    block: cell,
                    place: last_air,
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

#[derive(Clone, Copy)]
struct BlockRayHit {
    block: (i32, i32, i32),
    place: Option<(i32, i32, i32)>,
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
        || a.show_fps != b.show_fps
    {
        return true;
    }
    (a.fly_speed - b.fly_speed).abs() > 0.0001
        || (a.mouse_sens - b.mouse_sens).abs() > 0.0001
        || (a.ambient_boost - b.ambient_boost).abs() > 0.0001
        || (a.sun_softness - b.sun_softness).abs() > 0.0001
        || (a.fog_density - b.fog_density).abs() > 0.0001
        || (a.exposure - b.exposure).abs() > 0.0001
}

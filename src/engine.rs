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
use crate::player::Player;
use crate::renderer::Renderer;
use crate::world::chunk::spawn_plane_floor_y;
use crate::world::world::World;

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
}

impl Engine {
    pub async fn new(args: Args) -> (Self, EventLoop<()>) {
        let world_seed: u32 = 42;

        let event_loop = EventLoop::new().expect("event loop");
        event_loop.set_control_flow(ControlFlow::Poll);

        let window = WindowBuilder::new()
            .with_title("MyEngine")
            .with_inner_size(LogicalSize::new(1280u32, 720u32))
            .with_resizable(true)
            .build(&event_loop)
            .expect("window");

        let renderer = Renderer::new(window, &args).await;
        let spawn_y  = spawn_plane_floor_y(world_seed);
        let player   = Player::new(glam::Vec3::new(0.0, spawn_y, 0.0));
        let camera   = Camera::new(player.eye_pos());
        let world    = World::new(world_seed, &args);

        let settings = Settings {
            render_dist: args.render_dist,
            fly_speed:   camera.speed,
            mouse_sens:  camera.sensitivity,
            vsync:       args.vsync,
            show_fps:    true,
        };
        let menu = EscMenu::new(settings);
        let inventory = Inventory::new();

        (Self {
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
        },
         event_loop)
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

                    self.renderer.update_visibility(&self.camera);
                    self.renderer.update_camera(&self.camera, self.day_time);

                    let menu = &mut self.menu;
                    let inventory = &mut self.inventory;
                    let show_fps = menu.settings.show_fps;
                    let fps = fps_display;
                    let mut menu_action = MenuAction::None;

                    match self.renderer.render(&self.camera, |ctx| {
                        // FPS оверлей (когда меню закрыто)
                        if show_fps && !menu.open {
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
                            self.regenerate_world();
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

        if !self.world.ready_meshes.is_empty() {
            let n = self.world.ready_meshes.len().min(6);
            let batch: Vec<_> = self.world.ready_meshes.drain(..n).collect();
            self.renderer.update_chunks(batch);
        }

    }

    fn regenerate_world(&mut self) {
        self.world_seed = next_seed(self.world_seed);
        log::info!("Regenerating world with seed {}", self.world_seed);

        self.renderer.clear_chunks();
        self.world = World::new(self.world_seed, &self.args);

        let spawn_y = spawn_plane_floor_y(self.world_seed);
        self.player = Player::new(glam::Vec3::new(0.0, spawn_y, 0.0));
        self.camera.pos = self.player.eye_pos();
        self.day_time = 0.25;
        self.jump_was_down = false;
        self.keys.clear();

        self.menu.open = false;
        self.inventory.open = false;
        self.grab_cursor(true);
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

fn next_seed(seed: u32) -> u32 {
    // Xorshift32 step gives a new deterministic seed each regeneration.
    let mut x = seed ^ 0x9E37_79B9;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    if x == 0 { 1 } else { x }
}

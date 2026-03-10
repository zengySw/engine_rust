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
use crate::camera::{Camera, MoveDir};
use crate::renderer::Renderer;
use crate::world::world::World;

pub struct Engine {
    renderer: Renderer,
    camera:   Camera,
    world:    World,
    keys:     HashSet<KeyCode>,
    focused:  bool,
}

impl Engine {
    pub async fn new(args: Args) -> (Self, EventLoop<()>) {
        let event_loop = EventLoop::new().expect("event loop");
        event_loop.set_control_flow(ControlFlow::Poll);

        let window = WindowBuilder::new()
            .with_title(title)
            .with_inner_size(LogicalSize::new(width, height))
            .with_resizable(true)
            .build(&event_loop)
            .expect("window");

        let renderer = Renderer::new(window).await;
        let camera   = Camera::new(glam::Vec3::new(0.0, 90.0, 0.0));

        let world = World::new(42);

        (Self { renderer, camera, world, keys: HashSet::new(), focused: false }, event_loop)
    }

    pub fn run(mut self, event_loop: EventLoop<()>) {
        let mut last_time   = Instant::now();
        let mut frame_count = 0u64;
        let mut fps_timer   = Instant::now();

        event_loop.run(move |event, elwt| {
            match event {
                Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => elwt.exit(),

                Event::WindowEvent { event: WindowEvent::Focused(f), .. } => {
                    self.focused = f;
                    if f { self.grab_cursor(true); }
                }

                Event::WindowEvent {
                    event: WindowEvent::KeyboardInput { event: ke, .. }, ..
                } => {
                    if let PhysicalKey::Code(key) = ke.physical_key {
                        match ke.state {
                            ElementState::Pressed  => { self.keys.insert(key); }
                            ElementState::Released => { self.keys.remove(&key); }
                        }
                        if key == KeyCode::Escape && ke.state == ElementState::Pressed {
                            self.grab_cursor(false);
                        }
                        if key == KeyCode::Tab && ke.state == ElementState::Pressed {
                            self.grab_cursor(true);
                        }
                    }
                }

                Event::DeviceEvent {
                    event: DeviceEvent::MouseMotion { delta: (dx, dy) }, ..
                } => {
                    if self.focused {
                        self.camera.rotate(dx as f32, dy as f32);
                    }
                }

                Event::WindowEvent { event: WindowEvent::Resized(s), .. } => {
                    self.renderer.resize(s.width, s.height);
                }

                Event::AboutToWait => {
                    let now = Instant::now();
                    let dt  = now.duration_since(last_time).as_secs_f32().min(0.05);
                    last_time = now;

                    frame_count += 1;
                    if fps_timer.elapsed().as_secs_f32() >= 1.0 {
                        let fps = frame_count as f32 / fps_timer.elapsed().as_secs_f32();
                        self.renderer.window().set_title(&format!(
                            "MyEngine  |  {:.0} fps  |  {:.2} ms  |  xyz: {:.0} {:.0} {:.0}  |  chunks: {}",
                            fps, dt * 1000.0,
                            self.camera.pos.x, self.camera.pos.y, self.camera.pos.z,
                            self.world.chunk_count(),
                        ));
                        fps_timer   = Instant::now();
                        frame_count = 0;
                    }

                    self.update(dt);
                    self.renderer.update_visibility(&self.camera);

                    match self.renderer.render(&self.camera) {
                        Ok(_) => {}
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated)
                            => self.renderer.reconfigure(),
                        Err(e) => log::error!("{e}"),
                    }                }

                _ => {}
            }
        }).expect("event loop error");
    }

    fn update(&mut self, dt: f32) {
        if self.keys.contains(&KeyCode::KeyW) { self.camera.move_dir(MoveDir::Forward,  dt); }
        if self.keys.contains(&KeyCode::KeyS) { self.camera.move_dir(MoveDir::Backward, dt); }
        if self.keys.contains(&KeyCode::KeyA) { self.camera.move_dir(MoveDir::Left,     dt); }
        if self.keys.contains(&KeyCode::KeyD) { self.camera.move_dir(MoveDir::Right,    dt); }
        if self.keys.contains(&KeyCode::Space)     { self.camera.move_dir(MoveDir::Up,   dt); }
        if self.keys.contains(&KeyCode::ShiftLeft) { self.camera.move_dir(MoveDir::Down, dt); }

        self.world.update(self.camera.pos.x, self.camera.pos.z);

        // Удаляем выгруженные чанки из renderer
        if !self.world.removed.is_empty() {
            self.renderer.remove_chunks(&self.world.removed);
            self.world.removed.clear();
        }

        // Загружаем готовые меши на GPU (по одному за кадр — без фризов)
        if !self.world.ready_meshes.is_empty() {
            let batch: Vec<_> = self.world.ready_meshes
                .drain(..self.world.ready_meshes.len().min(4))
                .collect();
            self.renderer.update_chunks(batch);
        }

        self.renderer.update_camera(&self.camera);
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
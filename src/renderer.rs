use std::collections::HashMap;
use std::sync::Arc;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::args::Args;
use crate::camera::Camera;
use crate::culling::{Frustum, cull_chunks_parallel};
use crate::world::chunk::Vertex;
use crate::world::world::ChunkMesh;

pub struct EguiRenderer {
    pub ctx:      egui::Context,
    state:        egui_winit::State,
    renderer:     egui_wgpu::Renderer,
}

impl EguiRenderer {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        window: &Window,
    ) -> Self {
        let ctx   = egui::Context::default();

        // Тёмная тема
        ctx.set_visuals(egui::Visuals {
            window_corner_radius: egui::CornerRadius::same(12),
            ..egui::Visuals::dark()
        });

        let state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            window,
            None,
            None,
            None,
        );
        let renderer = egui_wgpu::Renderer::new(device, format, None, 1, false);
        Self { ctx, state, renderer }
    }

    pub fn handle_event(
        &mut self,
        window: &Window,
        event: &winit::event::WindowEvent,
    ) -> egui_winit::EventResponse {
        self.state.on_window_event(window, event)
    }

    pub fn draw(
        &mut self,
        device:   &wgpu::Device,
        queue:    &wgpu::Queue,
        encoder:  &mut wgpu::CommandEncoder,
        view:     &wgpu::TextureView,
        window:   &Window,
        run_ui:   impl FnOnce(&egui::Context),
    ) {
        let raw = self.state.take_egui_input(window);
        let output = self.ctx.run(raw, run_ui);

        self.state.handle_platform_output(window, output.platform_output);

        let tris = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        for (id, delta) in &output.textures_delta.set {
            self.renderer.update_texture(device, queue, *id, delta);
        }

        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [window.inner_size().width, window.inner_size().height],
            pixels_per_point: output.pixels_per_point,
        };
        self.renderer.update_buffers(device, queue, encoder, &tris, &screen);

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load:  wgpu::LoadOp::Load, // поверх игры
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        self.renderer.render(&mut pass, &tris, &screen);
        drop(pass);

        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
}

struct ChunkBuffer {
    buf:   wgpu::Buffer,
    count: u32,
}

pub struct Renderer {
    window:         Arc<Window>,
    surface:        wgpu::Surface<'static>,
    device:         wgpu::Device,
    queue:          wgpu::Queue,
    config:         wgpu::SurfaceConfiguration,
    pipeline:       wgpu::RenderPipeline,
    depth_texture:  wgpu::TextureView,
    cam_buffer:     wgpu::Buffer,
    cam_bind_group: wgpu::BindGroup,

    // per-chunk буферы
    chunk_buffers:  HashMap<(i32,i32), ChunkBuffer>,
    cull_pool:      rayon::ThreadPool,
    visible_keys:   Vec<(i32,i32)>,
    pub egui:       EguiRenderer,
}

impl Renderer {
    pub async fn new(window: Window, args: &Args) -> Self {
        let window = Arc::new(window);
        let size   = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(), ..Default::default()
        });
        let surface = instance.create_surface(Arc::clone(&window)).expect("surface");
        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference:   wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }).await.expect("adapter");

        log::info!("GPU: {}", adapter.get_info().name);

        let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("device"),
            required_features: wgpu::Features::empty(),
            required_limits:   wgpu::Limits::default(),
        }, None).await.expect("device");

        let caps   = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().find(|f| f.is_srgb()).copied().unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage:        wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width:        size.width.max(1),
            height:       size.height.max(1),
            present_mode: wgpu::PresentMode::AutoNoVsync, // убираем VSync — реальный FPS
            alpha_mode:   caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let depth_texture = make_depth_texture(&device, config.width, config.height);

        let cam_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cam_uniform"),
            size:  std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let cam_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cam_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0, visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false, min_binding_size: None,
                },
                count: None,
            }],
        });

        let cam_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cam_bg"), layout: &cam_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0, resource: cam_buffer.as_entire_binding(),
            }],
        });

        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("layout"), bind_group_layouts: &[&cam_bgl], push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label:  Some("pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader, entry_point: "vs_main", buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader, entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format, blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology:   wgpu::PrimitiveTopology::TriangleList,
                cull_mode:  Some(wgpu::Face::Back),
                front_face: wgpu::FrontFace::Ccw,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format:              wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare:       wgpu::CompareFunction::Less,
                stencil:             wgpu::StencilState::default(),
                bias:                wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview:   None,
        });

        let egui = EguiRenderer::new(&device, format, &window);

        let cull_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(args.cull_threads())
            .thread_name(|i| format!("cull-{i}"))
            .build()
            .unwrap();

        Self {
            window, surface, device, queue, config,
            pipeline, depth_texture,
            cam_buffer, cam_bind_group,
            chunk_buffers: HashMap::new(),
            cull_pool,
            visible_keys: Vec::new(),
            egui,
        }
    }

    // ── Обновить конкретные чанки ─────────────────────────────
    pub fn update_chunks(&mut self, meshes: Vec<((i32,i32), ChunkMesh)>) {
        for (key, mesh) in meshes {
            if mesh.verts.is_empty() {
                self.chunk_buffers.remove(&key);
                continue;
            }
            let buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label:    Some("chunk_vb"),
                contents: bytemuck::cast_slice(&mesh.verts),
                usage:    wgpu::BufferUsages::VERTEX,
            });
            self.chunk_buffers.insert(key, ChunkBuffer {
                buf,
                count: mesh.verts.len() as u32,
            });
        }
    }

    // ── Удалить буферы выгруженных чанков ─────────────────────
    pub fn remove_chunks(&mut self, keys: &[(i32,i32)]) {
        for key in keys { self.chunk_buffers.remove(key); }
    }

    pub fn update_camera(&self, cam: &Camera) {
        let u = CameraUniform {
            view_proj: cam.view_proj(self.config.width, self.config.height).to_cols_array_2d(),
        };
        self.queue.write_buffer(&self.cam_buffer, 0, bytemuck::bytes_of(&u));
    }

    /// Обновить список видимых чанков (вызывать раз в кадр до render)
    pub fn update_visibility(&mut self, cam: &Camera) {
        let vp = cam.view_proj(self.config.width, self.config.height);
        let frustum = Frustum::from_view_proj(&vp);
        let keys: Vec<_> = self.chunk_buffers.keys().copied().collect();
        self.visible_keys = cull_chunks_parallel(
            &keys, &frustum, cam.pos, &self.cull_pool
        );
    }

    pub fn render(&mut self, _cam: &Camera, run_ui: impl FnOnce(&egui::Context)) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view   = output.texture.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("frame") }
        );

        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view, resolve_target: None,
                    ops: wgpu::Operations {
                        load:  wgpu::LoadOp::Clear(wgpu::Color { r:0.53, g:0.81, b:0.98, a:1. }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_texture,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.cam_bind_group, &[]);

            // Используем предпосчитанный список видимых чанков
            for &key in &self.visible_keys {
                if let Some(cb) = self.chunk_buffers.get(&key) {
                    pass.set_vertex_buffer(0, cb.buf.slice(..));
                    pass.draw(0..cb.count, 0..1);
                }
            }
        }

        // Рисуем egui поверх игры
        let window = Arc::clone(&self.window);
        self.egui.draw(
            &self.device, &self.queue, &mut enc, &view, &window, run_ui,
        );

        self.queue.submit(std::iter::once(enc.finish()));
        output.present();
        Ok(())
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 { return; }
        self.config.width  = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        self.depth_texture = make_depth_texture(&self.device, w, h);
    }

    pub fn reconfigure(&mut self) {
        self.surface.configure(&self.device, &self.config);
    }

    pub fn window(&self) -> &Window { &self.window }
    #[allow(dead_code)]
    pub fn chunk_count(&self) -> usize { self.chunk_buffers.len() }
}

fn make_depth_texture(device: &wgpu::Device, w: u32, h: u32) -> wgpu::TextureView {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size:  wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1,
        dimension:   wgpu::TextureDimension::D2,
        format:      wgpu::TextureFormat::Depth32Float,
        usage:       wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    }).create_view(&Default::default())
}
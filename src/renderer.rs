use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use bytemuck::{Pod, Zeroable};
use image::{imageops::FilterType, Rgba, RgbaImage};
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
            window_rounding: egui::Rounding::same(12.0),
            ..egui::Visuals::dark()
        });

        let state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            window,
            None,
            None,
        );
        let renderer = egui_wgpu::Renderer::new(device, format, None, 1);
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
    time_of_day: [f32; 4],
}

struct ChunkBuffer {
    buf:   wgpu::Buffer,
    count: u32,
}

struct BlockTextures {
    bind_group: wgpu::BindGroup,
    _texture:   wgpu::Texture,
    _view:      wgpu::TextureView,
    _sampler:   wgpu::Sampler,
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
    block_textures: BlockTextures,
    day_time:       f32,

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
                binding: 0, visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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

        let tex_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("block_tex_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let block_textures = create_block_textures(&device, &queue, &tex_bgl);

        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("layout"),
            bind_group_layouts: &[&cam_bgl, &tex_bgl],
            push_constant_ranges: &[],
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
            cam_buffer, cam_bind_group, block_textures,
            day_time: 0.25,
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

    pub fn clear_chunks(&mut self) {
        self.chunk_buffers.clear();
        self.visible_keys.clear();
    }

    pub fn update_camera(&mut self, cam: &Camera, day_time: f32) {
        self.day_time = day_time;
        let u = CameraUniform {
            view_proj: cam.view_proj(self.config.width, self.config.height).to_cols_array_2d(),
            time_of_day: [day_time, 0.0, 0.0, 0.0],
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
                        load:  wgpu::LoadOp::Clear(sky_color_from_time(self.day_time)),
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
            pass.set_bind_group(1, &self.block_textures.bind_group, &[]);

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

    pub fn window_arc(&self) -> Arc<Window> {
        Arc::clone(&self.window)
    }

    pub fn window(&self) -> &Window { &self.window }
    #[allow(dead_code)]
    pub fn chunk_count(&self) -> usize { self.chunk_buffers.len() }
}

fn sky_color_from_time(t: f32) -> wgpu::Color {
    let angle = t * std::f32::consts::TAU;
    let sun_y = angle.sin();
    let day = smoothstep(-0.05, 0.15, sun_y);
    let night = 1.0 - day;

    let day_sky = [0.53, 0.81, 0.98];
    let night_sky = [0.02, 0.03, 0.08];
    let dusk_sky = [0.95, 0.55, 0.25];
    let dusk = smoothstep(0.25, 0.65, 1.0 - sun_y.abs()) * night;

    let r = (day_sky[0] * day + night_sky[0] * night) * (1.0 - dusk) + dusk_sky[0] * dusk;
    let g = (day_sky[1] * day + night_sky[1] * night) * (1.0 - dusk) + dusk_sky[1] * dusk;
    let b = (day_sky[2] * day + night_sky[2] * night) * (1.0 - dusk) + dusk_sky[2] * dusk;

    wgpu::Color { r: r as f64, g: g as f64, b: b as f64, a: 1.0 }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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

fn create_block_textures(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
) -> BlockTextures {
    const TEX_NAMES: [&str; 10] = [
        "air",
        "grass",
        "dirt",
        "stone",
        "sand",
        "water",
        "bedrock",
        "log",
        "logBottom",
        "leaves",
    ];

    let mut layers: Vec<RgbaImage> = Vec::with_capacity(TEX_NAMES.len());
    let mut base_size = None::<(u32, u32)>;

    for name in TEX_NAMES {
        let img = if name == "air" {
            RgbaImage::from_pixel(16, 16, Rgba([0, 0, 0, 0]))
        } else {
            load_block_texture(name)
        };

        let prepared = if let Some((w, h)) = base_size {
            if img.width() == w && img.height() == h {
                img
            } else {
                log::warn!(
                    "Resizing block texture '{}' from {}x{} to {}x{}",
                    name,
                    img.width(),
                    img.height(),
                    w,
                    h
                );
                image::imageops::resize(&img, w, h, FilterType::Nearest)
            }
        } else {
            base_size = Some((img.width(), img.height()));
            img
        };

        layers.push(prepared);
    }

    let (width, height) = base_size.unwrap_or((16, 16));
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("block_texture_array"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: layers.len() as u32,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    for (layer, img) in layers.iter().enumerate() {
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: layer as u32,
                },
                aspect: wgpu::TextureAspect::All,
            },
            img.as_raw(),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("block_texture_array_view"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("block_sampler"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        ..Default::default()
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("block_tex_bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });

    BlockTextures {
        bind_group,
        _texture: texture,
        _view: view,
        _sampler: sampler,
    }
}

fn load_block_texture(name: &str) -> RgbaImage {
    let assets_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("assets");
    let candidates = [
        assets_root.join("blocks").join(format!("{name}.png")),
        assets_root.join("blocks").join(format!("{name}.jpg")),
        assets_root.join("blocks").join(format!("{name}.jpeg")),
        assets_root
            .join("minecraft")
            .join("textures")
            .join("block")
            .join(format!("{name}.png")),
        assets_root
            .join("minecraft")
            .join("textures")
            .join("block")
            .join(format!("{name}.jpg")),
        assets_root
            .join("minecraft")
            .join("textures")
            .join("block")
            .join(format!("{name}.jpeg")),
    ];

    for path in candidates {
        if !path.exists() {
            continue;
        }
        match image::open(&path) {
            Ok(img) => return img.to_rgba8(),
            Err(err) => {
                log::warn!("Failed to decode {:?}: {}", path, err);
            }
        }
    }

    log::warn!("Missing block texture '{}', using fallback color", name);
    fallback_block_texture(name)
}

fn fallback_block_texture(name: &str) -> RgbaImage {
    let color = match name {
        "air" => [0, 0, 0, 0],
        "grass" => [72, 148, 46, 255],
        "dirt" => [122, 84, 46, 255],
        "stone" => [120, 120, 120, 255],
        "sand" => [217, 204, 128, 255],
        "water" => [46, 107, 199, 255],
        "bedrock" => [38, 32, 32, 255],
        "log" => [115, 77, 46, 255],
        "logBottom" => [145, 112, 72, 255],
        "leaves" => [46, 140, 56, 255],
        _ => [255, 0, 255, 255],
    };
    RgbaImage::from_pixel(16, 16, Rgba(color))
}

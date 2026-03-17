use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use bytemuck::{Pod, Zeroable};
use image::{imageops::FilterType, GrayImage, Luma, Rgba, RgbaImage};
use wgpu::util::DeviceExt;
use winit::window::Window;
use zip::ZipArchive;

use crate::args::Args;
use crate::camera::Camera;
use crate::culling::{Frustum, cull_chunks_parallel};
use crate::inventory::WoodenTool;
use crate::raytracing::RayTracingRenderer;
use crate::world::block::Block;
use crate::world::chunk::Vertex;
use crate::world::world::{ChunkMesh, World};

const CRACK_STAGE_COUNT: usize = 10;
const CRACK_PATTERN_SIZE: usize = 16;

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

        // Ð¢Ñ‘Ð¼Ð½Ð°Ñ Ñ‚ÐµÐ¼Ð°
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
                    load:  wgpu::LoadOp::Load, // Ð¿Ð¾Ð²ÐµÑ€Ñ… Ð¸Ð³Ñ€Ñ‹
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
    cam_pos: [f32; 4],   // xyz = camera world pos
    lighting: [f32; 4],  // x=ambient_boost, y=sun_softness, z=fog_density, w=exposure
    rt_params: [f32; 4], // x=enabled, y=max_steps, z=max_dist, w=step_len
    parallax: [f32; 4],  // x=strength
}

struct ChunkBuffer {
    vbuf: wgpu::Buffer,
    vcount: u32,
    ibuf: Option<wgpu::Buffer>,
    icount: u32,
}

#[derive(Clone, Copy)]
pub struct DroppedBlockVisual {
    pub pos: glam::Vec3,
    pub yaw_rad: f32,
    pub scale: f32,
    pub block: Block,
}

#[derive(Clone, Copy)]
pub struct RainDropVisual {
    pub pos: glam::Vec3,
    pub length: f32,
    pub thickness: f32,
    pub yaw_rad: f32,
}

#[derive(Clone, Copy)]
pub struct FirstPersonHandVisual {
    pub phase_rad: f32,
    pub swing: f32,
    pub action_phase_rad: f32,
    pub action_strength: f32,
    pub held_tool: Option<WoodenTool>,
    pub held_block: Option<Block>,
}

#[derive(Clone, Copy)]
pub struct PlayerVisual {
    pub feet_pos: glam::Vec3,
    pub yaw_rad: f32,
    pub phase_rad: f32,
    pub swing: f32,
    pub action_phase_rad: f32,
    pub action_strength: f32,
    pub held_tool: Option<WoodenTool>,
    pub held_block: Option<Block>,
}

#[derive(Clone, Copy)]
pub struct BreakOverlayVisual {
    pub block: (i32, i32, i32),
    pub face_normal: (i32, i32, i32),
    pub progress: f32,
}

#[derive(Clone, Copy)]
pub struct BlockOutlineVisual {
    pub block: (i32, i32, i32),
}

struct BlockTextures {
    bind_group: wgpu::BindGroup,
    _texture:   wgpu::Texture,
    view:       wgpu::TextureView,
    _parallax_texture: wgpu::Texture,
    _parallax_view: wgpu::TextureView,
    sampler:    wgpu::Sampler,
    crack_first_layer: u32,
    hand_layer: u32,
    outline_layer: u32,
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
    lighting:       [f32; 4],
    rt_params:      [f32; 4],
    ray_tracing_enabled: bool,
    ray_tracer:     Option<RayTracingRenderer>,
    ray_tracing_supported: bool,

    // per-chunk Ð±ÑƒÑ„ÐµÑ€Ñ‹
    chunk_buffers:  HashMap<(i32,i32), ChunkBuffer>,
    drop_buffer:    Option<ChunkBuffer>,
    rain_buffer:    Option<ChunkBuffer>,
    hand_buffer:    Option<ChunkBuffer>,
    player_buffer:  Option<ChunkBuffer>,
    break_overlay_buffer: Option<ChunkBuffer>,
    block_outline_buffer: Option<ChunkBuffer>,
    chunk_keys:     Vec<(i32, i32)>,
    cull_pool:      rayon::ThreadPool,
    visible_keys:   Vec<(i32,i32)>,
    pub egui:       EguiRenderer,
    gpu_name:       String,
    graphics_api:   String,
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

        let adapter_info = adapter.get_info();
        log::info!("GPU: {}", adapter_info.name);

        let ray_tracing_supported = RayTracingRenderer::is_supported(&adapter);
        if ray_tracing_supported {
            log::info!("Ray tracing: supported (optional)");
        } else {
            log::warn!(
                "Ray tracing: unavailable on this GPU/backend (game will run without RT)"
            );
        }

        let required_features = if ray_tracing_supported {
            RayTracingRenderer::required_features()
        } else {
            wgpu::Features::empty()
        };

        let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("device"),
            required_features,
            required_limits:   wgpu::Limits::default(),
        }, None).await.expect("device");

        let caps   = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().find(|f| f.is_srgb()).copied().unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage:        wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width:        size.width.max(1),
            height:       size.height.max(1),
            present_mode: wgpu::PresentMode::AutoNoVsync, // ÑƒÐ±Ð¸Ñ€Ð°ÐµÐ¼ VSync â€” Ñ€ÐµÐ°Ð»ÑŒÐ½Ñ‹Ð¹ FPS
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
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
        let ray_tracer = None;

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
            lighting: [1.06, 0.26, 0.90, 1.03],
            rt_params: [0.0, 96.0, 96.0, 0.9],
            ray_tracing_enabled: false,
            ray_tracer,
            ray_tracing_supported,
            chunk_buffers: HashMap::new(),
            drop_buffer: None,
            rain_buffer: None,
            hand_buffer: None,
            player_buffer: None,
            break_overlay_buffer: None,
            block_outline_buffer: None,
            chunk_keys: Vec::new(),
            cull_pool,
            visible_keys: Vec::new(),
            egui,
            gpu_name: adapter_info.name,
            graphics_api: format!("{:?}", adapter_info.backend),
        }
    }

    // â”€â”€ ÐžÐ±Ð½Ð¾Ð²Ð¸Ñ‚ÑŒ ÐºÐ¾Ð½ÐºÑ€ÐµÑ‚Ð½Ñ‹Ðµ Ñ‡Ð°Ð½ÐºÐ¸ â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    pub fn update_chunks(&mut self, meshes: Vec<((i32,i32), ChunkMesh)>) {
        let mut dirty_keys = false;
        for (key, mesh) in meshes {
            if mesh.verts.is_empty() {
                dirty_keys |= self.chunk_buffers.remove(&key).is_some();
                continue;
            }
            let (verts, indices) = compact_triangles_to_indexed_quads(&mesh.verts);
            let vbuf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("chunk_vb"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let ibuf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("chunk_ib"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            self.chunk_buffers.insert(key, ChunkBuffer {
                vbuf,
                vcount: verts.len() as u32,
                ibuf: Some(ibuf),
                icount: indices.len() as u32,
            });
            dirty_keys = true;
        }
        if dirty_keys {
            self.chunk_keys = self.chunk_buffers.keys().copied().collect();
        }
    }

    pub fn update_dropped_blocks(&mut self, drops: &[DroppedBlockVisual]) {
        if drops.is_empty() {
            self.drop_buffer = None;
            return;
        }

        let verts = build_drop_vertices(drops);
        if verts.is_empty() {
            self.drop_buffer = None;
            return;
        }

        let buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("dropped_blocks_vb"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.drop_buffer = Some(ChunkBuffer {
            vbuf: buf,
            vcount: verts.len() as u32,
            ibuf: None,
            icount: 0,
        });
    }

    pub fn update_rain_drops(&mut self, drops: &[RainDropVisual]) {
        if drops.is_empty() {
            self.rain_buffer = None;
            return;
        }

        let verts = build_rain_vertices(drops);
        if verts.is_empty() {
            self.rain_buffer = None;
            return;
        }

        let buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("rain_drops_vb"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.rain_buffer = Some(ChunkBuffer {
            vbuf: buf,
            vcount: verts.len() as u32,
            ibuf: None,
            icount: 0,
        });
    }

    pub fn update_first_person_hand(&mut self, cam: &Camera, hand: Option<FirstPersonHandVisual>) {
        let Some(hand) = hand else {
            self.hand_buffer = None;
            return;
        };

        let verts = build_first_person_hand_vertices(cam, hand, self.block_textures.hand_layer);
        if verts.is_empty() {
            self.hand_buffer = None;
            return;
        }

        let buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("first_person_hand_vb"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.hand_buffer = Some(ChunkBuffer {
            vbuf: buf,
            vcount: verts.len() as u32,
            ibuf: None,
            icount: 0,
        });
    }

    pub fn update_player_visual(&mut self, visual: Option<PlayerVisual>) {
        let Some(visual) = visual else {
            self.player_buffer = None;
            return;
        };

        let verts = build_player_vertices(visual, self.block_textures.hand_layer);
        if verts.is_empty() {
            self.player_buffer = None;
            return;
        }

        let buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("player_visual_vb"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.player_buffer = Some(ChunkBuffer {
            vbuf: buf,
            vcount: verts.len() as u32,
            ibuf: None,
            icount: 0,
        });
    }

    pub fn update_break_overlay(&mut self, overlay: Option<BreakOverlayVisual>) {
        let Some(overlay) = overlay else {
            self.break_overlay_buffer = None;
            return;
        };

        let verts = build_break_overlay_vertices(overlay, self.block_textures.crack_first_layer);
        if verts.is_empty() {
            self.break_overlay_buffer = None;
            return;
        }

        let buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("break_overlay_vb"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.break_overlay_buffer = Some(ChunkBuffer {
            vbuf: buf,
            vcount: verts.len() as u32,
            ibuf: None,
            icount: 0,
        });
    }

    pub fn update_block_outline(&mut self, outline: Option<BlockOutlineVisual>) {
        let Some(outline) = outline else {
            self.block_outline_buffer = None;
            return;
        };

        let verts = build_block_outline_vertices(outline, self.block_textures.outline_layer);
        if verts.is_empty() {
            self.block_outline_buffer = None;
            return;
        }

        let buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("block_outline_vb"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.block_outline_buffer = Some(ChunkBuffer {
            vbuf: buf,
            vcount: verts.len() as u32,
            ibuf: None,
            icount: 0,
        });
    }

    // â”€â”€ Ð£Ð´Ð°Ð»Ð¸Ñ‚ÑŒ Ð±ÑƒÑ„ÐµÑ€Ñ‹ Ð²Ñ‹Ð³Ñ€ÑƒÐ¶ÐµÐ½Ð½Ñ‹Ñ… Ñ‡Ð°Ð½ÐºÐ¾Ð² â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    pub fn remove_chunks(&mut self, keys: &[(i32,i32)]) {
        let mut dirty_keys = false;
        for key in keys {
            dirty_keys |= self.chunk_buffers.remove(key).is_some();
        }
        if dirty_keys {
            self.chunk_keys = self.chunk_buffers.keys().copied().collect();
        }
    }

    pub fn clear_chunks(&mut self) {
        self.chunk_buffers.clear();
        self.drop_buffer = None;
        self.rain_buffer = None;
        self.hand_buffer = None;
        self.player_buffer = None;
        self.break_overlay_buffer = None;
        self.block_outline_buffer = None;
        self.chunk_keys.clear();
        self.visible_keys.clear();
    }

    pub fn update_camera(&mut self, cam: &Camera, day_time: f32) {
        self.day_time = day_time;
        let u = CameraUniform {
            view_proj: cam.view_proj(self.config.width, self.config.height).to_cols_array_2d(),
            time_of_day: [day_time, 0.0, 0.0, 0.0],
            cam_pos: [cam.pos.x, cam.pos.y, cam.pos.z, 0.0],
            lighting: self.lighting,
            rt_params: self.rt_params,
            parallax: [0.050, 0.0, 0.0, 0.0],
        };
        self.queue.write_buffer(&self.cam_buffer, 0, bytemuck::bytes_of(&u));
    }

    #[allow(dead_code)]
    pub fn set_ray_tracing_stub_enabled(&mut self, enabled: bool) {
        self.rt_params[0] = if enabled { 1.0 } else { 0.0 };
    }

    pub fn update_ray_tracing(
        &mut self,
        cam: &Camera,
        world: &World,
        day_time: f32,
        rain_strength: f32,
        rain_time: f32,
        surface_wetness: f32,
    ) {
        if !self.ray_tracing_enabled {
            return;
        }
        let Some(ray_tracer) = self.ray_tracer.as_mut() else {
            return;
        };
        self.day_time = day_time;
        ray_tracer.update(
            &self.queue,
            cam,
            self.config.width,
            self.config.height,
            day_time,
            rain_strength,
            rain_time,
            surface_wetness,
            world,
        );
    }

    /// ÐžÐ±Ð½Ð¾Ð²Ð¸Ñ‚ÑŒ ÑÐ¿Ð¸ÑÐ¾Ðº Ð²Ð¸Ð´Ð¸Ð¼Ñ‹Ñ… Ñ‡Ð°Ð½ÐºÐ¾Ð² (Ð²Ñ‹Ð·Ñ‹Ð²Ð°Ñ‚ÑŒ Ñ€Ð°Ð· Ð² ÐºÐ°Ð´Ñ€ Ð´Ð¾ render)
    pub fn update_visibility(&mut self, cam: &Camera) {
        let vp = cam.view_proj(self.config.width, self.config.height);
        let frustum = Frustum::from_view_proj(&vp);
        self.visible_keys = cull_chunks_parallel(
            &self.chunk_keys, &frustum, cam.pos, &self.cull_pool
        );
    }

    pub fn render(&mut self, _cam: &Camera, run_ui: impl FnOnce(&egui::Context)) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view   = output.texture.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("frame") }
        );

        if self.ray_tracing_enabled && self.ray_tracer.is_some() {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rt_main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load:  wgpu::LoadOp::Clear(sky_color_from_time(self.day_time)),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            if let Some(ray_tracer) = self.ray_tracer.as_ref() {
                ray_tracer.draw(&mut pass);
            }
        } else {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load:  wgpu::LoadOp::Clear(sky_color_from_time(self.day_time)),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_texture,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.cam_bind_group, &[]);
            pass.set_bind_group(1, &self.block_textures.bind_group, &[]);

            for &key in &self.visible_keys {
                if let Some(cb) = self.chunk_buffers.get(&key) {
                    draw_chunk_buffer(&mut pass, cb);
                }
            }

            if let Some(drop_buf) = self.drop_buffer.as_ref() {
                draw_chunk_buffer(&mut pass, drop_buf);
            }

            if let Some(rain_buf) = self.rain_buffer.as_ref() {
                draw_chunk_buffer(&mut pass, rain_buf);
            }

            if let Some(player_buf) = self.player_buffer.as_ref() {
                draw_chunk_buffer(&mut pass, player_buf);
            }

            if let Some(overlay_buf) = self.break_overlay_buffer.as_ref() {
                draw_chunk_buffer(&mut pass, overlay_buf);
            }

            if let Some(outline_buf) = self.block_outline_buffer.as_ref() {
                draw_chunk_buffer(&mut pass, outline_buf);
            }

            if let Some(hand_buf) = self.hand_buffer.as_ref() {
                draw_chunk_buffer(&mut pass, hand_buf);
            }
        }

        let window = Arc::clone(&self.window);
        self.egui.draw(
            &self.device,
            &self.queue,
            &mut enc,
            &view,
            &window,
            run_ui,
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

    pub fn set_vsync_enabled(&mut self, enabled: bool) {
        self.config.present_mode = if enabled {
            wgpu::PresentMode::AutoVsync
        } else {
            wgpu::PresentMode::AutoNoVsync
        };
        self.reconfigure();
    }

    pub fn set_lighting_params(
        &mut self,
        ambient_boost: f32,
        sun_softness: f32,
        fog_density: f32,
        exposure: f32,
    ) {
        self.lighting = [
            ambient_boost.clamp(0.5, 2.0),
            sun_softness.clamp(0.0, 1.0),
            fog_density.clamp(0.1, 3.0),
            exposure.clamp(0.5, 2.5),
        ];
    }

    pub fn window_arc(&self) -> Arc<Window> {
        Arc::clone(&self.window)
    }

    pub fn window(&self) -> &Window { &self.window }
    pub fn gpu_name(&self) -> &str { &self.gpu_name }
    pub fn graphics_api(&self) -> &str { &self.graphics_api }
    pub fn set_ray_tracing_enabled(&mut self, enabled: bool) {
        if enabled {
            if !self.ray_tracing_supported {
                log::warn!("Ray tracing requested but unsupported on current GPU/backend");
                self.ray_tracing_enabled = false;
                self.rt_params[0] = 0.0;
                return;
            }

            if self.ray_tracer.is_none() {
                self.ray_tracer = Some(RayTracingRenderer::new(
                    &self.device,
                    self.config.format,
                    &self.block_textures.view,
                    &self.block_textures.sampler,
                ));
            }
            self.ray_tracing_enabled = self.ray_tracer.is_some();
            self.rt_params[0] = if self.ray_tracing_enabled { 1.0 } else { 0.0 };
            return;
        }

        self.ray_tracing_enabled = false;
        self.rt_params[0] = 0.0;
    }
    pub fn ray_tracing_enabled(&self) -> bool {
        self.ray_tracing_enabled
    }
    #[allow(dead_code)]
    pub fn ray_tracing_supported(&self) -> bool {
        self.ray_tracing_supported
    }
    #[allow(dead_code)]
    pub fn chunk_count(&self) -> usize { self.chunk_buffers.len() }
}

fn draw_chunk_buffer<'a>(pass: &mut wgpu::RenderPass<'a>, cb: &'a ChunkBuffer) {
    pass.set_vertex_buffer(0, cb.vbuf.slice(..));
    if let Some(ibuf) = cb.ibuf.as_ref() {
        pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..cb.icount, 0, 0..1);
    } else {
        pass.draw(0..cb.vcount, 0..1);
    }
}

fn compact_triangles_to_indexed_quads(verts: &[Vertex]) -> (Vec<Vertex>, Vec<u32>) {
    if verts.is_empty() {
        return (Vec::new(), Vec::new());
    }
    if verts.len() % 6 != 0 {
        let fallback_indices = (0..verts.len() as u32).collect::<Vec<_>>();
        return (verts.to_vec(), fallback_indices);
    }

    let quad_count = verts.len() / 6;
    let mut out_verts = Vec::with_capacity(quad_count * 4);
    let mut out_indices = Vec::with_capacity(quad_count * 6);

    for q in 0..quad_count {
        let base_tri = q * 6;
        let base_vtx = (q * 4) as u32;
        out_verts.push(verts[base_tri]);
        out_verts.push(verts[base_tri + 1]);
        out_verts.push(verts[base_tri + 2]);
        out_verts.push(verts[base_tri + 5]);
        out_indices.extend_from_slice(&[
            base_vtx,
            base_vtx + 1,
            base_vtx + 2,
            base_vtx,
            base_vtx + 2,
            base_vtx + 3,
        ]);
    }

    (out_verts, out_indices)
}

fn build_drop_vertices(drops: &[DroppedBlockVisual]) -> Vec<Vertex> {
    const FACES: [([f32; 3], [[f32; 3]; 4]); 6] = [
        ([1.0, 0.0, 0.0], [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]]),
        ([-1.0, 0.0, 0.0], [[0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]]),
        ([0.0, 1.0, 0.0], [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]]),
        ([0.0, -1.0, 0.0], [[0.0, 0.0, 1.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0]]),
        ([0.0, 0.0, 1.0], [[1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0], [0.0, 0.0, 1.0]]),
        ([0.0, 0.0, -1.0], [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]]),
    ];
    const UVS: [[f32; 2]; 4] = [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];

    let mut verts = Vec::with_capacity(drops.len() * 36);

    for drop in drops {
        let scale = drop.scale.clamp(0.06, 1.0);
        let c = drop.yaw_rad.cos();
        let s = drop.yaw_rad.sin();

        for (face_idx, (normal, corners)) in FACES.iter().enumerate() {
            let mut v: [Vertex; 4] = std::array::from_fn(|_| Vertex {
                pos: [0.0; 3],
                normal: [0.0; 3],
                tex_idx: 0,
                uv: [0.0; 2],
            });

            let n_local = glam::Vec3::new(normal[0], normal[1], normal[2]);
            let n_rot = glam::Vec3::new(
                n_local.x * c - n_local.z * s,
                n_local.y,
                n_local.x * s + n_local.z * c,
            );
            let tex = drop_face_texture(drop.block, face_idx);

            for j in 0..4 {
                let corner = corners[j];
                let local = glam::Vec3::new(corner[0] - 0.5, corner[1] - 0.5, corner[2] - 0.5) * scale;
                let rotated = glam::Vec3::new(
                    local.x * c - local.z * s,
                    local.y,
                    local.x * s + local.z * c,
                );
                let p = drop.pos + rotated;
                v[j] = Vertex {
                    pos: [p.x, p.y, p.z],
                    normal: [n_rot.x, n_rot.y, n_rot.z],
                    tex_idx: tex,
                    uv: UVS[j],
                };
            }
            verts.extend_from_slice(&[v[0], v[1], v[2], v[0], v[2], v[3]]);
        }
    }

    verts
}

fn build_rain_vertices(drops: &[RainDropVisual]) -> Vec<Vertex> {
    const FACES: [([f32; 3], [[f32; 3]; 4]); 6] = [
        ([1.0, 0.0, 0.0], [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]]),
        ([-1.0, 0.0, 0.0], [[0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]]),
        ([0.0, 1.0, 0.0], [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]]),
        ([0.0, -1.0, 0.0], [[0.0, 0.0, 1.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0]]),
        ([0.0, 0.0, 1.0], [[1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0], [0.0, 0.0, 1.0]]),
        ([0.0, 0.0, -1.0], [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]]),
    ];
    const UVS: [[f32; 2]; 4] = [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];

    let tex_idx = Block::Water.texture_index();
    let mut verts = Vec::with_capacity(drops.len() * 36);

    for drop in drops {
        let thickness = drop.thickness.clamp(0.006, 0.040);
        let length = drop.length.clamp(0.12, 0.90);
        let size = glam::Vec3::new(thickness, length, thickness);
        let rot = glam::Quat::from_rotation_y(drop.yaw_rad)
            * glam::Quat::from_rotation_x(-0.22);

        for (normal, corners) in FACES {
            let mut v: [Vertex; 4] = std::array::from_fn(|_| Vertex {
                pos: [0.0; 3],
                normal: [0.0; 3],
                tex_idx,
                uv: [0.0; 2],
            });

            let n_local = glam::Vec3::new(normal[0], normal[1], normal[2]);
            let n_rot = rot * n_local;

            for j in 0..4 {
                let corner = corners[j];
                let local = glam::Vec3::new(
                    (corner[0] - 0.5) * size.x,
                    (corner[1] - 0.5) * size.y,
                    (corner[2] - 0.5) * size.z,
                );
                let p = drop.pos + rot * local;
                v[j] = Vertex {
                    pos: [p.x, p.y, p.z],
                    normal: [n_rot.x, n_rot.y, n_rot.z],
                    tex_idx,
                    uv: UVS[j],
                };
            }
            verts.extend_from_slice(&[v[0], v[1], v[2], v[0], v[2], v[3]]);
        }
    }

    verts
}

fn build_first_person_hand_vertices(
    cam: &Camera,
    hand: FirstPersonHandVisual,
    tex_idx: u32,
) -> Vec<Vertex> {
    const FACES: [([f32; 3], [[f32; 3]; 4]); 6] = [
        ([1.0, 0.0, 0.0], [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]]),
        ([-1.0, 0.0, 0.0], [[0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]]),
        ([0.0, 1.0, 0.0], [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]]),
        ([0.0, -1.0, 0.0], [[0.0, 0.0, 1.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0]]),
        ([0.0, 0.0, 1.0], [[1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0], [0.0, 0.0, 1.0]]),
        ([0.0, 0.0, -1.0], [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]]),
    ];
    const UVS: [[f32; 2]; 4] = [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];

    let forward = cam.forward();
    let mut right = forward.cross(glam::Vec3::Y);
    if right.length_squared() <= 1e-6 {
        right = glam::Vec3::X;
    } else {
        right = right.normalize();
    }
    let up = right.cross(forward).normalize_or_zero();

    let swing = hand.swing.clamp(0.0, 1.0);
    let phase = hand.phase_rad;
    let action = hand.action_strength.clamp(0.0, 1.0);
    let action_phase = hand.action_phase_rad;

    let walk_x = phase.sin() * 0.010 * swing;
    let walk_y = (phase * 2.0).sin().abs() * 0.012 * swing;
    let idle_y = (phase * 0.55).sin() * 0.002;
    let attack_pull = (action_phase.sin().abs()) * action;
    let anchor = cam.pos
        + forward * (0.48 - attack_pull * 0.05)
        + right * (0.38 + walk_x)
        - up * (0.44 - walk_y - idle_y + attack_pull * 0.03);

    let base_rot = glam::Quat::from_rotation_x(-1.06)
        * glam::Quat::from_rotation_y(0.34)
        * glam::Quat::from_rotation_z(-0.08);
    let walk_rot = glam::Quat::from_rotation_x(phase.sin() * 0.06 * swing)
        * glam::Quat::from_rotation_z(phase.sin() * 0.04 * swing);
    let attack_rot = glam::Quat::from_rotation_x(-attack_pull * 0.42)
        * glam::Quat::from_rotation_y(-attack_pull * 0.08);
    let rot = attack_rot * walk_rot * base_rot;

    let mut verts = Vec::with_capacity(36);
    let forearm_size = glam::Vec3::new(0.11, 0.36, 0.11);

    append_box_faces(
        &mut verts,
        &FACES,
        &UVS,
        right,
        up,
        forward,
        anchor,
        rot,
        tex_idx,
        forearm_size,
        glam::Vec3::new(0.0, -0.18, 0.0),
    );

    if hand.held_tool.is_some_and(|tool| tool.is_hoe()) {
        let hoe_local = rot * glam::Vec3::new(0.05, -0.34, 0.08);
        let hoe_anchor = anchor
            + right * hoe_local.x
            + up * hoe_local.y
            + forward * hoe_local.z;
        let hoe_rot = rot
            * glam::Quat::from_rotation_x(-0.96)
            * glam::Quat::from_rotation_y(0.16)
            * glam::Quat::from_rotation_z(0.10);
        append_hoe_model(
            &mut verts,
            &FACES,
            &UVS,
            right,
            up,
            forward,
            hoe_anchor,
            hoe_rot,
            tex_idx,
        );
    } else if hand.held_tool.is_some_and(|tool| tool.is_sword()) {
        let sword_local = rot * glam::Vec3::new(0.05, -0.33, 0.07);
        let sword_anchor = anchor
            + right * sword_local.x
            + up * sword_local.y
            + forward * sword_local.z;
        let sword_rot = rot
            * glam::Quat::from_rotation_x(-1.06)
            * glam::Quat::from_rotation_y(0.10)
            * glam::Quat::from_rotation_z(0.04);
        append_sword_model(
            &mut verts,
            &FACES,
            &UVS,
            right,
            up,
            forward,
            sword_anchor,
            sword_rot,
            tex_idx,
        );
    } else if let Some(block) = hand.held_block {
        let held_local = rot * glam::Vec3::new(0.04, -0.36, 0.06);
        let held_anchor = anchor
            + right * held_local.x
            + up * held_local.y
            + forward * held_local.z;
        let held_rot = rot
            * glam::Quat::from_rotation_x(-0.88)
            * glam::Quat::from_rotation_y(0.28)
            * glam::Quat::from_rotation_z(0.06);

        append_item_cube(
            &mut verts,
            &FACES,
            &UVS,
            right,
            up,
            forward,
            held_anchor,
            held_rot,
            block,
            glam::Vec3::splat(0.20),
        );
    }

    verts
}

fn build_player_vertices(player: PlayerVisual, hand_tex_idx: u32) -> Vec<Vertex> {
    const FACES: [([f32; 3], [[f32; 3]; 4]); 6] = [
        ([1.0, 0.0, 0.0], [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]]),
        ([-1.0, 0.0, 0.0], [[0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]]),
        ([0.0, 1.0, 0.0], [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]]),
        ([0.0, -1.0, 0.0], [[0.0, 0.0, 1.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0]]),
        ([0.0, 0.0, 1.0], [[1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0], [0.0, 0.0, 1.0]]),
        ([0.0, 0.0, -1.0], [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]]),
    ];
    const UVS: [[f32; 2]; 4] = [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];

    let mut verts = Vec::with_capacity(36 * 8);
    let mut forward = glam::Vec3::new(player.yaw_rad.cos(), 0.0, player.yaw_rad.sin());
    if forward.length_squared() <= 1e-6 {
        forward = glam::Vec3::Z;
    } else {
        forward = forward.normalize();
    }
    let right = glam::Vec3::new(-forward.z, 0.0, forward.x).normalize_or_zero();
    let up = glam::Vec3::Y;

    let feet = player.feet_pos;
    let swing = player.swing.clamp(0.0, 1.0);
    let walk = player.phase_rad.sin() * 0.70 * swing;
    let attack = player.action_phase_rad.sin().abs() * player.action_strength.clamp(0.0, 1.0);

    let leg_size = glam::Vec3::new(0.14, 0.90, 0.14);
    let body_size = glam::Vec3::new(0.38, 0.58, 0.22);
    let arm_size = glam::Vec3::new(0.13, 0.54, 0.13);
    let head_size = glam::Vec3::new(0.34, 0.34, 0.34);

    let leg_y = feet.y + 0.90;
    let body_center = feet + up * 1.19;
    let shoulder = feet + up * 1.43;
    let head_center = feet + up * 1.66 + forward * 0.01;

    let right_leg_anchor = glam::Vec3::new(feet.x, leg_y, feet.z) + right * 0.10;
    let left_leg_anchor = glam::Vec3::new(feet.x, leg_y, feet.z) - right * 0.10;
    let right_leg_rot = glam::Quat::from_rotation_x(-walk * 0.60);
    let left_leg_rot = glam::Quat::from_rotation_x(walk * 0.60);
    let body_rot = glam::Quat::IDENTITY;
    let right_arm_rot = glam::Quat::from_rotation_x(walk * 0.50 - attack * 0.95)
        * glam::Quat::from_rotation_z(0.06);
    let left_arm_rot = glam::Quat::from_rotation_x(-walk * 0.42) * glam::Quat::from_rotation_z(-0.06);

    append_box_faces(
        &mut verts,
        &FACES,
        &UVS,
        right,
        up,
        forward,
        right_leg_anchor,
        right_leg_rot,
        hand_tex_idx,
        leg_size,
        glam::Vec3::new(0.0, -0.45, 0.0),
    );
    append_box_faces(
        &mut verts,
        &FACES,
        &UVS,
        right,
        up,
        forward,
        left_leg_anchor,
        left_leg_rot,
        hand_tex_idx,
        leg_size,
        glam::Vec3::new(0.0, -0.45, 0.0),
    );
    append_box_faces(
        &mut verts,
        &FACES,
        &UVS,
        right,
        up,
        forward,
        body_center,
        body_rot,
        hand_tex_idx,
        body_size,
        glam::Vec3::ZERO,
    );
    append_box_faces(
        &mut verts,
        &FACES,
        &UVS,
        right,
        up,
        forward,
        head_center,
        body_rot,
        hand_tex_idx,
        head_size,
        glam::Vec3::ZERO,
    );

    let right_arm_anchor = shoulder + right * 0.26;
    let left_arm_anchor = shoulder - right * 0.26;
    append_box_faces(
        &mut verts,
        &FACES,
        &UVS,
        right,
        up,
        forward,
        right_arm_anchor,
        right_arm_rot,
        hand_tex_idx,
        arm_size,
        glam::Vec3::new(0.0, -0.27, 0.0),
    );
    append_box_faces(
        &mut verts,
        &FACES,
        &UVS,
        right,
        up,
        forward,
        left_arm_anchor,
        left_arm_rot,
        hand_tex_idx,
        arm_size,
        glam::Vec3::new(0.0, -0.27, 0.0),
    );

    if player.held_tool.is_some_and(|tool| tool.is_hoe()) {
        let hold_local = right_arm_rot * glam::Vec3::new(0.04, -0.50, 0.04);
        let hold_anchor = right_arm_anchor
            + right * hold_local.x
            + up * hold_local.y
            + forward * hold_local.z;
        let hold_rot = right_arm_rot
            * glam::Quat::from_rotation_x(-1.00)
            * glam::Quat::from_rotation_y(0.14)
            * glam::Quat::from_rotation_z(0.10);
        append_hoe_model(
            &mut verts,
            &FACES,
            &UVS,
            right,
            up,
            forward,
            hold_anchor,
            hold_rot,
            hand_tex_idx,
        );
    } else if player.held_tool.is_some_and(|tool| tool.is_sword()) {
        let hold_local = right_arm_rot * glam::Vec3::new(0.04, -0.51, 0.03);
        let hold_anchor = right_arm_anchor
            + right * hold_local.x
            + up * hold_local.y
            + forward * hold_local.z;
        let hold_rot = right_arm_rot
            * glam::Quat::from_rotation_x(-1.08)
            * glam::Quat::from_rotation_y(0.08)
            * glam::Quat::from_rotation_z(0.05);
        append_sword_model(
            &mut verts,
            &FACES,
            &UVS,
            right,
            up,
            forward,
            hold_anchor,
            hold_rot,
            hand_tex_idx,
        );
    } else if let Some(block) = player.held_block {
        let hold_local = right_arm_rot * glam::Vec3::new(0.03, -0.52, 0.03);
        let hold_anchor = right_arm_anchor
            + right * hold_local.x
            + up * hold_local.y
            + forward * hold_local.z;
        let hold_rot = right_arm_rot
            * glam::Quat::from_rotation_x(-0.95)
            * glam::Quat::from_rotation_y(0.25)
            * glam::Quat::from_rotation_z(0.08);

        append_item_cube(
            &mut verts,
            &FACES,
            &UVS,
            right,
            up,
            forward,
            hold_anchor,
            hold_rot,
            block,
            glam::Vec3::splat(0.22),
        );
    }

    verts
}

#[allow(clippy::too_many_arguments)]
fn append_item_cube(
    verts: &mut Vec<Vertex>,
    faces: &[([f32; 3], [[f32; 3]; 4]); 6],
    uvs: &[[f32; 2]; 4],
    right: glam::Vec3,
    up: glam::Vec3,
    forward: glam::Vec3,
    anchor: glam::Vec3,
    rot: glam::Quat,
    block: Block,
    size: glam::Vec3,
) {
    for (face_idx, (normal, corners)) in faces.iter().enumerate() {
        let tex_idx = drop_face_texture(block, face_idx);
        let mut v: [Vertex; 4] = std::array::from_fn(|_| Vertex {
            pos: [0.0; 3],
            normal: [0.0; 3],
            tex_idx,
            uv: [0.0; 2],
        });

        let n_local = glam::Vec3::new(normal[0], normal[1], normal[2]);
        let n_rot = rot * n_local;
        let n_world = right * n_rot.x + up * n_rot.y + forward * n_rot.z;

        for j in 0..4 {
            let c = corners[j];
            let local = glam::Vec3::new(
                (c[0] - 0.5) * size.x,
                (c[1] - 0.5) * size.y,
                (c[2] - 0.5) * size.z,
            );
            let p_local = rot * local;
            let p_world = anchor + right * p_local.x + up * p_local.y + forward * p_local.z;
            v[j] = Vertex {
                pos: [p_world.x, p_world.y, p_world.z],
                normal: [n_world.x, n_world.y, n_world.z],
                tex_idx,
                uv: uvs[j],
            };
        }

        verts.extend_from_slice(&[v[0], v[1], v[2], v[0], v[2], v[3]]);
    }
}

#[allow(clippy::too_many_arguments)]
fn append_hoe_model(
    verts: &mut Vec<Vertex>,
    faces: &[([f32; 3], [[f32; 3]; 4]); 6],
    uvs: &[[f32; 2]; 4],
    right: glam::Vec3,
    up: glam::Vec3,
    forward: glam::Vec3,
    anchor: glam::Vec3,
    rot: glam::Quat,
    tex_idx: u32,
) {
    // Two-part simplified hoe: wooden handle + compact head.
    append_box_faces(
        verts,
        faces,
        uvs,
        right,
        up,
        forward,
        anchor,
        rot,
        tex_idx,
        glam::Vec3::new(0.045, 0.44, 0.045),
        glam::Vec3::new(0.0, -0.20, 0.0),
    );
    append_box_faces(
        verts,
        faces,
        uvs,
        right,
        up,
        forward,
        anchor,
        rot,
        tex_idx,
        glam::Vec3::new(0.18, 0.06, 0.06),
        glam::Vec3::new(0.07, 0.04, 0.0),
    );
}

#[allow(clippy::too_many_arguments)]
fn append_sword_model(
    verts: &mut Vec<Vertex>,
    faces: &[([f32; 3], [[f32; 3]; 4]); 6],
    uvs: &[[f32; 2]; 4],
    right: glam::Vec3,
    up: glam::Vec3,
    forward: glam::Vec3,
    anchor: glam::Vec3,
    rot: glam::Quat,
    tex_idx: u32,
) {
    // Simplified sword: handle + guard + blade.
    append_box_faces(
        verts,
        faces,
        uvs,
        right,
        up,
        forward,
        anchor,
        rot,
        tex_idx,
        glam::Vec3::new(0.040, 0.24, 0.040),
        glam::Vec3::new(0.0, -0.22, 0.0),
    );
    append_box_faces(
        verts,
        faces,
        uvs,
        right,
        up,
        forward,
        anchor,
        rot,
        tex_idx,
        glam::Vec3::new(0.18, 0.03, 0.05),
        glam::Vec3::new(0.0, -0.08, 0.0),
    );
    append_box_faces(
        verts,
        faces,
        uvs,
        right,
        up,
        forward,
        anchor,
        rot,
        tex_idx,
        glam::Vec3::new(0.07, 0.52, 0.03),
        glam::Vec3::new(0.0, 0.20, 0.0),
    );
}

#[allow(clippy::too_many_arguments)]
fn append_box_faces(
    verts: &mut Vec<Vertex>,
    faces: &[([f32; 3], [[f32; 3]; 4]); 6],
    uvs: &[[f32; 2]; 4],
    right: glam::Vec3,
    up: glam::Vec3,
    forward: glam::Vec3,
    anchor: glam::Vec3,
    rot: glam::Quat,
    tex_idx: u32,
    size: glam::Vec3,
    offset: glam::Vec3,
) {
    for (normal, corners) in faces {
        let mut v: [Vertex; 4] = std::array::from_fn(|_| Vertex {
            pos: [0.0; 3],
            normal: [0.0; 3],
            tex_idx,
            uv: [0.0; 2],
        });

        let n_local = glam::Vec3::new(normal[0], normal[1], normal[2]);
        let n_cam = rot * n_local;
        let n_world = right * n_cam.x + up * n_cam.y + forward * n_cam.z;

        for j in 0..4 {
            let c = corners[j];
            let local = glam::Vec3::new(
                (c[0] - 0.5) * size.x,
                (c[1] - 0.5) * size.y,
                (c[2] - 0.5) * size.z,
            ) + offset;
            let p_cam = rot * local;
            let p_world = anchor + right * p_cam.x + up * p_cam.y + forward * p_cam.z;
            v[j] = Vertex {
                pos: [p_world.x, p_world.y, p_world.z],
                normal: [n_world.x, n_world.y, n_world.z],
                tex_idx,
                uv: uvs[j],
            };
        }

        verts.extend_from_slice(&[v[0], v[1], v[2], v[0], v[2], v[3]]);
    }
}

fn build_break_overlay_vertices(overlay: BreakOverlayVisual, crack_first_layer: u32) -> Vec<Vertex> {
    const UVS: [[f32; 2]; 4] = [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];

    let t = overlay.progress.clamp(0.0, 1.0);
    let stage = crack_stage_from_progress(t);
    let tex_idx = crack_first_layer + stage;
    let (nx, ny, nz) = overlay.face_normal;
    let normal = glam::Vec3::new(nx as f32, ny as f32, nz as f32);
    if normal.length_squared() <= 0.01 {
        return Vec::new();
    }

    let bx = overlay.block.0 as f32;
    let by = overlay.block.1 as f32;
    let bz = overlay.block.2 as f32;
    let eps = 0.0018f32;
    let inset = 0.0f32;
    let a = inset;
    let b = 1.0 - inset;

    let corners: [glam::Vec3; 4] = match (nx, ny, nz) {
        (1, 0, 0) => {
            let x = bx + 1.0 + eps;
            [
                glam::Vec3::new(x, by + a, bz + a),
                glam::Vec3::new(x, by + b, bz + a),
                glam::Vec3::new(x, by + b, bz + b),
                glam::Vec3::new(x, by + a, bz + b),
            ]
        }
        (-1, 0, 0) => {
            let x = bx - eps;
            [
                glam::Vec3::new(x, by + a, bz + b),
                glam::Vec3::new(x, by + b, bz + b),
                glam::Vec3::new(x, by + b, bz + a),
                glam::Vec3::new(x, by + a, bz + a),
            ]
        }
        (0, 1, 0) => {
            let y = by + 1.0 + eps;
            [
                glam::Vec3::new(bx + a, y, bz + a),
                glam::Vec3::new(bx + a, y, bz + b),
                glam::Vec3::new(bx + b, y, bz + b),
                glam::Vec3::new(bx + b, y, bz + a),
            ]
        }
        (0, -1, 0) => {
            let y = by - eps;
            [
                glam::Vec3::new(bx + a, y, bz + b),
                glam::Vec3::new(bx + a, y, bz + a),
                glam::Vec3::new(bx + b, y, bz + a),
                glam::Vec3::new(bx + b, y, bz + b),
            ]
        }
        (0, 0, 1) => {
            let z = bz + 1.0 + eps;
            [
                glam::Vec3::new(bx + b, by + a, z),
                glam::Vec3::new(bx + b, by + b, z),
                glam::Vec3::new(bx + a, by + b, z),
                glam::Vec3::new(bx + a, by + a, z),
            ]
        }
        (0, 0, -1) => {
            let z = bz - eps;
            [
                glam::Vec3::new(bx + a, by + a, z),
                glam::Vec3::new(bx + a, by + b, z),
                glam::Vec3::new(bx + b, by + b, z),
                glam::Vec3::new(bx + b, by + a, z),
            ]
        }
        _ => return Vec::new(),
    };

    let mut v: [Vertex; 4] = std::array::from_fn(|_| Vertex {
        pos: [0.0; 3],
        normal: [normal.x, normal.y, normal.z],
        tex_idx,
        uv: [0.0; 2],
    });
    for i in 0..4 {
        let p = corners[i];
        v[i] = Vertex {
            pos: [p.x, p.y, p.z],
            normal: [normal.x, normal.y, normal.z],
            tex_idx,
            uv: UVS[i],
        };
    }
    vec![v[0], v[1], v[2], v[0], v[2], v[3]]
}

fn build_block_outline_vertices(outline: BlockOutlineVisual, outline_tex_layer: u32) -> Vec<Vertex> {
    let bx = outline.block.0 as f32;
    let by = outline.block.1 as f32;
    let bz = outline.block.2 as f32;

    let eps = 0.0024f32;
    let edge = 0.014f32;
    let min = glam::Vec3::new(bx - eps, by - eps, bz - eps);
    let max = glam::Vec3::new(bx + 1.0 + eps, by + 1.0 + eps, bz + 1.0 + eps);

    let mut verts = Vec::with_capacity(12 * 36);

    for &y in &[min.y, max.y] {
        for &z in &[min.z, max.z] {
            append_axis_box(
                &mut verts,
                glam::Vec3::new(min.x, y - edge, z - edge),
                glam::Vec3::new(max.x, y + edge, z + edge),
                outline_tex_layer,
            );
        }
    }
    for &x in &[min.x, max.x] {
        for &z in &[min.z, max.z] {
            append_axis_box(
                &mut verts,
                glam::Vec3::new(x - edge, min.y, z - edge),
                glam::Vec3::new(x + edge, max.y, z + edge),
                outline_tex_layer,
            );
        }
    }
    for &x in &[min.x, max.x] {
        for &y in &[min.y, max.y] {
            append_axis_box(
                &mut verts,
                glam::Vec3::new(x - edge, y - edge, min.z),
                glam::Vec3::new(x + edge, y + edge, max.z),
                outline_tex_layer,
            );
        }
    }

    verts
}

#[inline]
fn crack_stage_from_progress(progress: f32) -> u32 {
    // 0..100% -> 10 crack stages (0..9), so 50% => stage 5.
    let pct = (progress.clamp(0.0, 1.0) * 100.0).floor() as u32;
    let step = (100 / CRACK_STAGE_COUNT as u32).max(1);
    (pct / step).min((CRACK_STAGE_COUNT as u32).saturating_sub(1))
}

fn append_axis_box(
    verts: &mut Vec<Vertex>,
    min: glam::Vec3,
    max: glam::Vec3,
    tex_idx: u32,
) {
    const FACES: [([f32; 3], [[f32; 3]; 4]); 6] = [
        ([1.0, 0.0, 0.0], [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]]),
        ([-1.0, 0.0, 0.0], [[0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]]),
        ([0.0, 1.0, 0.0], [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]]),
        ([0.0, -1.0, 0.0], [[0.0, 0.0, 1.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0]]),
        ([0.0, 0.0, 1.0], [[1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0], [0.0, 0.0, 1.0]]),
        ([0.0, 0.0, -1.0], [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]]),
    ];
    const UVS: [[f32; 2]; 4] = [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];

    let size = max - min;
    if size.x <= 0.0 || size.y <= 0.0 || size.z <= 0.0 {
        return;
    }

    for (normal, corners) in FACES {
        let mut v: [Vertex; 4] = std::array::from_fn(|_| Vertex {
            pos: [0.0; 3],
            normal: normal,
            tex_idx,
            uv: [0.0; 2],
        });

        for j in 0..4 {
            let c = corners[j];
            let p = glam::Vec3::new(
                min.x + c[0] * size.x,
                min.y + c[1] * size.y,
                min.z + c[2] * size.z,
            );
            v[j] = Vertex {
                pos: [p.x, p.y, p.z],
                normal,
                tex_idx,
                uv: UVS[j],
            };
        }
        verts.extend_from_slice(&[v[0], v[1], v[2], v[0], v[2], v[3]]);
    }
}

#[inline]
fn drop_face_texture(block: Block, face_idx: usize) -> u32 {
    const WORKBENCH_TOP_TEX_IDX: u32 = 19;
    const WORKBENCH_FRONT_TEX_IDX: u32 = 20;
    const FURNACE_TOP_TEX_IDX: u32 = 22;
    const FURNACE_FRONT_TEX_IDX: u32 = 23;

    match block {
        Block::Grass => {
            if face_idx == 2 { Block::Grass.texture_index() }
            else { Block::Dirt.texture_index() }
        }
        Block::Log => {
            if face_idx == 2 || face_idx == 3 { Block::LogBottom.texture_index() }
            else { Block::Log.texture_index() }
        }
        Block::Wood => {
            if face_idx == 2 || face_idx == 3 { Block::LogBottom.texture_index() }
            else { Block::Wood.texture_index() }
        }
        Block::Workbench => {
            if face_idx == 2 || face_idx == 3 {
                WORKBENCH_TOP_TEX_IDX
            } else if face_idx == 4 || face_idx == 5 {
                WORKBENCH_FRONT_TEX_IDX
            } else {
                Block::Workbench.texture_index()
            }
        }
        Block::Furnace => {
            if face_idx == 2 || face_idx == 3 {
                FURNACE_TOP_TEX_IDX
            } else if face_idx == 4 {
                FURNACE_FRONT_TEX_IDX
            } else {
                Block::Furnace.texture_index()
            }
        }
        _ => block.texture_index(),
    }
}

fn sky_color_from_time(t: f32) -> wgpu::Color {
    let angle = t * std::f32::consts::TAU;
    let sun_y = angle.sin();
    let day = smoothstep(-0.08, 0.14, sun_y);
    let twilight = 1.0 - smoothstep(0.02, 0.42, sun_y.abs());

    let day_sky = [0.56, 0.80, 1.00];
    let night_sky = [0.015, 0.025, 0.060];
    let dusk_sky = [1.00, 0.56, 0.26];

    let mut r = day_sky[0] * day + night_sky[0] * (1.0 - day);
    let mut g = day_sky[1] * day + night_sky[1] * (1.0 - day);
    let mut b = day_sky[2] * day + night_sky[2] * (1.0 - day);
    r = r * (1.0 - twilight * 0.45) + dusk_sky[0] * twilight * 0.45;
    g = g * (1.0 - twilight * 0.45) + dusk_sky[1] * twilight * 0.45;
    b = b * (1.0 - twilight * 0.45) + dusk_sky[2] * twilight * 0.45;

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
    const TEX_NAMES: [&str; 27] = [
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
        "coal_ore",
        "iron_ore",
        "copper_ore",
        "farmland_dry",
        "farmland_wet",
        "grass_top",
        "workbench",
        "wood",
        "stick",
        "workbench_top",
        "workbench_front",
        "furnace",
        "furnace_top",
        "furnace_front",
        "coal",
        "torch",
        "iron_ingot",
    ];

    let pack = load_active_resource_pack();
    let mut base_size = None::<(u32, u32)>;
    let mut source_layers: Vec<(&str, RgbaImage, Option<GrayImage>)> = Vec::with_capacity(TEX_NAMES.len());

    for name in TEX_NAMES {
        let color = if name == "air" {
            RgbaImage::from_pixel(16, 16, Rgba([0, 0, 0, 0]))
        } else {
            load_block_texture(name, pack.as_ref())
        };
        if name != "air" && base_size.is_none() {
            base_size = Some((color.width(), color.height()));
        }

        let parallax = if name == "air" {
            None
        } else {
            load_parallax_texture(name, pack.as_ref())
        };
        source_layers.push((name, color, parallax));
    }

    let (width, height) = base_size.unwrap_or((16, 16));
    let mut layers: Vec<RgbaImage> = Vec::with_capacity(TEX_NAMES.len() + CRACK_STAGE_COUNT + 1);
    let mut parallax_layers: Vec<GrayImage> = Vec::with_capacity(TEX_NAMES.len() + CRACK_STAGE_COUNT + 1);
    let neutral_parallax = neutral_parallax_map(width, height);

    for (name, color, parallax) in source_layers {
        let prepared = if color.width() == width && color.height() == height {
            color
        } else {
            log::warn!(
                "Resizing block texture '{}' from {}x{} to {}x{}",
                name,
                color.width(),
                color.height(),
                width,
                height
            );
            image::imageops::resize(&color, width, height, FilterType::Nearest)
        };
        layers.push(prepared);

        let prepared_parallax = match parallax {
            Some(height_img) if height_img.width() == width && height_img.height() == height => height_img,
            Some(height_img) => image::imageops::resize(&height_img, width, height, FilterType::Nearest),
            None => neutral_parallax.clone(),
        };
        parallax_layers.push(prepared_parallax);
    }

    let crack_first_layer = layers.len() as u32;
    for stage in 0..CRACK_STAGE_COUNT {
        layers.push(generate_crack_texture(width, height, stage));
        parallax_layers.push(neutral_parallax.clone());
    }
    let hand_layer = layers.len() as u32;
    layers.push(generate_hand_texture(width, height));
    parallax_layers.push(neutral_parallax.clone());
    let outline_layer = layers.len() as u32;
    layers.push(generate_outline_texture(width, height));
    parallax_layers.push(neutral_parallax.clone());

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

    let parallax_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("block_parallax_array"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: parallax_layers.len() as u32,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    for (layer, img) in parallax_layers.iter().enumerate() {
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &parallax_texture,
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
                bytes_per_row: Some(width),
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
    let parallax_view = parallax_texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("block_parallax_array_view"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("block_sampler"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        // Repeat is required for merged greedy quads with tiled UV (>1.0).
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
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
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&parallax_view),
            },
        ],
    });

    BlockTextures {
        bind_group,
        _texture: texture,
        view,
        _parallax_texture: parallax_texture,
        _parallax_view: parallax_view,
        sampler,
        crack_first_layer,
        hand_layer,
        outline_layer,
    }
}

fn generate_crack_texture(width: u32, height: u32, stage: usize) -> RgbaImage {
    let w = width.max(2);
    let h = height.max(2);
    let mut img = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 0]));
    let stage = stage.min(CRACK_STAGE_COUNT.saturating_sub(1)) as u8;
    let alpha = (102u16 + stage as u16 * 11).min(212) as u8;
    let pattern = crack_pattern_stage_map();

    for y in 0..h {
        let py = if h <= 1 {
            0usize
        } else {
            ((y as usize * (CRACK_PATTERN_SIZE - 1)) + ((h as usize - 1) / 2)) / (h as usize - 1)
        };
        for x in 0..w {
            let px = if w <= 1 {
                0usize
            } else {
                ((x as usize * (CRACK_PATTERN_SIZE - 1)) + ((w as usize - 1) / 2)) / (w as usize - 1)
            };

            let cell_stage = pattern[py * CRACK_PATTERN_SIZE + px];
            if cell_stage > stage {
                continue;
            }

            // Slight per-pixel shade variation to keep crisp pixel-art look.
            let n = hash32(
                (px as u32).wrapping_mul(0x9E37_79B9)
                    ^ (py as u32).wrapping_mul(0x85EB_CA6B)
                    ^ (stage as u32).wrapping_mul(0xC2B2_AE35),
            );
            let shade = 22u8.saturating_add((n & 0x07) as u8);
            img.put_pixel(x, y, Rgba([shade, shade, shade, alpha]));
        }
    }

    img
}

fn crack_pattern_stage_map() -> [u8; CRACK_PATTERN_SIZE * CRACK_PATTERN_SIZE] {
    let mut map = [u8::MAX; CRACK_PATTERN_SIZE * CRACK_PATTERN_SIZE];

    apply_crack_stage_points(&mut map, 0, &[(8, 8)]);
    apply_crack_stage_points(&mut map, 1, &[(8, 7), (7, 6), (9, 6)]);
    apply_crack_stage_points(
        &mut map,
        2,
        &[(8, 9), (6, 5), (10, 5), (7, 7), (9, 7)],
    );
    apply_crack_stage_points(
        &mut map,
        3,
        &[(5, 4), (11, 4), (6, 6), (10, 6), (8, 10), (7, 9), (9, 9), (8, 6)],
    );
    apply_crack_stage_points(
        &mut map,
        4,
        &[
            (4, 3), (12, 3), (5, 5), (11, 5), (6, 8), (10, 8), (8, 11), (7, 10), (9, 10),
            (8, 5), (7, 8), (9, 8),
        ],
    );
    apply_crack_stage_points(
        &mut map,
        5,
        &[
            (3, 2), (13, 2), (4, 4), (12, 4), (5, 7), (11, 7), (6, 10), (10, 10), (8, 12),
            (7, 11), (9, 11), (6, 4), (10, 4), (5, 8), (11, 8),
        ],
    );
    apply_crack_stage_points(
        &mut map,
        6,
        &[
            (2, 1), (14, 1), (3, 3), (13, 3), (4, 6), (12, 6), (5, 9), (11, 9), (6, 12),
            (10, 12), (8, 13), (7, 12), (9, 12), (5, 3), (11, 3), (8, 4), (7, 4), (9, 4),
            (4, 8), (12, 8), (3, 6), (13, 6),
        ],
    );
    apply_crack_stage_points(
        &mut map,
        7,
        &[
            (1, 0), (15, 0), (1, 2), (15, 2), (2, 4), (14, 4), (3, 7), (13, 7), (4, 10),
            (12, 10), (5, 12), (11, 12), (6, 14), (10, 14), (8, 15), (7, 14), (9, 14), (0, 5),
            (15, 5), (0, 9), (15, 9), (2, 11), (14, 11), (1, 7), (15, 7),
        ],
    );
    apply_crack_stage_points(
        &mut map,
        8,
        &[
            (0, 0), (0, 2), (2, 0), (14, 0), (15, 1), (0, 7), (15, 7), (0, 11), (15, 11),
            (2, 13), (14, 13), (3, 15), (13, 15), (5, 14), (11, 14), (6, 15), (10, 15), (4, 12),
            (12, 12), (3, 9), (13, 9), (2, 6), (14, 6), (1, 5), (15, 4), (1, 10), (14, 10),
            (6, 2), (10, 2), (4, 14), (12, 14),
        ],
    );
    apply_crack_stage_points(
        &mut map,
        9,
        &[
            (6, 3), (7, 3), (8, 3), (9, 3), (10, 3), (5, 6), (6, 7), (7, 8), (9, 8), (10, 7),
            (11, 6), (5, 10), (6, 11), (7, 13), (9, 13), (10, 11), (11, 10), (4, 9), (12, 9),
            (4, 5), (12, 5), (5, 1), (10, 1), (3, 11), (13, 11), (2, 8), (14, 8), (6, 9), (10, 9),
            (7, 5), (9, 5), (8, 2), (8, 14),
        ],
    );

    map
}

fn apply_crack_stage_points(
    map: &mut [u8; CRACK_PATTERN_SIZE * CRACK_PATTERN_SIZE],
    stage: u8,
    points: &[(usize, usize)],
) {
    for &(x, y) in points {
        if x >= CRACK_PATTERN_SIZE || y >= CRACK_PATTERN_SIZE {
            continue;
        }
        let idx = y * CRACK_PATTERN_SIZE + x;
        if stage < map[idx] {
            map[idx] = stage;
        }
    }
}

fn hash32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^ (x >> 16)
}

fn generate_hand_texture(width: u32, height: u32) -> RgbaImage {
    let w = width.max(2);
    let h = height.max(2);
    let mut img = RgbaImage::from_pixel(w, h, Rgba([216, 174, 145, 255]));

    for y in 0..h {
        for x in 0..w {
            let n = hash32(
                x.wrapping_mul(1_973) ^ y.wrapping_mul(9_277) ^ 0xA5A5_A5A5,
            );
            let tone = ((n & 0x07) as u8).saturating_mul(2);
            let mut px = Rgba([
                216u8.saturating_sub(tone),
                174u8.saturating_sub(tone),
                145u8.saturating_sub(tone / 2),
                255,
            ]);

            let edge = if x < w / 8 || x > (w * 7) / 8 { 12 } else { 0 };
            let top = if y < h / 7 { 8 } else { 0 };
            let bottom = if y > (h * 6) / 7 { 10 } else { 0 };
            let shade = edge + top + bottom;
            px[0] = px[0].saturating_sub(shade);
            px[1] = px[1].saturating_sub(shade);
            px[2] = px[2].saturating_sub(shade / 2);
            img.put_pixel(x, y, px);
        }
    }

    img
}

fn generate_outline_texture(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_pixel(width.max(1), height.max(1), Rgba([0, 0, 0, 255]))
}

#[derive(Debug)]
struct ResourcePackData {
    source_path: PathBuf,
    entries: HashMap<String, Vec<u8>>,
}

fn neutral_parallax_map(width: u32, height: u32) -> GrayImage {
    GrayImage::from_pixel(width.max(1), height.max(1), Luma([128]))
}

fn load_active_resource_pack() -> Option<ResourcePackData> {
    let mut zip_paths = find_resource_pack_zips();
    if zip_paths.is_empty() {
        return None;
    }

    zip_paths.sort_by(|a, b| {
        let ma = std::fs::metadata(a)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let mb = std::fs::metadata(b)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0);
        ma.cmp(&mb).then_with(|| a.cmp(b))
    });

    let selected = zip_paths.pop()?;
    let file = match File::open(&selected) {
        Ok(file) => file,
        Err(err) => {
            log::warn!("Failed to open resource pack {:?}: {}", selected, err);
            return None;
        }
    };

    let mut archive = match ZipArchive::new(file) {
        Ok(archive) => archive,
        Err(err) => {
            log::warn!("Failed to read zip resource pack {:?}: {}", selected, err);
            return None;
        }
    };

    let mut entries = HashMap::new();
    for idx in 0..archive.len() {
        let mut entry = match archive.by_index(idx) {
            Ok(entry) => entry,
            Err(err) => {
                log::warn!("Failed to read zip entry #{} from {:?}: {}", idx, selected, err);
                continue;
            }
        };
        if entry.is_dir() {
            continue;
        }

        let key = normalize_pack_path(entry.name());
        if key.is_empty() {
            continue;
        }

        let mut bytes = Vec::new();
        match entry.read_to_end(&mut bytes) {
            Ok(_) => {
                entries.entry(key).or_insert(bytes);
            }
            Err(err) => {
                log::warn!(
                    "Failed to read zip entry '{}' from {:?}: {}",
                    entry.name(),
                    selected,
                    err
                );
            }
        }
    }

    if entries.is_empty() {
        log::warn!("Resource pack {:?} has no readable files", selected);
        return None;
    }

    log::info!("Loaded resource pack {:?}", selected);
    Some(ResourcePackData {
        source_path: selected,
        entries,
    })
}

fn find_resource_pack_zips() -> Vec<PathBuf> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let search_roots = [
        base.join("src").join("assets").join("packs"),
        base.join("src").join("assets").join("resourcepacks"),
        base.join("src").join("assets"),
    ];

    let mut out = Vec::new();
    for root in search_roots {
        let read_dir = match std::fs::read_dir(&root) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            if ext == "zip" {
                out.push(path);
            }
        }
    }
    out
}

fn normalize_pack_path(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches('/').to_ascii_lowercase()
}

fn has_supported_image_ext(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "avif")
}

fn texture_aliases(name: &str) -> Vec<&str> {
    match name {
        "workbench" => vec![
            "workbench_side",
            "workbench_front1",
            "workbench",
            "crafting_table_side",
            "planks",
            "wood",
        ],
        "workbench_top" => vec!["workbench_top", "crafting_table_top", "workbench"],
        "workbench_front" => vec![
            "workbench_front",
            "workbench1",
            "workbench_front1",
            "crafting_table_front",
            "workbench",
            "workbench_side",
        ],
        "furnace" => vec![
            "furnace",
            "furnace_side",
            "furnace_face",
            "furnace_front",
            "furnace_face_active",
            "stone",
        ],
        "furnace_top" => vec![
            "furnace_top",
            "furnace",
            "furnace_side",
            "stone",
        ],
        "furnace_front" => vec![
            "furnace_front",
            "furnace_face",
            "furnace_face_active",
            "furnace",
            "stone",
        ],
        "wood" => vec!["wood", "planks", "oak_planks"],
        "grass" => vec!["grass", "grass_side", "grass_block_side"],
        "grass_top" => vec!["grass_top", "grass_block_top"],
        "farmland_dry" => vec!["farmland_dry", "farmland", "dirt"],
        "farmland_wet" => vec!["farmland_wet", "farmland", "dirt"],
        "log" => vec!["log", "oak_log"],
        "logBottom" => vec!["logbottom", "log_bottom", "log_top_down", "oak_log_top", "oak_log"],
        "leaves" => vec!["leaves", "oak_leaves"],
        "coal_ore" => vec!["coal_ore", "coal_ore_stone"],
        "iron_ore" => vec!["iron_ore", "iron_ore_stone"],
        "copper_ore" => vec!["copper_ore", "copper_ore_stone"],
        "coal" => vec!["coal", "charcoal", "coal_item"],
        "torch" => vec!["torch", "wall_torch"],
        "iron_ingot" => vec!["iron_ingot", "iron", "iron_nugget"],
        _ => vec![name],
    }
}

fn find_pack_image_bytes(
    pack: &ResourcePackData,
    roots: &[&str],
    aliases: &[&str],
) -> Option<Vec<u8>> {
    for root in roots {
        let root = root.trim_matches('/').to_ascii_lowercase();
        for alias in aliases {
            let alias = alias.to_ascii_lowercase();

            for ext in ["png", "jpg", "jpeg", "webp", "avif"] {
                let direct = format!("{root}/{alias}.{ext}");
                if let Some(bytes) = pack.entries.get(&direct) {
                    return Some(bytes.clone());
                }
            }

            let prefix = format!("{root}/{alias}/");
            let mut matches: Vec<&str> = pack
                .entries
                .keys()
                .map(|k| k.as_str())
                .filter(|k| k.starts_with(&prefix) && has_supported_image_ext(k))
                .collect();
            matches.sort_unstable();
            if let Some(found) = matches.first() {
                if let Some(bytes) = pack.entries.get(*found) {
                    return Some(bytes.clone());
                }
            }

            let root_prefix = format!("{root}/");
            let mut deep_matches: Vec<&str> = pack
                .entries
                .keys()
                .map(|k| k.as_str())
                .filter(|k| k.starts_with(&root_prefix))
                .filter(|k| has_supported_image_ext(k))
                .filter(|k| {
                    Path::new(k)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(|stem| stem.eq_ignore_ascii_case(&alias))
                        .unwrap_or(false)
                })
                .collect();
            deep_matches.sort_unstable();
            if let Some(found) = deep_matches.first() {
                if let Some(bytes) = pack.entries.get(*found) {
                    return Some(bytes.clone());
                }
            }
        }
    }
    None
}

fn find_first_local_image(dir: &Path) -> Option<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            p.extension()
                .and_then(|s| s.to_str())
                .map(|s| has_supported_image_ext(&format!("x.{s}")))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    files.into_iter().next()
}

fn find_local_texture_path(roots: &[PathBuf], folders: &[&str], aliases: &[&str]) -> Option<PathBuf> {
    for root in roots {
        for folder in folders {
            let base = root.join(folder);
            if !base.exists() {
                continue;
            }
            for alias in aliases {
                for ext in ["png", "jpg", "jpeg", "webp", "avif"] {
                    let candidate = base.join(format!("{alias}.{ext}"));
                    if candidate.exists() {
                        return Some(candidate);
                    }
                }
                let dir = base.join(alias);
                if dir.is_dir() {
                    if let Some(path) = find_first_local_image(&dir) {
                        return Some(path);
                    }
                }
                // Keep alias priority for nested folders too:
                // try recursive search per alias in given order.
                if let Some(path) = find_local_texture_recursive_alias(&base, alias) {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn find_local_texture_recursive_alias(base: &Path, alias: &str) -> Option<PathBuf> {
    let mut stack = vec![base.to_path_buf()];
    let mut matches: Vec<PathBuf> = Vec::new();
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if !path.is_file() {
                continue;
            }
            let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
                continue;
            };
            if !matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp" | "avif") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if stem.eq_ignore_ascii_case(alias) {
                matches.push(path);
            }
        }
    }
    matches.sort();
    matches.into_iter().next()
}

fn asset_roots() -> [PathBuf; 2] {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    [
        base.join("src").join("assets"),
        base.join("assets"),
    ]
}

fn load_block_texture(name: &str, pack: Option<&ResourcePackData>) -> RgbaImage {
    let aliases = texture_aliases(name);

    if let Some(pack) = pack {
        let bytes = find_pack_image_bytes(pack, &["material"], &aliases)
            .or_else(|| find_pack_image_bytes(pack, &["minecraft/textures/block", "textures/block", "blocks"], &aliases))
            .or_else(|| find_pack_image_bytes(pack, &["minecraft/textures/item", "textures/item", "items"], &aliases));
        if let Some(bytes) = bytes {
            match image::load_from_memory(&bytes) {
                Ok(img) => return img.to_rgba8(),
                Err(err) => {
                    log::warn!(
                        "Failed to decode material texture '{}' from {:?}: {}",
                        name,
                        pack.source_path,
                        err
                    );
                }
            }
        }
    }

    let roots = asset_roots();
    if let Some(path) = find_local_texture_path(
        &roots,
        &[
            "blocks",
            "material",
            "minecraft/textures/block",
            "textures/block",
            "items",
            "minecraft/textures/item",
            "textures/item",
        ],
        &aliases,
    ) {
        match image::open(&path) {
            Ok(img) => return img.to_rgba8(),
            Err(err) => {
                // Some user textures are saved with a mismatched extension.
                // Fallback to content-based format detection.
                match std::fs::read(&path) {
                    Ok(bytes) => match image::load_from_memory(&bytes) {
                        Ok(img) => return img.to_rgba8(),
                        Err(mem_err) => {
                            log::warn!(
                                "Failed to decode {:?}: {} (content decode failed: {})",
                                path,
                                err,
                                mem_err
                            );
                        }
                    },
                    Err(read_err) => {
                        log::warn!("Failed to decode {:?}: {} (read failed: {})", path, err, read_err);
                    }
                }
            }
        }
    }

    log::warn!("Missing block texture '{}', using fallback color", name);
    fallback_block_texture(name)
}

fn load_parallax_texture(name: &str, pack: Option<&ResourcePackData>) -> Option<GrayImage> {
    let aliases = texture_aliases(name);

    if let Some(pack) = pack {
        if let Some(bytes) = find_pack_image_bytes(pack, &["parallax"], &aliases) {
            return match image::load_from_memory(&bytes) {
                Ok(img) => Some(img.to_luma8()),
                Err(err) => {
                    log::warn!(
                        "Failed to decode parallax map '{}' from {:?}: {}",
                        name,
                        pack.source_path,
                        err
                    );
                    None
                }
            };
        }
    }

    let roots = asset_roots();
    let path = find_local_texture_path(&roots, &["parallax"], &aliases)?;
    match image::open(&path) {
        Ok(img) => Some(img.to_luma8()),
        Err(_) => {
            let bytes = std::fs::read(&path).ok()?;
            image::load_from_memory(&bytes).ok().map(|img| img.to_luma8())
        }
    }
}

fn fallback_block_texture(name: &str) -> RgbaImage {
    let color = match name {
        "air" => [0, 0, 0, 0],
        "grass" => [72, 148, 46, 255],
        "dirt" => [122, 84, 46, 255],
        "farmland_dry" => [104, 72, 42, 255],
        "farmland_wet" => [78, 58, 36, 255],
        "grass_top" => [84, 156, 54, 255],
        "workbench" => [130, 94, 58, 255],
        "workbench_top" => [154, 120, 76, 255],
        "workbench_front" => [122, 88, 52, 255],
        "furnace" => [118, 118, 118, 255],
        "furnace_top" => [126, 126, 126, 255],
        "furnace_front" => [108, 108, 108, 255],
        "wood" => [166, 132, 89, 255],
        "stick" => [174, 142, 104, 255],
        "stone" => [120, 120, 120, 255],
        "sand" => [217, 204, 128, 255],
        "water" => [46, 107, 199, 255],
        "bedrock" => [38, 32, 32, 255],
        "log" => [115, 77, 46, 255],
        "logBottom" => [145, 112, 72, 255],
        "leaves" => [46, 140, 56, 255],
        "coal_ore" => [84, 84, 84, 255],
        "iron_ore" => [184, 135, 98, 255],
        "copper_ore" => [168, 100, 66, 255],
        "coal" => [48, 48, 48, 255],
        "torch" => [238, 186, 86, 255],
        "iron_ingot" => [206, 206, 214, 255],
        _ => [255, 0, 255, 255],
    };
    RgbaImage::from_pixel(16, 16, Rgba(color))
}


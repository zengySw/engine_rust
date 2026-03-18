use bytemuck::{Pod, Zeroable};
use glam::IVec3;

use crate::camera::Camera;
use crate::world::world::World;

const RT_VOLUME_W: u32 = 96;
const RT_VOLUME_H: u32 = 88;
const RT_VOLUME_D: u32 = 96;
const RT_VOLUME_SNAP_XZ: i32 = 8;
const RT_VOLUME_SNAP_Y: i32 = 4;
const RT_VOLUME_RELOCATE_THRESHOLD: i32 = 8;

const RT_VOLUME_W_I32: i32 = RT_VOLUME_W as i32;
const RT_VOLUME_D_I32: i32 = RT_VOLUME_D as i32;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RtUniform {
    inv_view_proj: [[f32; 4]; 4],
    cam_day: [f32; 4],    // xyz = camera world pos, w = day_time
    world_min: [f32; 4],  // xyz = voxel volume world origin
    world_size: [f32; 4], // xyz = voxel volume size
    settings: [f32; 4],   // x=max_primary_steps, y=max_shadow_steps, z=max_shadow_dist, w=exposure
    weather: [f32; 4],    // x=rain_strength, y=rain_time, z=surface_wetness
    lighting: [f32; 4],   // x=ambient_boost, y=sun_softness, z=fog_density, w=exposure
}

pub struct RayTracingRenderer {
    pipeline: wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    voxel_tex: wgpu::Texture,
    world_origin: IVec3,
    needs_volume_upload: bool,
    last_chunk_count: usize,
}

impl RayTracingRenderer {
    pub fn required_features() -> wgpu::Features {
        wgpu::Features::RAY_QUERY | wgpu::Features::RAY_TRACING_ACCELERATION_STRUCTURE
    }

    pub fn is_supported(adapter: &wgpu::Adapter) -> bool {
        let info = adapter.get_info();
        if info.backend != wgpu::Backend::Vulkan {
            return false;
        }

        let supported = adapter.features();
        supported.contains(Self::required_features())
    }

    #[allow(dead_code)]
    pub fn assert_supported(adapter: &wgpu::Adapter) {
        let info = adapter.get_info();
        if info.backend != wgpu::Backend::Vulkan {
            panic!(
                "Full ray tracing requires Vulkan backend. Current backend: {:?}",
                info.backend
            );
        }

        let supported = adapter.features();
        let required = Self::required_features();
        if !supported.contains(required) {
            let missing_query = !supported.contains(wgpu::Features::RAY_QUERY);
            let missing_as = !supported.contains(wgpu::Features::RAY_TRACING_ACCELERATION_STRUCTURE);
            panic!(
                "GPU does not support required RT features. Missing: RAY_QUERY={} RAY_TRACING_ACCELERATION_STRUCTURE={}",
                missing_query,
                missing_as
            );
        }
    }

    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        block_tex_view: &wgpu::TextureView,
        block_sampler: &wgpu::Sampler,
    ) -> Self {
        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rt_uniform"),
            size: std::mem::size_of::<RtUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let voxel_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rt_voxel_volume"),
            size: wgpu::Extent3d {
                width: RT_VOLUME_W,
                height: RT_VOLUME_H,
                depth_or_array_layers: RT_VOLUME_D,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let voxel_view = voxel_tex.create_view(&wgpu::TextureViewDescriptor {
            label: Some("rt_voxel_volume_view"),
            dimension: Some(wgpu::TextureViewDimension::D3),
            ..Default::default()
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rt_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rt_bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&voxel_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(block_tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(block_sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::include_wgsl!("raytracing.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rt_pipeline_layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rt_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        Self {
            pipeline,
            uniform_buf,
            bind_group,
            voxel_tex,
            world_origin: IVec3::new(i32::MIN / 2, i32::MIN / 2, i32::MIN / 2),
            needs_volume_upload: true,
            last_chunk_count: 0,
        }
    }

    pub fn update(
        &mut self,
        queue: &wgpu::Queue,
        cam: &Camera,
        width: u32,
        height: u32,
        day_time: f32,
        rain_strength: f32,
        rain_time: f32,
        surface_wetness: f32,
        lighting: [f32; 4],
        world: &World,
    ) {
        let center = IVec3::new(
            cam.pos.x.floor() as i32,
            cam.pos.y.floor() as i32,
            cam.pos.z.floor() as i32,
        );
        let raw_origin = IVec3::new(
            center.x - RT_VOLUME_W_I32 / 2,
            (center.y - (RT_VOLUME_H as i32 / 3)).max(0),
            center.z - RT_VOLUME_D_I32 / 2,
        );
        let target_origin = IVec3::new(
            snap_to_grid(raw_origin.x, RT_VOLUME_SNAP_XZ),
            snap_to_grid(raw_origin.y, RT_VOLUME_SNAP_Y).max(0),
            snap_to_grid(raw_origin.z, RT_VOLUME_SNAP_XZ),
        );

        let shift = (target_origin - self.world_origin).abs();
        if self.world_origin.x <= i32::MIN / 4 || shift.max_element() >= RT_VOLUME_RELOCATE_THRESHOLD {
            self.world_origin = target_origin;
            self.needs_volume_upload = true;
        }

        let chunk_count = world.chunk_count();
        if chunk_count != self.last_chunk_count {
            self.last_chunk_count = chunk_count;
            self.needs_volume_upload = true;
        }

        if self.needs_volume_upload {
            if chunk_count > 0 {
                self.upload_volume(queue, world);
                self.needs_volume_upload = false;
            }
        }

        let vp = cam.view_proj(width, height);
        let inv_vp = vp.inverse();
        let pixel_count = width.max(1) as f32 * height.max(1) as f32;
        let res_scale = (pixel_count / (1280.0 * 720.0)).sqrt().clamp(0.80, 2.20);
        let rain_penalty = 1.0 - rain_strength.clamp(0.0, 1.0) * 0.22;
        let max_primary_steps = (300.0 / res_scale * rain_penalty).clamp(120.0, 320.0);
        let max_shadow_steps = (40.0 / res_scale * rain_penalty).clamp(14.0, 48.0);
        let max_shadow_dist = (64.0 / res_scale).clamp(34.0, 72.0);
        let exposure = lighting[3].clamp(0.5, 2.5);

        let uniform = RtUniform {
            inv_view_proj: inv_vp.to_cols_array_2d(),
            cam_day: [cam.pos.x, cam.pos.y, cam.pos.z, day_time],
            world_min: [
                self.world_origin.x as f32,
                self.world_origin.y as f32,
                self.world_origin.z as f32,
                0.0,
            ],
            world_size: [RT_VOLUME_W as f32, RT_VOLUME_H as f32, RT_VOLUME_D as f32, 0.0],
            settings: [max_primary_steps, max_shadow_steps, max_shadow_dist, exposure],
            weather: [
                rain_strength.clamp(0.0, 1.0),
                rain_time.max(0.0),
                surface_wetness.clamp(0.0, 1.0),
                0.0,
            ],
            lighting: [
                lighting[0].clamp(0.5, 2.0),
                lighting[1].clamp(0.0, 1.0),
                lighting[2].clamp(0.1, 3.0),
                exposure,
            ],
        };
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&uniform));
    }

    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn upload_volume(&mut self, queue: &wgpu::Queue, world: &World) {
        let mut data = vec![0u8; (RT_VOLUME_W * RT_VOLUME_H * RT_VOLUME_D * 4) as usize];
        for z in 0..RT_VOLUME_D {
            let wz = self.world_origin.z + z as i32;
            for y in 0..RT_VOLUME_H {
                let wy = self.world_origin.y + y as i32;
                for x in 0..RT_VOLUME_W {
                    let wx = self.world_origin.x + x as i32;
                    let b = world.block_at_world(wx, wy, wz);
                    if !b.is_solid() {
                        continue;
                    }
                    let i = (((z * RT_VOLUME_H + y) * RT_VOLUME_W + x) * 4) as usize;
                    data[i] = (b.texture_index().min(255)) as u8;
                    data[i + 3] = 255;
                }
            }
        }

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.voxel_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * RT_VOLUME_W),
                rows_per_image: Some(RT_VOLUME_H),
            },
            wgpu::Extent3d {
                width: RT_VOLUME_W,
                height: RT_VOLUME_H,
                depth_or_array_layers: RT_VOLUME_D,
            },
        );
    }
}

#[inline]
fn snap_to_grid(v: i32, step: i32) -> i32 {
    if step <= 1 {
        return v;
    }
    v.div_euclid(step) * step
}

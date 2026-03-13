use bytemuck::{Pod, Zeroable};
use glam::IVec3;

use crate::camera::Camera;
use crate::world::world::World;

const RT_VOLUME_W: u32 = 128;
const RT_VOLUME_H: u32 = 96;
const RT_VOLUME_D: u32 = 128;

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
        world: &World,
    ) {
        let center = IVec3::new(
            cam.pos.x.floor() as i32,
            cam.pos.y.floor() as i32,
            cam.pos.z.floor() as i32,
        );
        let target_origin = IVec3::new(
            center.x - RT_VOLUME_W_I32 / 2,
            (center.y - 28).max(0),
            center.z - RT_VOLUME_D_I32 / 2,
        );

        let shift = (target_origin - self.world_origin).abs();
        if self.world_origin.x <= i32::MIN / 4 || shift.max_element() >= 2 {
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
            settings: [640.0, 96.0, 96.0, 1.0],
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

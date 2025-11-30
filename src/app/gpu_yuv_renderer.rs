// src/gpu_yuv_renderer.rs
//! GPU YUV420 -> RGB renderer using wgpu + egui_wgpu_backend.
//!
//! Usage (resumen):
//!  - crea GpuYuvRenderer en la inicialización con device/queue/target_format.
//!  - por cada frame YUV: call update_frame(queue, &video_frame).
//!  - obtener egui::TextureId con `egui_texture_id(&mut render_pass, device, queue)`
//!  - dibujar en UI con `ui.image(texture_id, size)`
//!
//! Nota: requiere las crates:
//!  - wgpu
//!  - egui_wgpu_backend (o egui_wgpu)
//!  - egui
//!   Ajusta nombres si tu versión de egui_wgpu_backend difiere ligeramente.

use crate::{
    log::log_sink::LogSink,
    media_agent::video_frame::{VideoFrame, VideoFrameData},
    sink_debug, sink_error,
};
use eframe::wgpu::{self, util::DeviceExt};
use std::sync::Arc;

pub struct GpuYuvRenderer {
    // pipeline / layout
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,

    // textures for Y, U, V
    tex_y: Option<wgpu::Texture>,
    tex_u: Option<wgpu::Texture>,
    tex_v: Option<wgpu::Texture>,

    // sampler and bindgroup
    sampler: wgpu::Sampler,
    bind_group: Option<wgpu::BindGroup>,

    // output (RGBA) texture where shader renders RGB result
    output_texture: Option<wgpu::Texture>,
    output_view: Option<wgpu::TextureView>,

    // quad vertex buffer (full screen)
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,

    // tracked sizes
    y_size: (u32, u32),
    uv_size: (u32, u32),

    // formats and config
    output_format: wgpu::TextureFormat,

    logger: Arc<dyn LogSink>,

    // dentro de GpuYuvRenderer
    u_info_buffer: wgpu::Buffer,
}

impl GpuYuvRenderer {
    /// Create a new renderer.
    /// `output_format` should be the swapchain/target format for egui backend; Rgba8UnormSrgb is fine.
    pub fn new(
        device: &wgpu::Device,
        output_format: wgpu::TextureFormat,
        logger: Arc<dyn LogSink>,
    ) -> Self {
        // simple full-screen triangle vertices (pos, uv)
        // we'll use two triangles as a quad (pos.xy, uv.xy)
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Vertex {
            pos: [f32; 2],
            uv: [f32; 2],
        }
        let vertices: &[Vertex] = &[
            // triangle 1
            Vertex {
                pos: [-1.0, -1.0],
                uv: [0.0, 1.0],
            },
            Vertex {
                pos: [3.0, -1.0],
                uv: [2.0, 1.0],
            }, // trick: full-screen triangle variant
            Vertex {
                pos: [-1.0, 3.0],
                uv: [0.0, -1.0],
            },
        ];

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("yuv-quad-vertex-buffer"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // sampler
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("yuv-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // bind group layout: Y(0), U(1), V(2), sampler(3)
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yuv-bind-group-layout"),
            entries: &[
                // Y texture
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                // U texture
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                // V texture
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                // sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        // WGSL shader: vertex passthrough + fragment YUV->RGB
        let shader_src = include_str!("shaders/yuv_to_rgb.wgsl");
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("yuv_to_rgb_shader"),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });

        // pipeline layout
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yuv-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        // create render pipeline
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yuv-render-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        // pos
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        // uv
                        wgpu::VertexAttribute {
                            offset: 8,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: output_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let u_info_data = [1.0f32, 1.0, 0.0, 0.0]; // default
        let u_info_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("yuv-u-info-buffer"),
            contents: bytemuck::cast_slice(&u_info_data),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        Self {
            pipeline,
            bind_group_layout,
            tex_y: None,
            tex_u: None,
            tex_v: None,
            sampler,
            bind_group: None,
            output_texture: None,
            output_view: None,
            vertex_buffer,
            vertex_count: 3,
            y_size: (0, 0),
            uv_size: (0, 0),
            output_format,
            logger,
            u_info_buffer,
        }
    }

    /// Update with a new YUV frame. This uploads Y/U/V planes to GPU, re-create textures if sizes changed,
    /// runs a render pass to the output texture, but DOES NOT register the texture with egui.
    /// After calling this, call `egui_texture_id(...)` to register the output texture for egui.
    #[allow(clippy::expect_used)]
    pub fn update_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &VideoFrame,
        logger: Arc<dyn LogSink>,
    ) {
        let (width, height, y_plane, u_plane, v_plane, y_stride, u_stride, v_stride) =
            match &frame.data {
                VideoFrameData::Yuv420 {
                    y,
                    u,
                    v,
                    y_stride,
                    u_stride,
                    v_stride,
                } => (
                    frame.width,
                    frame.height,
                    y.clone(),
                    u.clone(),
                    v.clone(),
                    *y_stride,
                    *u_stride,
                    *v_stride,
                ),
                _ => return,
            };

        // UV planes: size = ceil(width/2), ceil(height/2)
        let y_w = width;
        let y_h = height;
        let uv_w = (width as usize).div_ceil(2);
        let uv_h = (height as usize).div_ceil(2);

        // Re-create texturas si cambian tamaños
        if self.y_size != (y_w, y_h) || self.tex_y.is_none() {
            self.tex_y = Some(device.create_texture(&wgpu::TextureDescriptor {
                label: Some("y-plane"),
                size: wgpu::Extent3d {
                    width: y_w,
                    height: y_h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            }));
            self.y_size = (y_w, y_h);
        }

        if self.uv_size != (uv_w as u32, uv_h as u32)
            || self.tex_u.is_none()
            || self.tex_v.is_none()
        {
            let create_uv = |label: &str| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: uv_w as u32,
                        height: uv_h as u32,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                })
            };
            self.tex_u = Some(create_uv("u-plane"));
            self.tex_v = Some(create_uv("v-plane"));
            self.uv_size = (uv_w as u32, uv_h as u32);
        }

        // Output texture (RGBA) igual tamaño que Y
        if self.output_texture.is_none() || self.output_view.is_none() || self.y_size.0 != y_w {
            let out = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("yuv-rgb-output"),
                size: wgpu::Extent3d {
                    width: y_w,
                    height: y_h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.output_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.output_view = Some(out.create_view(&wgpu::TextureViewDescriptor::default()));
            self.output_texture = Some(out);
        }

        // Subida de texturas respetando stride
        let upload_plane = |tex: &wgpu::Texture, data: &[u8], w: usize, h: usize, stride: usize| {
            let bytes_per_row = aligned_bytes_per_row(stride);

            let expected_bytes = bytes_per_row as usize * h;
            if data.len() < expected_bytes {
                sink_error!(
                    logger,
                    "Data length {} < expected {} bytes. Adjusting rows!",
                    data.len(),
                    expected_bytes
                );
            }

            queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                data,
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(h as u32),
                },
                wgpu::Extent3d {
                    width: w as u32,
                    height: h as u32,
                    depth_or_array_layers: 1,
                },
            );
        };

        upload_plane(
            self.tex_y
                .as_ref()
                .expect("Y-plane texture not initialized"),
            &y_plane,
            y_w as usize,
            y_h as usize,
            y_stride,
        );
        upload_plane(
            self.tex_u
                .as_ref()
                .expect("U-plane texture not initialized"),
            &u_plane,
            uv_w,
            uv_h,
            u_stride,
        );
        upload_plane(
            self.tex_v
                .as_ref()
                .expect("V-plane texture not initialized"),
            &v_plane,
            uv_w,
            uv_h,
            v_stride,
        );
        // Bind group
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yuv-bind-group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &self
                            .tex_y
                            .as_ref()
                            .expect("Y texture missing for bind group")
                            .create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &self
                            .tex_u
                            .as_ref()
                            .expect("U texture missing for bind group")
                            .create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        &self
                            .tex_v
                            .as_ref()
                            .expect("V texture missing for bind group")
                            .create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.u_info_buffer.as_entire_binding(),
                },
            ],
        });
        self.bind_group = Some(bind_group);

        // Render pass
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("yuv-render-encoder"),
        });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("yuv-render-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: self
                        .output_view
                        .as_ref()
                        .expect("Output view not initialized"),
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            rpass.set_pipeline(&self.pipeline);
            rpass.set_bind_group(
                0,
                self.bind_group
                    .as_ref()
                    .expect("Bind group not initialized"),
                &[],
            );
            rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            rpass.draw(0..self.vertex_count, 0..1);
        }
        queue.submit(std::iter::once(encoder.finish()));
    }

    pub fn output_texture(&self) -> Option<&wgpu::Texture> {
        self.output_texture.as_ref()
    }
}

fn aligned_bytes_per_row(stride: usize) -> u32 {
    (stride as u32).div_ceil(256) * 256
}

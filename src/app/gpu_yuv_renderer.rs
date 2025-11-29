use eframe::wgpu::{self, PipelineCompilationOptions, util::DeviceExt};
use std::sync::Arc;
use crate::{
    app::log_sink::LogSink,
    media_agent::video_frame::{VideoFrame, VideoFrameData}, sink_debug, sink_error
};

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
    pub fn new(device: &wgpu::Device, output_format: wgpu::TextureFormat, logger: Arc<dyn LogSink>) -> Self {
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
            Vertex { pos: [-1.0, -1.0], uv: [0.0, 1.0] },
            Vertex { pos: [ 3.0, -1.0], uv: [2.0, 1.0] }, // trick: full-screen triangle variant
            Vertex { pos: [-1.0,  3.0], uv: [0.0, -1.0] },
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
                }
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
                buffers: &[
                    wgpu::VertexBufferLayout {
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
                    }
                ],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: output_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
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
    pub fn update_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &VideoFrame,
        logger: Arc<dyn LogSink>,
    ) {
        let (width, height, y_plane, u_plane, v_plane, y_stride, u_stride, v_stride) =
            match &frame.data {
                VideoFrameData::Yuv420 { y, u, v, y_stride, u_stride, v_stride } => {
                    (frame.width, frame.height, y.clone(), u.clone(), v.clone(), *y_stride, *u_stride, *v_stride)
                }
                _ => return,
            };

        // UV planes: size = ceil(width/2), ceil(height/2)
        let y_w = width;
        let y_h = height;
        let uv_w = width.div_ceil(2);
        let uv_h = height.div_ceil(2);

        if self.y_size != (y_w, y_h) || self.tex_y.is_none() {
            self.tex_y = Some(device.create_texture(&wgpu::TextureDescriptor {
                label: Some("y-plane"),
                size: wgpu::Extent3d { width: y_w, height: y_h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            }));
            self.y_size = (y_w, y_h);
        }

        if self.uv_size != (uv_w, uv_h) || self.tex_u.is_none() || self.tex_v.is_none() {
            let create_uv = |label: &str| device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: uv_w, height: uv_h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.tex_u = Some(create_uv("u-plane"));
            self.tex_v = Some(create_uv("v-plane"));
            self.uv_size = (uv_w, uv_h);
        }

        // Output texture (RGBA) Same size as Y
        if self.output_texture.is_none() || self.output_view.is_none() || self.y_size.0 != y_w {
            let out = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("yuv-rgb-output"),
                size: wgpu::Extent3d { width: y_w, height: y_h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.output_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            self.output_view = Some(out.create_view(&wgpu::TextureViewDescriptor::default()));
            self.output_texture = Some(out);
        }

        upload_plane(
            logger.clone(),
            self.tex_y.as_ref().unwrap(), 
            &y_plane, 
            y_w, 
            y_h, 
            y_stride,
            queue
            );
        upload_plane(
            logger.clone(),
            self.tex_u.as_ref().unwrap(), 
            &u_plane, 
            uv_w, 
            uv_h, 
            u_stride,
            queue
            );
        upload_plane(
            logger.clone(),
            self.tex_v.as_ref().unwrap(), 
            &v_plane, 
            uv_w, 
            uv_h, 
            v_stride,
            queue
            );

        // Bind group
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yuv-bind-group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&self.tex_y.as_ref().unwrap().create_view(&Default::default())) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.tex_u.as_ref().unwrap().create_view(&Default::default())) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.tex_v.as_ref().unwrap().create_view(&Default::default())) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 4, resource: self.u_info_buffer.as_entire_binding() },
            ],
        });
        self.bind_group = Some(bind_group);

        // Render pass
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("yuv-render-encoder") });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("yuv-render-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: self.output_view.as_ref().unwrap(),
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            rpass.set_pipeline(&self.pipeline);
            rpass.set_bind_group(0, self.bind_group.as_ref().unwrap(), &[]);
            rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            rpass.draw(0..self.vertex_count, 0..1);
        }
        queue.submit(std::iter::once(encoder.finish()));
    }

    pub fn output_texture(&self) -> Option<&wgpu::Texture> {
        self.output_texture.as_ref()
    }
}

fn upload_plane(
    logger: Arc<dyn LogSink>,
    tex: &wgpu::Texture, 
    data: &[u8], 
    width: u32, 
    height: u32, 
    stride: usize,
    queue: &wgpu::Queue
) {
    let unpadded_bytes_per_row = width as usize;
    let aligned_bytes_per_row = ((unpadded_bytes_per_row as u32 + 255) / 256) * 256;
    
    sink_debug!(
        logger, 
        "Upload plane - Width: {}, Height: {}, Stride: {}, Unpadded: {}, Aligned: {}, Data len: {}",
        width, height, stride, unpadded_bytes_per_row, aligned_bytes_per_row, data.len()
    );

    // No padding
    if stride == unpadded_bytes_per_row {
        let expected_size = (aligned_bytes_per_row * height) as usize;
        if data.len() < expected_size {
            sink_error!(logger, "Insufficient data: have {}, need {}", data.len(), expected_size);
            return;
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
                bytes_per_row: Some(aligned_bytes_per_row),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d { 
                width, 
                height, 
                depth_or_array_layers: 1 
            },
        );
    } else {
        let total_size = (aligned_bytes_per_row * height) as usize;
        let mut aligned_data = Vec::with_capacity(total_size);
        
        for row in 0..height as usize {
            let start = row * stride;
            let end = start + unpadded_bytes_per_row.min(data.len().saturating_sub(start));
            
            // Copiar datos válidos de esta fila
            if start < data.len() {
                let row_data = &data[start..end.min(data.len())];
                aligned_data.extend_from_slice(row_data);
            } else {
                // Si no hay datos, llenar con negro
                aligned_data.extend(std::iter::repeat(0).take(unpadded_bytes_per_row));
            }
            
            // Añadir padding de alineación para esta fila
            let bytes_in_current_row = aligned_data.len() % aligned_bytes_per_row as usize;
            if bytes_in_current_row > 0 {
                let padding_needed = aligned_bytes_per_row as usize - bytes_in_current_row;
                aligned_data.extend(std::iter::repeat(0).take(padding_needed));
            }
        }
        
        // Asegurar que el tamaño final es exactamente el esperado
        if aligned_data.len() != total_size {
            sink_debug!(logger, "Resizing aligned data from {} to {}", aligned_data.len(), total_size);
            aligned_data.resize(total_size, 0);
        }
        
        sink_debug!(logger, "Final aligned data size: {}", aligned_data.len());
        
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &aligned_data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(aligned_bytes_per_row),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d { 
                width, 
                height, 
                depth_or_array_layers: 1 
            },
        );
    }
}

//! GPU-accelerated YUV420 to RGB renderer using `wgpu`.
//!
//! This module provides a `GpuYuvRenderer` that handles the GPU-side conversion
//! of YUV420 planar video frames into a final RGB texture that can be displayed
//! using `egui` and `egui_wgpu`.
//!
//! # Usage
//!
//! 1. Create a `GpuYuvRenderer` instance during initialization, providing the `wgpu::Device`
//!    and the target texture format.
//! 2. For each new `VideoFrame`, call `update_frame()` to upload the Y, U, and V planes
//!    to the GPU and run the conversion shader.
//! 3. The resulting RGB texture can be obtained via `output_texture()` and then
//!    registered with `egui` to be displayed in the UI.
//!
use crate::{
    log::log_sink::LogSink,
    media_agent::video_frame::{VideoFrame, VideoFrameData},
    sink_debug,
};
use eframe::wgpu::{self, util::DeviceExt, PipelineCompilationOptions};
use std::sync::Arc;

/// Manages GPU resources and the rendering pipeline for YUV-to-RGB conversion.
pub struct GpuYuvRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,

    // Textures for Y, U, V planes
    tex_y: Option<wgpu::Texture>,
    tex_u: Option<wgpu::Texture>,
    tex_v: Option<wgpu::Texture>,

    sampler: wgpu::Sampler,
    bind_group: Option<wgpu::BindGroup>,

    /// The final RGBA texture where the shader renders the conversion result.
    output_texture: Option<wgpu::Texture>,
    output_view: Option<wgpu::TextureView>,

    /// Vertex buffer for drawing a full-screen quad.
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,

    // Tracked texture sizes to detect dimension changes.
    y_size: (u32, u32),
    uv_size: (u32, u32),

    output_format: wgpu::TextureFormat,

    #[allow(dead_code)]
    logger: Arc<dyn LogSink>,

    /// Uniform buffer for passing stride information to the shader.
    u_info_buffer: wgpu::Buffer,
}

impl GpuYuvRenderer {
    /// Creates a new renderer instance.
    ///
    /// `output_format` should match the swapchain/target format for the `egui` backend.
    pub fn new(
        device: &wgpu::Device,
        output_format: wgpu::TextureFormat,
        logger: Arc<dyn LogSink>,
    ) -> Self {
        // A full-screen triangle is used to render a quad. (pos.xy, uv.xy)
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Vertex {
            pos: [f32; 2],
            uv: [f32; 2],
        }
        let vertices: &[Vertex] = &[
            Vertex {
                pos: [-1.0, -1.0],
                uv: [0.0, 1.0],
            },
            Vertex {
                pos: [3.0, -1.0],
                uv: [2.0, 1.0],
            },
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

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("yuv-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

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
                // Sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // Uniform info buffer
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

        let shader_src = include_str!("shaders/yuv_to_rgb.wgsl");
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("yuv_to_rgb_shader"),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yuv-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

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

    /// Updates the renderer with a new YUV video frame.
    ///
    /// This method uploads the Y, U, and V planes to their respective GPU textures,
    /// re-creating them if the frame dimensions have changed. It then executes a
    /// render pass to convert the YUV data to RGB, storing the result in an
    /// internal output texture.
    ///
    /// To display the result, obtain the output texture via `output_texture()` and
    /// register it with `egui`.
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

        let y_w = width;
        let y_h = height;
        let uv_w = (width).div_ceil(2);
        let uv_h = (height).div_ceil(2);

        // Recreate textures if frame dimensions have changed
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

        if self.uv_size != (uv_w, uv_h) || self.tex_u.is_none() || self.tex_v.is_none() {
            let create_uv = |label: &str| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: uv_w,
                        height: uv_h,
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
            self.uv_size = (uv_w, uv_h);
        }

        // Recreate output texture if size has changed
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

        // Upload plane data to GPU textures
        upload_plane(
            logger.clone(),
            self.tex_y.as_ref().expect("Y-plane texture missing"),
            &y_plane,
            y_w,
            y_h,
            y_stride,
            queue,
        );
        upload_plane(
            logger.clone(),
            self.tex_u.as_ref().expect("U-plane texture missing"),
            &u_plane,
            uv_w,
            uv_h,
            u_stride,
            queue,
        );
        upload_plane(
            logger,
            self.tex_v.as_ref().expect("V-plane texture missing"),
            &v_plane,
            uv_w,
            uv_h,
            v_stride,
            queue,
        );

        // Create the bind group for the shader
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

        // Execute the render pass to perform the YUV-to-RGB conversion
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

    /// Returns a reference to the final RGB output texture.
    ///
    /// This texture contains the result of the YUV-to-RGB conversion and can be
    /// registered with `egui` for rendering. Returns `None` if `update_frame` has
    /// not yet been called.
    pub fn output_texture(&self) -> Option<&wgpu::Texture> {
        self.output_texture.as_ref()
    }
}

/// Uploads a single Y, U, or V plane to a `wgpu` texture, handling stride alignment.
///
/// `wgpu` requires that the `bytes_per_row` in `write_texture` be a multiple of 256.
/// Video decoders often produce frames with a different stride (bytes per row).
///
/// This function checks if the source `stride` matches the required alignment. If not,
/// it creates a temporary, correctly aligned buffer and copies the image data into it
/// row-by-row before uploading to the GPU. This avoids an expensive copy when the
/// strides already match.
fn upload_plane(
    logger: Arc<dyn LogSink>,
    tex: &wgpu::Texture,
    data: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    queue: &wgpu::Queue,
) {
    let w = width as usize;
    let h = height as usize;

    // wgpu requires texture row alignment to be a multiple of 256 bytes.
    let aligned_bpr = ((w as u32).div_ceil(256)) * 256;

    sink_debug!(
        logger,
        "[UPLOAD] w={} h={} stride={} aligned_bpr={}",
        w,
        h,
        stride,
        aligned_bpr
    );

    // If the source stride happens to match the required alignment, we can copy directly.
    // This avoids an expensive allocation and copy for every frame plane.
    if stride as u32 == aligned_bpr {
        queue.write_texture(
            tex.as_image_copy(),
            data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(stride as u32),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        return;
    }

    // The stride from the decoder is not 256-byte aligned. We must copy the data
    // row-by-row into a temporary, correctly aligned buffer before uploading to the GPU.
    let mut aligned_data = Vec::with_capacity((aligned_bpr * height) as usize);
    for row_idx in 0..h {
        let src_start = row_idx * stride;
        let src_end = src_start + w;

        if src_end > data.len() {
            // Source data is smaller than expected for this row.
            // Avoid panicking by padding the rest of the row with black.
            let valid_len = data.len().saturating_sub(src_start).max(0);
            if valid_len > 0 {
                aligned_data.extend_from_slice(&data[src_start..src_start + valid_len]);
            }
            aligned_data.extend(std::iter::repeat_n(0, w - valid_len));
        } else {
            // Copy the actual pixel data for the row.
            aligned_data.extend_from_slice(&data[src_start..src_end]);
        }

        // Add padding to the end of the row to meet wgpu's alignment requirement.
        let padding = aligned_bpr as usize - w;
        aligned_data.extend(std::iter::repeat_n(0, padding));
    }

    queue.write_texture(
        tex.as_image_copy(),
        &aligned_data,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(aligned_bpr),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

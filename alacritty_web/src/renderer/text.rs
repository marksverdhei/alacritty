//! Text rendering pipeline using wgpu instanced rendering.
//!
//! Each terminal cell is rendered as a textured quad via instancing.
//! Two passes: background quads first, then text quads with alpha blending.

use wgpu::util::DeviceExt;

use super::glyph_cache::GlyphCache;

/// Per-instance data for a single terminal cell.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CellInstance {
    /// Grid position (col, row) in cell units.
    pub grid_col: f32,
    pub grid_row: f32,
    /// UV coordinates in the glyph atlas (x, y, w, h) normalized.
    pub uv_x: f32,
    pub uv_y: f32,
    pub uv_w: f32,
    pub uv_h: f32,
    /// Foreground color (RGBA).
    pub fg_r: f32,
    pub fg_g: f32,
    pub fg_b: f32,
    pub fg_a: f32,
    /// Background color (RGBA).
    pub bg_r: f32,
    pub bg_g: f32,
    pub bg_b: f32,
    pub bg_a: f32,
}

/// Uniforms for the text shader.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TextUniforms {
    /// Orthographic projection matrix (column-major).
    pub projection: [f32; 16],
    /// Cell dimensions in pixels.
    pub cell_width: f32,
    pub cell_height: f32,
    /// Pass type: 0 = background, 1 = text.
    pub pass_type: u32,
    pub _padding: u32,
}

const TEXT_SHADER: &str = r#"
struct TextUniforms {
    projection: mat4x4<f32>,
    cell_width: f32,
    cell_height: f32,
    pass_type: u32,
    _padding: u32,
}

struct CellInstance {
    @location(0) grid_pos: vec2<f32>,
    @location(1) uv_rect: vec4<f32>,
    @location(2) fg_color: vec4<f32>,
    @location(3) bg_color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) fg_color: vec4<f32>,
    @location(2) bg_color: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> uniforms: TextUniforms;

@group(0) @binding(1)
var atlas_texture: texture_2d<f32>;

@group(0) @binding(2)
var atlas_sampler: sampler;

// Quad vertices: two triangles forming a unit quad.
var<private> QUAD_VERTICES: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(1.0, 1.0),
    vec2<f32>(0.0, 1.0),
);

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_index: u32,
    instance: CellInstance,
) -> VertexOutput {
    var out: VertexOutput;

    let quad_pos = QUAD_VERTICES[vertex_index];

    // Cell position in pixels.
    let pixel_pos = vec2<f32>(
        instance.grid_pos.x * uniforms.cell_width,
        instance.grid_pos.y * uniforms.cell_height,
    );

    // Scale quad to cell size and offset.
    let pos = pixel_pos + quad_pos * vec2<f32>(uniforms.cell_width, uniforms.cell_height);

    out.position = uniforms.projection * vec4<f32>(pos, 0.0, 1.0);

    // UV mapping into the atlas.
    out.uv = instance.uv_rect.xy + quad_pos * instance.uv_rect.zw;

    out.fg_color = instance.fg_color;
    out.bg_color = instance.bg_color;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    if uniforms.pass_type == 0u {
        // Background pass: solid color.
        if in.bg_color.a < 0.001 {
            discard;
        }
        return in.bg_color;
    } else {
        // Text pass: sample glyph alpha from atlas, apply foreground color.
        let glyph_alpha = textureSample(atlas_texture, atlas_sampler, in.uv).r;
        if glyph_alpha < 0.01 {
            discard;
        }
        return vec4<f32>(in.fg_color.rgb, in.fg_color.a * glyph_alpha);
    }
}
"#;

/// Text renderer using instanced quad rendering.
pub struct TextRenderer {
    pipeline_bg: wgpu::RenderPipeline,
    pipeline_text: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: Option<wgpu::BindGroup>,
    uniform_buffer: wgpu::Buffer,
    instance_buffer: Option<wgpu::Buffer>,
    instance_count: u32,
}

impl TextRenderer {
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("text_shader"),
            source: wgpu::ShaderSource::Wgsl(TEXT_SHADER.into()),
        });

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("text_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("text_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<CellInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 40,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        };

        let pipeline_bg = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("text_pipeline_bg"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[instance_layout.clone()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let pipeline_text = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("text_pipeline_text"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[instance_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("text_uniform_buffer"),
            contents: bytemuck::bytes_of(&TextUniforms {
                projection: orthographic_projection(800.0, 600.0),
                cell_width: 8.0,
                cell_height: 16.0,
                pass_type: 0,
                _padding: 0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        Self {
            pipeline_bg,
            pipeline_text,
            bind_group_layout,
            bind_group: None,
            uniform_buffer,
            instance_buffer: None,
            instance_count: 0,
        }
    }

    /// Update instance buffer and bind group from cell data.
    pub fn update_instances(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[CellInstance],
        cell_width: f32,
        cell_height: f32,
        viewport_width: f32,
        viewport_height: f32,
        glyph_cache: Option<&GlyphCache>,
    ) {
        self.instance_count = instances.len() as u32;

        // Update uniforms.
        let uniforms = TextUniforms {
            projection: orthographic_projection(viewport_width, viewport_height),
            cell_width,
            cell_height,
            pass_type: 0, // Updated per-pass in draw.
            _padding: 0,
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        if instances.is_empty() {
            self.instance_buffer = None;
            self.bind_group = None;
            return;
        }

        self.instance_buffer = Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("text_instance_buffer"),
            contents: bytemuck::cast_slice(instances),
            usage: wgpu::BufferUsages::VERTEX,
        }));

        // Create bind group with atlas texture.
        if let Some(cache) = glyph_cache {
            self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("text_bind_group"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(cache.atlas_view()),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(cache.atlas_sampler()),
                    },
                ],
            }));
        }
    }

    /// Draw text (called within a render pass).
    pub fn draw<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>) {
        if self.instance_count == 0 {
            return;
        }

        let (Some(buf), Some(bind_group)) = (&self.instance_buffer, &self.bind_group) else {
            return;
        };

        // Background pass.
        render_pass.set_pipeline(&self.pipeline_bg);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.set_vertex_buffer(0, buf.slice(..));
        render_pass.draw(0..6, 0..self.instance_count);

        // Text pass.
        render_pass.set_pipeline(&self.pipeline_text);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.set_vertex_buffer(0, buf.slice(..));
        render_pass.draw(0..6, 0..self.instance_count);
    }
}

/// Create an orthographic projection matrix (column-major).
pub fn orthographic_projection(width: f32, height: f32) -> [f32; 16] {
    [
        2.0 / width,
        0.0,
        0.0,
        0.0,
        0.0,
        -2.0 / height,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        -1.0,
        1.0,
        0.0,
        1.0,
    ]
}

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use wgpu::ShaderStages;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]

pub struct Vertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

impl Vertex {
    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ScreenUniform {
    pub size: [f32; 2],
}

#[derive(Clone)]
pub struct DrawCmd {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: [f32; 4],
    pub texture_id: u32,
    pub uv: [f32; 4], // [tcLeft, tcTop, tcRight, tcBottom]
    pub clip: Option<[u32; 4]>,
}

#[derive(Clone)]
pub struct DrawQuadCmd {
    pub texture_id: u32,
    pub color: [f32; 4],
    pub clip: Option<[u32; 4]>,
    pub positions: [[f32; 2]; 4],
    pub uvs: [[f32; 2]; 4],
}

pub enum DrawItem {
    Rect(DrawCmd),
    Quad(DrawQuadCmd),
    Text(TextCmd),
}

pub type DrawQueue = Arc<Mutex<Vec<DrawItem>>>;

pub type CursorPos = Arc<Mutex<[f32; 2]>>;

#[derive(Clone)]
pub struct TextureUploadCmd {
    pub id: u32,
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub type TextureUploadQueue = Arc<Mutex<Vec<TextureUploadCmd>>>;

pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    screen_bind_group: wgpu::BindGroup,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    textures: HashMap<u32, wgpu::BindGroup>,
    byte_offset: u64,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, queue: &wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: std::mem::size_of::<ScreenUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let screen_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let screen_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &screen_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&screen_bind_group_layout, &texture_bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Vertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (std::mem::size_of::<Vertex>() * 131072) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let white_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            white_texture.as_image_copy(),
            &[255u8, 255, 255, 255],
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );

        let white_view = white_texture.create_view(&Default::default());
        let white_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&white_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let mut textures = HashMap::new();
        textures.insert(0u32, white_bind_group);

        Self {
            pipeline,
            vertex_buffer,
            uniform_buffer,
            screen_bind_group,
            texture_bind_group_layout,
            sampler,
            textures,
            byte_offset: 0,
        }
    }

    pub fn begin_frame(&mut self) {
        self.byte_offset = 0;
    }

    pub fn load_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: u32,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: width,
                height: height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            texture.as_image_copy(),
            rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let view = texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        self.textures.insert(id, bind_group);
    }

    pub fn draw<'a>(
        &'a mut self,
        pass: &mut wgpu::RenderPass<'a>,
        queue: &wgpu::Queue,
        screen_size: (u32, u32),
        cmds: &[DrawItem],
    ) {
        let uniform = ScreenUniform {
            size: [screen_size.0 as f32, screen_size.1 as f32],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniform]));

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.screen_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));

        let tid_of = |item: &DrawItem| match item {
            DrawItem::Rect(c) => c.texture_id,
            DrawItem::Quad(c) => c.texture_id,
            DrawItem::Text(_) => 0u32,
        };

        let clip_of = |item: &DrawItem| match item {
            DrawItem::Rect(c) => c.clip,
            DrawItem::Quad(c) => c.clip,
            DrawItem::Text(_) => None,
        };

        // batch by texture_id
        let vertex_size = std::mem::size_of::<Vertex>() as u64;
        let mut i = 0;
        while i < cmds.len() {
            let tid = tid_of(&cmds[i]);
            let start = i;
            while i < cmds.len()
                && tid_of(&cmds[i]) == tid
                && clip_of(&cmds[i]) == clip_of(&cmds[start])
            {
                i += 1;
            }
            let mut vertices: Vec<Vertex> = Vec::new();
            for item in &cmds[start..i] {
                match item {
                    DrawItem::Rect(cmd) => {
                        let x2 = cmd.x + cmd.w;
                        let y2 = cmd.y + cmd.h;
                        let tl = Vertex {
                            position: [cmd.x, cmd.y],
                            uv: [cmd.uv[0], cmd.uv[1]],
                            color: cmd.color,
                        };
                        let tr = Vertex {
                            position: [x2, cmd.y],
                            uv: [cmd.uv[2], cmd.uv[1]],
                            color: cmd.color,
                        };
                        let bl = Vertex {
                            position: [cmd.x, y2],
                            uv: [cmd.uv[0], cmd.uv[3]],
                            color: cmd.color,
                        };
                        let br = Vertex {
                            position: [x2, y2],
                            uv: [cmd.uv[2], cmd.uv[3]],
                            color: cmd.color,
                        };

                        // triangle 1
                        vertices.push(tl);
                        vertices.push(tr);
                        vertices.push(bl);
                        // triangle 2
                        vertices.push(tr);
                        vertices.push(br);
                        vertices.push(bl);
                    }
                    DrawItem::Quad(cmd) => {
                        let [p1, p2, p3, p4] = cmd.positions;
                        let [uv1, uv2, uv3, uv4] = cmd.uvs;
                        let v = |p: [f32; 2], uv: [f32; 2]| Vertex {
                            position: p,
                            uv,
                            color: cmd.color,
                        };
                        vertices.extend_from_slice(&[
                            v(p1, uv1),
                            v(p2, uv2),
                            v(p3, uv3),
                            v(p1, uv1),
                            v(p3, uv3),
                            v(p4, uv4),
                        ]);
                    }
                    DrawItem::Text(_) => continue,
                }
            }

            let bg = self
                .textures
                .get(&tid)
                .unwrap_or_else(|| self.textures.get(&0).unwrap());
            match clip_of(&cmds[start]) {
                Some([cx, cy, cw, ch]) => {
                    pass.set_scissor_rect(cx, cy, cw.max(1), ch.max(1));
                }
                None => {
                    pass.set_scissor_rect(0, 0, screen_size.0, screen_size.1);
                }
            }
            pass.set_bind_group(1, bg, &[]);
            if vertices.is_empty() {
                continue;
            }
            let buffer_cap = self.vertex_buffer.size();
            if self.byte_offset + vertices.len() as u64 * vertex_size > buffer_cap {
                break;
            }
            queue.write_buffer(
                &self.vertex_buffer,
                self.byte_offset,
                bytemuck::cast_slice(&vertices),
            );
            let vert_start = (self.byte_offset / vertex_size) as u32;
            let vert_end = vert_start + vertices.len() as u32;
            pass.draw(vert_start..vert_end, 0..1);
            self.byte_offset += vertices.len() as u64 * vertex_size;
        }
    }
}

#[derive(Clone)]
pub struct TextCmd {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub text: String,
    pub color: [f32; 4],
    pub align: String,
    pub font: String,
    pub clip: Option<[u32; 4]>,
}

pub type TextQueue = Arc<Mutex<Vec<TextCmd>>>;

pub struct TextRenderer {
    font_system: glyphon::FontSystem,
    swash_cache: glyphon::SwashCache,
    atlas: glyphon::TextAtlas,
    renderer: glyphon::TextRenderer,
}

impl TextRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let font_system = glyphon::FontSystem::new();
        let swash_cache = glyphon::SwashCache::new();
        let mut atlas = glyphon::TextAtlas::new(device, queue, format);
        let renderer =
            glyphon::TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);

        Self {
            font_system,
            swash_cache,
            atlas,
            renderer,
        }
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen_size: (u32, u32),
        cmds: &[TextCmd],
    ) -> Result<(), glyphon::PrepareError> {
        let mut text_areas: Vec<glyphon::TextArea> = Vec::new();
        let mut buffers: Vec<glyphon::Buffer> = Vec::new();
        for cmd in cmds {
            let mut buffer = glyphon::Buffer::new(
                &mut self.font_system,
                glyphon::Metrics::new(cmd.size, cmd.size * 1.2),
            );
            buffer.set_size(
                &mut self.font_system,
                screen_size.0 as f32,
                screen_size.1 as f32,
            );

            let attrs = match cmd.font.as_str() {
                "FIXED" => glyphon::Attrs::new().family(glyphon::Family::Monospace),
                _ => glyphon::Attrs::new().family(glyphon::Family::SansSerif),
            };

            let spans = parse_color_spans(&cmd.text, cmd.color);
            let rich: Vec<(&str, glyphon::Attrs)> = spans
                .iter()
                .map(|(s, c)| {
                    let gc = glyphon::Color::rgba(
                        (c[0] * 255.0) as u8,
                        (c[1] * 255.0) as u8,
                        (c[2] * 255.0) as u8,
                        (c[3] * 255.0) as u8,
                    );
                    (*s, attrs.color(gc))
                })
                .collect();

            buffer.set_rich_text(&mut self.font_system, rich, glyphon::Shaping::Basic);
            buffer.shape_until_scroll(&mut self.font_system);
            buffers.push(buffer);
        }

        for (i, cmd) in cmds.iter().enumerate() {
            let cmd_color = glyphon::Color::rgba(
                (cmd.color[0] * 255.0) as u8,
                (cmd.color[1] * 255.0) as u8,
                (cmd.color[2] * 255.0) as u8,
                (cmd.color[3] * 255.0) as u8,
            );
            let line_w = buffers[i]
                .layout_runs()
                .map(|r| r.line_w)
                .fold(0.0f32, f32::max);
            let left = match cmd.align.as_str() {
                "RIGHT_X" => cmd.x - line_w,
                "CENTER_X" => cmd.x - line_w / 2.0,
                _ => cmd.x,
            };
            let bounds = match cmd.clip {
                Some([cx, cy, cw, ch]) => glyphon::TextBounds {
                    left: cx as i32,
                    top: cy as i32,
                    right: (cx + cw) as i32,
                    bottom: (cy + ch) as i32,
                },
                None => glyphon::TextBounds {
                    left: 0,
                    top: 0,
                    right: screen_size.0 as i32,
                    bottom: screen_size.1 as i32,
                },
            };
            text_areas.push(glyphon::TextArea {
                buffer: &buffers[i],
                left: left,
                top: cmd.y,
                scale: 1.0,
                bounds,
                default_color: cmd_color,
            })
        }

        self.renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            glyphon::Resolution {
                width: screen_size.0,
                height: screen_size.1,
            },
            text_areas,
            &mut self.swash_cache,
        )?;
        Ok(())
    }

    pub fn render<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
    ) -> Result<(), glyphon::RenderError> {
        self.renderer.render(&self.atlas, pass)?;
        Ok(())
    }
}

fn parse_color_spans<'a>(text: &'a str, default_color: [f32; 4]) -> Vec<(&'a str, [f32; 4])> {
    let alpha = default_color[3];
    let mut spans: Vec<(&'a str, [f32; 4])> = Vec::new();
    let mut color = default_color;
    let mut start = 0;

    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'^' {
            i += 1;
            continue;
        }

        if i > start {
            spans.push((&text[start..i], color));
        }
        i += 1;
        if i >= bytes.len() {
            start = i;
            break;
        }

        if (bytes[i] == b'X' || bytes[i] == b'x') && i + 7 <= bytes.len() {
            // ^xRRGGBB
            if let Ok(hex) = u32::from_str_radix(&text[i + 1..i + 7], 16) {
                color = [
                    ((hex >> 16) & 0xFF) as f32 / 255.0,
                    ((hex >> 8) & 0xFF) as f32 / 255.0,
                    ((hex) & 0xFF) as f32 / 255.0,
                    alpha,
                ];
            }
            i += 7;
        } else if bytes[i].is_ascii_digit() {
            color = pob_digit_color(bytes[i] - b'0', alpha);
            i += 1;
        }
        start = i;
    }
    if start < text.len() {
        spans.push((&text[start..], color));
    }

    spans
}

fn pob_digit_color(digit: u8, alpha: f32) -> [f32; 4] {
    let (r, g, b): (f32, f32, f32) = match digit {
        0 => (0.0, 0.0, 0.0),    // black
        1 => (1.0, 0.0, 0.0),    // red
        2 => (0.0, 1.0, 0.0),    // green
        3 => (0.0, 0.0, 1.0),    // blue
        4 => (1.0, 1.0, 0.0),    // yellow
        5 => (0.5, 0.5, 0.5),    // gray
        6 => (0.5, 0.5, 0.5),    // gray
        7 => (1.0, 1.0, 1.0),    // white
        8 => (0.75, 0.75, 0.75), // light gray
        9 => (0.3, 0.3, 0.3),    // dark gray
        _ => (1.0, 1.0, 1.0),
    };
    [r, g, b, alpha]
}

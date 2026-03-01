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

pub type DrawQueue = Arc<Mutex<Vec<DrawCmd>>>;

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
        }
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
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        queue: &wgpu::Queue,
        screen_size: (u32, u32),
        cmds: &[DrawCmd],
    ) {
        let uniform = ScreenUniform {
            size: [screen_size.0 as f32, screen_size.1 as f32],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniform]));

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.screen_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));

        // batch by texture_id
        let mut byte_offset: u64 = 0;
        let vertex_size = std::mem::size_of::<Vertex>() as u64;
        let mut i = 0;
        while i < cmds.len() {
            let tid = cmds[i].texture_id;
            let start = i;
            while i < cmds.len() && cmds[i].texture_id == tid && cmds[i].clip == cmds[start].clip {
                i += 1;
            }
            let mut vertices: Vec<Vertex> = Vec::new();
            for cmd in &cmds[start..i] {
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

            let bg = self
                .textures
                .get(&tid)
                .unwrap_or_else(|| self.textures.get(&0).unwrap());
            match cmds[start].clip {
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
            if byte_offset + vertices.len() as u64 * vertex_size > buffer_cap {
                break;
            }
            queue.write_buffer(
                &self.vertex_buffer,
                byte_offset,
                bytemuck::cast_slice(&vertices),
            );
            let vert_start = (byte_offset / vertex_size) as u32;
            let vert_end = vert_start + vertices.len() as u32;
            pass.draw(vert_start..vert_end, 0..1);
            byte_offset += vertices.len() as u64 * vertex_size;
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

            buffer.set_text(
                &mut self.font_system,
                &cmd.text,
                glyphon::Attrs::new(),
                glyphon::Shaping::Basic,
            );
            buffers.push(buffer);
        }

        for (i, cmd) in cmds.iter().enumerate() {
            let cmd_color = glyphon::Color::rgba(
                (cmd.color[0] * 255.0) as u8,
                (cmd.color[1] * 255.0) as u8,
                (cmd.color[2] * 255.0) as u8,
                (cmd.color[3] * 255.0) as u8,
            );
            text_areas.push(glyphon::TextArea {
                buffer: &buffers[i],
                left: cmd.x,
                top: cmd.y,
                scale: 1.0,
                bounds: glyphon::TextBounds {
                    left: 0,
                    top: 0,
                    right: screen_size.0 as i32,
                    bottom: screen_size.1 as i32,
                },
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

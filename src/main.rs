mod graphics;
mod lua_host;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::graphics::{CursorPos, DrawQueue, TextQueue, Vertex};
use crate::lua_host::LuaHost;

use mlua::prelude::{LuaMultiValue, LuaValue};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::EventLoop;
use winit::window::Window;

struct GfxState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: graphics::Renderer,
    text_renderer: graphics::TextRenderer,
}

struct App {
    window: Option<Arc<Window>>,
    gfx: Option<GfxState>,
    host: LuaHost,
    draw_queue: DrawQueue,
    text_queue: TextQueue,
    cursor_pos: CursorPos,
    pressed_keys: Arc<Mutex<HashSet<String>>>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Path Of Building")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
                )
                .unwrap(),
        );
        self.window = Some(window.clone());
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        println!("instance created");

        let surface = instance.create_surface(window.clone()).unwrap();
        println!("surface created");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("no adapter found");
        println!("adapter: {}", adapter.get_info().name);

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
            },
            None,
        ))
        .expect("failed to create device");
        println!("device created");

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        println!("format: {:?}", format);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };

        surface.configure(&device, &config);
        let renderer = graphics::Renderer::new(&device, format);
        let text_renderer = graphics::TextRenderer::new(&device, &queue, format);
        self.gfx = Some(GfxState {
            surface,
            device,
            queue,
            config,
            renderer,
            text_renderer,
        })
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(new_size) => {
                if let Some(g) = &mut self.gfx {
                    g.config.width = new_size.width.max(1);
                    g.config.height = new_size.height.max(1);
                    g.surface.configure(&g.device, &g.config);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                *self.cursor_pos.lock().unwrap() = [position.x as f32, position.y as f32];
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let btn = match button {
                    winit::event::MouseButton::Left => 1i64,
                    winit::event::MouseButton::Right => 2i64,
                    winit::event::MouseButton::Middle => 3i64,
                    _ => return,
                };

                match state {
                    winit::event::ElementState::Pressed => {
                        self.host
                            .callback_args(
                                "OnMouseDown",
                                LuaMultiValue::from_vec(vec![
                                    LuaValue::Integer(btn),
                                    LuaValue::Boolean(false),
                                ]),
                            )
                            .unwrap();
                    }
                    winit::event::ElementState::Released => {
                        self.host
                            .callback_args(
                                "OnMouseUp",
                                LuaMultiValue::from_vec(vec![LuaValue::Integer(btn)]),
                            )
                            .unwrap();
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                    winit::event::MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / 20.0,
                };
                if lines != 0.0 {
                    let dir = if lines > 0.0 { 1i64 } else { -1i64 };
                    self.host
                        .callback_args(
                            "OnMouseWheel",
                            LuaMultiValue::from_vec(vec![LuaValue::Integer(dir)]),
                        )
                        .unwrap();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(key_name) = pob_key_name(event.physical_key) {
                    let name = LuaValue::String(self.host.lua.create_string(key_name).unwrap());
                    let args = LuaMultiValue::from_vec(vec![name]);
                    match event.state {
                        winit::event::ElementState::Pressed => {
                            self.host.callback_args("OnKeyDown", args).unwrap();
                            self.pressed_keys
                                .lock()
                                .unwrap()
                                .insert(key_name.to_string());
                        }
                        winit::event::ElementState::Released => {
                            self.host.callback_args("OnKeyUp", args).unwrap();
                            self.pressed_keys
                                .lock()
                                .unwrap()
                                .remove(&key_name.to_string());
                        }
                    }
                }
                if event.state == ElementState::Pressed {
                    if let Some(text) = &event.text {
                        let ch =
                            LuaValue::String(self.host.lua.create_string(text.as_str()).unwrap());
                        self.host
                            .callback_args("OnChar", LuaMultiValue::from_vec(vec![ch]))
                            .unwrap();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(g) = &mut self.gfx {
                    let frame = match g.surface.get_current_texture() {
                        Ok(f) => f,
                        Err(_) => return,
                    };
                    let view = frame.texture.create_view(&Default::default());
                    let mut encoder = g.device.create_command_encoder(&Default::default());
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: None,
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color {
                                        r: 0.05,
                                        g: 0.05,
                                        b: 0.05,
                                        a: 1.0,
                                    }),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        println!("DrawImage calls: {}", self.draw_queue.lock().unwrap().len());
                        let draw_cmds = self
                            .draw_queue
                            .lock()
                            .unwrap()
                            .drain(..)
                            .collect::<Vec<_>>();
                        let mut vertices: Vec<Vertex> = Vec::new();
                        for cmd in &draw_cmds {
                            let x2 = cmd.x + cmd.w;
                            let y2 = cmd.y + cmd.h;
                            let tl = graphics::Vertex {
                                position: [cmd.x, cmd.y],
                                uv: [0.0, 0.0],
                                color: cmd.color,
                            };
                            let tr = graphics::Vertex {
                                position: [x2, cmd.y],
                                uv: [1.0, 0.0],
                                color: cmd.color,
                            };
                            let bl = graphics::Vertex {
                                position: [cmd.x, y2],
                                uv: [0.0, 1.0],
                                color: cmd.color,
                            };
                            let br = graphics::Vertex {
                                position: [x2, y2],
                                uv: [1.0, 1.0],
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
                        g.renderer.draw(
                            &mut pass,
                            &g.queue,
                            (g.config.width, g.config.height),
                            &vertices,
                        );

                        println!(
                            "DrawString calls: {}",
                            self.text_queue.lock().unwrap().len()
                        );
                        let text_cmds = self
                            .text_queue
                            .lock()
                            .unwrap()
                            .drain(..)
                            .collect::<Vec<_>>();
                        g.text_renderer
                            .prepare(
                                &g.device,
                                &g.queue,
                                (g.config.width, g.config.height),
                                &text_cmds,
                            )
                            .unwrap();
                        g.text_renderer.render(&mut pass).unwrap();
                    }
                    g.queue.submit(std::iter::once(encoder.finish()));
                    frame.present();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.host.callback("OnFrame").unwrap();
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();

    let root_dir = std::env::current_dir().unwrap();
    let draw_queue = Arc::new(Mutex::new(Vec::new()));
    let text_queue = Arc::new(Mutex::new(Vec::new()));
    let cursor_pos = Arc::new(Mutex::new([0.0, 0.0]));
    let pressed_keys = Arc::new(Mutex::new(HashSet::new()));
    let host = lua_host::LuaHost::new(
        root_dir,
        draw_queue.clone(),
        text_queue.clone(),
        cursor_pos.clone(),
        pressed_keys.clone(),
    )
    .unwrap();

    std::env::set_current_dir(host.root_dir.join("PathOfBuilding/src")).unwrap();
    host.launch().unwrap();
    println!(
        "main object set: {}",
        host.main_object.lock().unwrap().is_some()
    );
    host.lua.load("launch.devMode = true").exec().unwrap();
    host.callback("OnInit").unwrap();
    let msg: Option<String> = host.lua.load("return launch.promptMsg").eval().unwrap();
    println!("promptMsg: {:?}", msg);

    let mut app = App {
        window: None,
        gfx: None,
        host,
        draw_queue,
        text_queue,
        cursor_pos,
        pressed_keys,
    };

    event_loop.run_app(&mut app).unwrap();
}

fn pob_key_name(key: winit::keyboard::PhysicalKey) -> Option<&'static str> {
    use winit::keyboard::{KeyCode, PhysicalKey};

    let PhysicalKey::Code(code) = key else {
        return None;
    };
    match code {
        KeyCode::Escape => Some("ESCAPE"),
        KeyCode::Enter => Some("RETURN"),
        KeyCode::Backspace => Some("BACK"),
        KeyCode::Delete => Some("DELETE"),
        KeyCode::Tab => Some("TAB"),
        KeyCode::Space => Some("SPACE"),
        KeyCode::ArrowLeft => Some("LEFT"),
        KeyCode::ArrowRight => Some("RIGHT"),
        KeyCode::ArrowUp => Some("UP"),
        KeyCode::ArrowDown => Some("DOWN"),
        KeyCode::Home => Some("HOME"),
        KeyCode::End => Some("END"),
        KeyCode::PageUp => Some("PGUP"),
        KeyCode::PageDown => Some("PGDN"),
        KeyCode::Insert => Some("INSERT"),
        KeyCode::ShiftLeft | KeyCode::ShiftRight => Some("SHIFT"),
        KeyCode::ControlLeft | KeyCode::ControlRight => Some("CTRL"),
        KeyCode::AltLeft | KeyCode::AltRight => Some("ALT"),
        KeyCode::F1 => Some("F1"),
        KeyCode::F2 => Some("F2"),
        KeyCode::F3 => Some("F3"),
        KeyCode::F4 => Some("F4"),
        KeyCode::F5 => Some("F5"),
        KeyCode::F6 => Some("F6"),
        KeyCode::F7 => Some("F7"),
        KeyCode::F8 => Some("F8"),
        KeyCode::F9 => Some("F9"),
        KeyCode::F10 => Some("F10"),
        KeyCode::F11 => Some("F11"),
        KeyCode::F12 => Some("F12"),
        _ => None,
    }
}

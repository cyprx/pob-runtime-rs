use std::{
    collections::HashSet,
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use arboard::Clipboard;
use flate2::{Compression, read::DeflateDecoder, write::DeflateEncoder};
use image::EncodableLayout;
use mlua::prelude::*;

use crate::graphics::{CursorPos, DrawQueue, TextQueue, TextureUploadQueue};

pub struct LuaHost {
    pub lua: Lua,
    pub main_object: Arc<Mutex<Option<LuaRegistryKey>>>,
    pub root_dir: PathBuf,
}

impl LuaHost {
    pub fn new(
        root_dir: PathBuf,
        screen_size: Arc<Mutex<[u32; 2]>>,
        draw_queue: DrawQueue,
        text_queue: TextQueue,
        texture_queue: TextureUploadQueue,
        cursor_pos: CursorPos,
        pressed_keys: Arc<Mutex<HashSet<String>>>,
    ) -> LuaResult<Self> {
        let lua = unsafe { Lua::unsafe_new() };
        let main_object: Arc<Mutex<Option<LuaRegistryKey>>> = Arc::new(Mutex::new(None));
        let mo = main_object.clone();
        let clipboard = Arc::new(Mutex::new(Clipboard::new().unwrap()));

        let start_time = std::time::Instant::now();

        {
            let g = lua.globals();
            let script_path = Arc::new(root_dir.join("PathOfBuilding/src"));
            let runtime_path = root_dir.join("PathOfBuilding/runtime/lua");

            g.set(
                "GetTime",
                lua.create_function(move |_, ()| Ok(start_time.elapsed().as_millis() as u64))?,
            )?;

            g.set(
                "SetWindowTitle",
                lua.create_function(|_, _: String| Ok(()))?,
            )?;

            g.set("ConExecute", lua.create_function(|_, _: String| Ok(()))?)?;

            g.set("ConClear", lua.create_function(|_, ()| Ok(()))?)?;

            g.set(
                "ConPrintf",
                lua.create_function(|_, _: LuaMultiValue| Ok(()))?,
            )?;

            g.set(
                "SetMainObject",
                lua.create_function(move |lua, obj: LuaValue| {
                    *mo.lock().unwrap() = Some(lua.create_registry_value(obj)?);
                    Ok(())
                })?,
            )?;

            {
                let package: LuaTable = g.get("package")?;
                let current_path: String = package.get("path")?;
                let new_path = format!(
                    "{};{}/?.lua;{}/?/init.lua",
                    current_path,
                    runtime_path.display(),
                    runtime_path.display(),
                );
                package.set("path", new_path)?;
            }

            g.set(
                "RenderInit",
                lua.create_function(|_, _: LuaMultiValue| Ok(()))?,
            )?;

            g.set(
                "PCall",
                lua.create_function(|lua, (func, args): (LuaFunction, LuaMultiValue)| {
                    match func.call::<LuaMultiValue, LuaMultiValue>(args) {
                        Ok(results) => {
                            let mut out = vec![LuaValue::Nil];
                            out.extend(results);
                            Ok(LuaMultiValue::from_vec(out))
                        }
                        Err(e) => Ok(LuaMultiValue::from_vec(vec![LuaValue::String(
                            lua.create_string(e.to_string().as_bytes())?,
                        )])),
                    }
                })?,
            )?;

            let sp = script_path.clone();
            g.set(
                "PLoadModule",
                lua.create_function(move |lua, (name, args): (String, LuaMultiValue)| {
                    // check if name has suffix .lua or not
                    let mut full_name = name.clone();
                    if !name.ends_with(".lua") {
                        full_name += ".lua";
                    }

                    // build the full module path
                    let module_path = sp.join(full_name);

                    let code = std::fs::read_to_string(&module_path)
                        .map_err(|e| LuaError::RuntimeError(e.to_string()))?;
                    match lua.load(&code).call::<LuaMultiValue, LuaMultiValue>(args) {
                        Ok(results) => {
                            let mut out = vec![LuaValue::Nil];
                            out.extend(results);
                            Ok(LuaMultiValue::from_vec(out))
                        }
                        Err(e) => Ok(LuaMultiValue::from_vec(vec![LuaValue::String(
                            lua.create_string(e.to_string().as_bytes())?,
                        )])),
                    }
                })?,
            )?;

            let sp = script_path.clone();
            g.set(
                "LoadModule",
                lua.create_function(move |lua, (name, args): (String, LuaMultiValue)| {
                    // check if name has suffix .lua or not
                    let mut full_name = name.clone();
                    if !name.ends_with(".lua") {
                        full_name += ".lua";
                    }

                    // build the full module path
                    let module_path = sp.join(full_name);

                    let code = std::fs::read_to_string(&module_path)
                        .map_err(|e| LuaError::RuntimeError(e.to_string()))?;
                    lua.load(&code).call::<LuaMultiValue, LuaMultiValue>(args)
                })?,
            )?;

            let sp = script_path.clone();
            g.set(
                "GetScriptPath",
                lua.create_function(move |_, ()| Ok(sp.to_string_lossy().into_owned()))?,
            )?;

            let runtime_dir = root_dir.join("PathOfBuilding/runtime");
            g.set(
                "GetRuntimePath",
                lua.create_function(move |_, ()| Ok(runtime_dir.to_string_lossy().into_owned()))?,
            )?;
            g.set(
                "GetUserPath",
                lua.create_function(|_, ()| {
                    let path = dirs::data_dir().unwrap_or_default().join("PathOfBuilding");
                    std::fs::create_dir_all(&path).ok();
                    Ok(path.to_string_lossy().into_owned() + "/")
                })?,
            )?;
            g.set(
                "StripEscapes",
                lua.create_function(|_, s: String| {
                    let out = strip_pob_escapes(&s);
                    Ok(out)
                })?,
            )?;

            g.set(
                "MakeDir",
                lua.create_function(|_, path: String| {
                    std::fs::create_dir_all(&path).map_err(LuaError::external)?;
                    Ok(())
                })?,
            )?;

            g.set(
                "IsKeyDown",
                lua.create_function(move |_, key: String| {
                    Ok(pressed_keys.lock().unwrap().contains(&key))
                })?,
            )?;

            // clipboard
            let cb = clipboard.clone();
            g.set(
                "Copy",
                lua.create_function(move |_, text: String| {
                    cb.lock().unwrap().set_text(text).ok();
                    Ok(())
                })?,
            )?;
            let cb = clipboard.clone();
            g.set(
                "Paste",
                lua.create_function(move |_, ()| {
                    let text = cb.lock().unwrap().get_text().unwrap_or_default();
                    Ok(text)
                })?,
            )?;

            // Code parser
            g.set(
                "Deflate",
                lua.create_function(|_, (data, level): (LuaString, u32)| {
                    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(level));
                    encoder
                        .write_all(data.as_bytes())
                        .map_err(LuaError::external)?;
                    let compressed = encoder.finish().map_err(LuaError::external)?;
                    Ok(compressed)
                })?,
            )?;
            g.set(
                "Inflate",
                lua.create_function(|_, data: LuaString| {
                    let mut decoder = DeflateDecoder::new(data.as_bytes());
                    let mut out = String::new();
                    decoder
                        .read_to_string(&mut out)
                        .map_err(LuaError::external)?;
                    Ok(out)
                })?,
            )?;

            g.set(
                "SetDrawLayer",
                lua.create_function(|_, _: LuaMultiValue| Ok(()))?,
            )?;
            g.set(
                "SetViewport",
                lua.create_function(|_, _: LuaMultiValue| Ok(()))?,
            )?;
            let ss = screen_size.clone();
            g.set(
                "GetVirtualScreenSize",
                lua.create_function(move |_, ()| {
                    let v = ss.lock().unwrap();
                    Ok((v[0], v[1]))
                })?,
            )?;
            let ss = screen_size.clone();
            g.set(
                "GetScreenSize",
                lua.create_function(move |_, ()| {
                    let v = ss.lock().unwrap();
                    Ok((v[0], v[1]))
                })?,
            )?;

            let color: Arc<Mutex<[f32; 4]>> = Arc::new(Mutex::new([1.0, 1.0, 1.0, 1.0]));
            let color_set = color.clone();
            let color_draw = color.clone();
            g.set(
                "SetDrawColor",
                lua.create_function(
                    move |_, (r, g, b, a): (LuaValue, LuaValue, LuaValue, Option<LuaValue>)| {
                        let to_f32 = |v: LuaValue| match v {
                            LuaValue::Number(n) => n as f32,
                            LuaValue::Integer(n) => n as f32,
                            LuaValue::String(s) => s.to_str().unwrap_or("1").parse().unwrap_or(1.0),
                            _ => 1.0,
                        };
                        *color_set.lock().unwrap() = [
                            to_f32(r),
                            to_f32(g),
                            to_f32(b),
                            a.map(to_f32).unwrap_or(1.0),
                        ];
                        Ok(())
                    },
                )?,
            )?;

            let dq = draw_queue.clone();
            g.set(
                "DrawImage",
                lua.create_function(
                    move |_,
                          (handle, x, y, w, h, tcl, tct, tcr, tcb): (
                        LuaValue,
                        f32,
                        f32,
                        f32,
                        f32,
                        Option<f32>,
                        Option<f32>,
                        Option<f32>,
                        Option<f32>,
                    )| {
                        let texture_id = if let LuaValue::Table(t) = &handle {
                            t.get::<_, u32>("id").unwrap_or(0)
                        } else {
                            0
                        };
                        let color = *color_draw.lock().unwrap();
                        let uv = [
                            tcl.unwrap_or(0.0),
                            tct.unwrap_or(0.0),
                            tcr.unwrap_or(0.0),
                            tcb.unwrap_or(0.0),
                        ];
                        dq.lock().unwrap().push(crate::graphics::DrawCmd {
                            x,
                            y,
                            w,
                            h,
                            color,
                            texture_id,
                            uv,
                        });
                        Ok(())
                    },
                )?,
            )?;

            g.set(
                "DrawStringWidth",
                lua.create_function(|_, _: LuaMultiValue| Ok(1280u32))?,
            )?;

            let tq = text_queue.clone();
            let color_text = color.clone();
            g.set(
                "DrawString",
                lua.create_function(
                    move |_,
                          (x, y, _align, size, _font, text): (
                        f32,
                        f32,
                        String,
                        f32,
                        String,
                        String,
                    )| {
                        let color = *color_text.lock().unwrap();
                        let stripped_text = strip_pob_escapes(&text);
                        tq.lock().unwrap().push(crate::graphics::TextCmd {
                            x,
                            y,
                            size,
                            color,
                            text: stripped_text,
                        });
                        Ok(())
                    },
                )?,
            )?;

            g.set(
                "GetCursorPos",
                lua.create_function(move |_, ()| {
                    let pos = *cursor_pos.lock().unwrap();
                    Ok((pos[0], pos[1]))
                })?,
            )?;
            g.set(
                "DrawImageQuad",
                lua.create_function(|_, _: LuaMultiValue| Ok(()))?,
            )?;

            lua.load(
                r#"
                local _require = require
                local _utf8 = {
                    reverse = string.reverse,
                    gsub    = string.gsub,
                    find    = string.find,
                    sub     = string.sub,
                    match   = string.match,
                    next    = function(s, i, n) return i + (n or 1) end,
                }
                function require(name)
                    if name == "lcurl.safe" then return nil end
                    if name == "lua-utf8" then return _utf8 end
                    return _require(name)
                end
                "#,
            )
            .exec()?;
            lua.load("arg = {}").exec()?;

            let next_id = Arc::new(Mutex::new(1));
            let tuq = texture_queue.clone();
            g.set(
                "NewImageHandle",
                lua.create_function(move |lua, ()| {
                    let id = {
                        let mut n = next_id.lock().unwrap();
                        let id = *n;
                        *n += 1;
                        id
                    };

                    let t = lua.create_table()?;
                    t.set("id", id)?;
                    t.set("valid", false)?;
                    t.set("width", 0u32)?;
                    t.set("height", 0u32)?;

                    let tuq2 = tuq.clone();

                    t.set(
                        "Load",
                        lua.create_function(
                            move |_, (this, path, _): (LuaTable, String, LuaMultiValue)| {
                                let img = match image::open(&path) {
                                    Ok(img) => img.to_rgba8(),
                                    Err(e) => {
                                        println!("Load image {}: {}", path, e);
                                        return Ok(());
                                    }
                                };
                                let w = img.width();
                                let h = img.height();
                                let rgba = img.into_raw();
                                tuq2.lock()
                                    .unwrap()
                                    .push(crate::graphics::TextureUploadCmd {
                                        id,
                                        rgba: rgba,
                                        width: w,
                                        height: h,
                                    });
                                this.set("valid", true)?;
                                this.set("width", w)?;
                                this.set("height", h)?;

                                Ok(())
                            },
                        )?,
                    )?;

                    t.set(
                        "IsValid",
                        lua.create_function(|_, this: LuaTable| Ok(this.get::<_, bool>("valid")?))?,
                    )?;
                    t.set(
                        "ImageSize",
                        lua.create_function(|_, this: LuaTable| {
                            Ok((this.get::<_, u32>("width")?, this.get::<_, u32>("height")?))
                        })?,
                    )?;
                    t.set(
                        "Unload",
                        lua.create_function(|_, this: LuaTable| this.set("valid", false))?,
                    )?;
                    t.set(
                        "SetLoadingPriority",
                        lua.create_function(|_, _: LuaMultiValue| Ok(()))?,
                    )?;

                    Ok(t)
                })?,
            )?;
        }

        Ok(Self {
            lua,
            main_object,
            root_dir,
        })
    }

    pub fn launch(&self) -> LuaResult<()> {
        let path = self.root_dir.join("PathOfBuilding/src/Launch.lua");
        let code =
            std::fs::read_to_string(&path).map_err(|e| LuaError::RuntimeError(e.to_string()))?;
        self.lua.load(&code).exec()
    }

    pub fn callback(&self, name: &str) -> LuaResult<()> {
        let guard = self.main_object.lock().unwrap();
        let Some(key) = guard.as_ref() else {
            return Ok(());
        };

        let obj: LuaTable = self.lua.registry_value(key)?;
        if let Ok(func) = obj.get::<_, LuaFunction>(name) {
            func.call::<_, ()>(obj.clone())?;
        }
        Ok(())
    }

    pub fn callback_args(&self, name: &str, args: LuaMultiValue) -> LuaResult<()> {
        let guard = self.main_object.lock().unwrap();
        let Some(key) = guard.as_ref() else {
            return Ok(());
        };

        let obj: LuaTable = self.lua.registry_value(key)?;
        let mut args_vec = vec![LuaValue::Table(obj.clone())];
        args_vec.extend(args);
        if let Ok(func) = obj.get::<_, LuaFunction>(name) {
            func.call::<LuaMultiValue, ()>(LuaMultiValue::from_vec(args_vec))?;
        }
        Ok(())
    }
}

fn strip_pob_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '^' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('0'..='9') => {
                chars.next();
            }
            Some('x') => {
                chars.next();
                for _ in 0..6 {
                    match chars.peek() {
                        Some(h) if h.is_ascii_hexdigit() => {
                            chars.next();
                        }
                        _ => break,
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_time_returns_u64() {
        let root_dir = std::env::current_dir().unwrap();
        let dq = Arc::new(Mutex::new(vec![]));
        let tq = Arc::new(Mutex::new(vec![]));
        let cp = Arc::new(Mutex::new([0.0, 0.0]));
        let hs = Arc::new(Mutex::new(HashSet::new()));
        let host = LuaHost::new(root_dir, dq, tq, cp, hs).unwrap();
        let t: u64 = host.lua.load("return GetTime()").eval().unwrap();
        assert!(t < 1000);
    }

    #[test]
    fn window_title_does_not_crash() {
        let root_dir = std::env::current_dir().unwrap();
        let dq = Arc::new(Mutex::new(vec![]));
        let tq = Arc::new(Mutex::new(vec![]));
        let cp = Arc::new(Mutex::new([0.0, 0.0]));
        let hs = Arc::new(Mutex::new(HashSet::new()));
        let host = LuaHost::new(root_dir, dq, tq, cp, hs).unwrap();
        host.lua.load(r#"SetWindowTitle("test")"#).exec().unwrap();
    }
}

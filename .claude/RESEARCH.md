# pob-runtime-rs Research Notes

> **Rule**: Update this file whenever a new finding is discovered during debugging.
> Link to it from MEMORY.md so it persists across sessions.

---

## Click-handling / Popup Investigation (2026-03-01)

### Confirmed root cause: popup height exceeds window height

The Options popup (`main:OpenOptionsPopup`) has ~25 rows at 26px each + headers:
- Final `currentY` ≈ **753px** (the height passed to `OpenPopup`)
- In a **1280×720** window: `popup.y = (720 - 753) / 2 = -17` → **off-screen at y = -17**
- In **1080p** fullscreen: `popup.y = (1080 - 753) / 2 = 163` → **visible**

Since `main.popups[1]` is set, ALL keyboard/mouse events go to the popup's
`ProcessInput`, not the main controls. The popup is invisible but silently consumes
every click → explains why modeSkills appears unresponsive.

**Fix**: Increase initial window height. Need at least ~800px; 900px+ recommended.

### Screen size handling is correct

`screen_size: Arc<Mutex<[u32; 2]>>` is:
1. Initialized to `[1280, 720]` in `main()` (before window created)
2. Updated in `resumed()` from `window.inner_size()` → confirmed prints 1280×720
3. Updated in `WindowEvent::Resized` handler

GetVirtualScreenSize → Common.lua wrapper → calls GetScreenSize → reads screen_size Arc.
`main.screenW/screenH` updated every `OnFrame` → always current.

**No bug in screen_size flow.** The Resized event is NOT needed to fix screen_size.
The popup resize "fix" works only because the window becomes tall enough to show the popup.

### Event flow through PoB

```
winit MouseInput(Pressed, LEFTBUTTON)
  → main.rs: callback_args("OnKeyDown", ["LEFTBUTTON", false])
  → launch:OnKeyDown (if not promptMsg) → main:OnKeyDown
  → inputEvents += {KeyDown, "LEFTBUTTON"}

winit MouseInput(Released, LEFTBUTTON)
  → main.rs: callback_args("OnKeyUp", ["LEFTBUTTON"])
  → launch:OnKeyUp (if not promptMsg) → main:OnKeyUp
  → inputEvents += {KeyUp, "LEFTBUTTON"}

Next OnFrame:
  → main:OnFrame → if popups[1]: ProcessInput → wipeTable(inputEvents) ← BLOCKER
  → else: ProcessControlsInput → Build:OnFrame → Build:ProcessControlsInput
  → isMouseInRegion(viewPort)? → GetMouseOverControl → ButtonControl:OnKeyDown
  → self.clicked = true, selControl = button
  → On KeyUp: selControl:OnKeyUp → IsMouseOver? → onClick()
```

### Viewport-relative coordinates confirmed

Inside `SetViewport(x, y, w, h)`, PoB uses **(0, 0) as origin**, not absolute screen coords.
- `DrawString(0, 0, ...)` inside viewport draws at absolute (x, y)
- This was verified from DropDownControl.lua line ~320
- Our coordinate-shift implementation in Rust is **correct**

Every control that sets a viewport also calls `SetViewport()` (no args) to reset.
So viewport should be null (offset=0,0) by the time popup is drawn.
PopupDialog:Draw uses absolute screen coords (popup.x = (screenW-width)/2) — should work.

### Popup position formula (PopupDialog.lua)

```lua
self.x = function() return m_floor((main.screenW - width) / 2) end
self.y = function() return m_floor((main.screenH - height) / 2) end
```

Tall popups that exceed screenH go **negative** — invisible but still capturing input.

### Other popup heights

- Options popup: **753px** (too tall for 720px window)
- About popup: 628px (borderline for 720px: y = 46, OK)
- Update Available: 600px (y = 60, OK)
- OpenMessagePopup: `70 + numMsgLines*16` (usually small)

## Text Color Rendering (2026-03-01) — COMPLETE

### Implementation
- `DrawString` in `lua_host.rs`: removed `strip_pob_escapes` call — raw escape codes kept in `TextCmd.text`
- `DrawStringWidth` / `DrawStringCursorIndex`: still strip (measurement needs clean text)
- `graphics.rs`: added `parse_color_spans(&str, default_color) -> Vec<(&str, [f32;4])>`
  - `^xRRGGBB`: parsed via `u32::from_str_radix(..., 16)`, bit-shifted to extract R/G/B bytes
  - `^N` (digit): mapped via `pob_digit_color(digit, alpha)` palette
  - Unknown escapes: `^` char skipped, next char left as-is
- `TextRenderer::prepare`: replaced `buffer.set_text` with `buffer.set_rich_text` using per-span `Attrs::color()`
- `SetDrawColor("^xRRGGBB")` string form: still pending (only needed for CALCS tab)

### Digit palette (^0–^9) — educated guesses, tune visually
```
^0 = black  (0.0, 0.0, 0.0)   confident
^1 = red    (1.0, 0.0, 0.0)
^2 = green  (0.0, 1.0, 0.0)
^3 = blue   (0.0, 0.0, 1.0)
^4 = yellow (1.0, 1.0, 0.0)
^5 = gray   (0.5, 0.5, 0.5)
^6 = gray   (0.5, 0.5, 0.5)
^7 = white  (1.0, 1.0, 1.0)   confident (NORMAL text)
^8 = lt.gray(0.75,0.75,0.75)
^9 = dk.gray(0.3, 0.3, 0.3)
```

### Minimum viable window height

Options popup is the tallest at 753px.
- Minimum: > 753px
- Comfortable: ≥ 900px (gives 73px margin each side)
- Recommended initial size: **1280 × 900** or **1600 × 900**

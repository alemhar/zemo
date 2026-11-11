# Zemo — Product Requirements Document

**A transparent, always-on-top screen annotation layer for live video calls.**

| | |
|---|---|
| **Status** | Draft v0.1 |
| **Owner** | alemhar@synnch.com.au |
| **Date** | 2026-11-11 |
| **Target platform** | Windows 10/11 (x64) — macOS parked for v2 |
| **Stack** | Tauri v2 · Rust · React 19 · TypeScript · Konva · Zustand |
| **Prior art (same author)** | Klipp / SnippingZo at `D:\FuselabsAI\Projects\SnippingZo` — capture & recording annotator |

---

## 1. Problem

When you share your screen on a video call, you lose the single most useful thing you have in a
room with a whiteboard: the ability to **point at something**.

Today presenters fall back on:

- *"the button on the left… no, the other left… under the blue header"* — verbal pointing, slow and ambiguous
- Wiggling the mouse cursor — imprecise, invisible at low share framerates, gone the instant you move
- Dropping out of the app to a drawing tool, or pasting a screenshot into a whiteboard — kills the flow

The result is wasted call time, misunderstood instructions, and demos that don't land. The fix is
well understood — draw on top of the live screen — but existing tools are either unmaintained,
modal (you *stop* presenting to annotate), or bolted into a specific conferencing vendor.

### Positioning

| Tool | Missing |
|---|---|
| **Microsoft ZoomIt** | Great zoom & draw, but drawing is a blocking mode that freezes the screen; no modern shape tools, no arrows worth the name, no config UI |
| **Epic Pen** | Closest competitor, but paywalls shapes/arrows behind Pro, heavy UI chrome, ageing stack |
| **Presentify** | macOS only |
| **Zoom / Teams built-in annotation** | Vendor-locked, only works inside that vendor's share, missing on Meet, unusable when presenting from a browser tab |
| **Klipp (our own)** | Annotation is attached to a capture/recording session, not to the live desktop — wrong loop for a call |

**Zemo's positioning:** a lightweight, vendor-neutral, always-available annotation layer that works
on any call — Zoom, Teams, Meet, Slack huddle, Discord — because it draws on the desktop itself,
not inside the conferencing app.

---

## 2. Goals & non-goals

### Goals

- **G1 — Point in under a second.** From "I'm talking" to "there's an arrow on that button" must be
  one hotkey and one drag. Anything slower and presenters go back to wiggling the mouse.
- **G2 — Never get in the way.** When not drawing, Zemo must be completely invisible and completely
  click-through. A presenter must never find themselves clicking an overlay instead of their app.
- **G3 — Visible to the people on the call.** The overlay must appear in the remote participants'
  video feed, not just on the presenter's monitor (see §4 — this is the product's existence premise).
- **G4 — Survive a whole call.** No crashes, no leaked GPU memory, no state where the screen is
  stuck dimmed or stuck intercepting clicks. A panic hotkey always restores a clean desktop.

### Non-goals (explicitly out of scope)

Everything that made Klipp heavy is deliberately excluded:

- ❌ Screen recording, FFmpeg, video encoding, file output of any kind
- ❌ Screenshot capture, image saving, clipboard image workflows
- ❌ Webcam bubble / camera overlay
- ❌ OCR / text extraction
- ❌ Cloud sync, accounts, telemetry, licensing servers
- ❌ Collaborative/remote annotation (participants drawing back) — see §10
- ❌ Replacing the conferencing app's own share controls

Zemo draws pixels on a transparent window. That is the entire product.

---

## 3. Users

| Persona | Use case | What they need most |
|---|---|---|
| **Engineer in a design/code review** | "This null check, right here" | Arrow + rectangle, fast, precise |
| **Support / CSM doing a walkthrough** | "Click the gear, then Billing" | Numbered steps, arrows that persist while they talk |
| **Teacher / trainer** | Explaining a diagram live | Pen, highlighter, spotlight, zoom |
| **Product/design demo** | Calling out a UI detail | Spotlight, zoom, clean marks that look deliberate |

All four share one trait: **they are mid-sentence**. Every interaction must be executable without
looking away from what they're presenting.

---

## 4. Critical constraint — screen-share visibility

> **Zemo's annotations are only visible to call participants when the presenter shares an
> entire screen / monitor. Window-share and application-share will not show them.**

This is a hard constraint of how Windows screen capture works, not a bug we can fix:

- **Monitor / full-screen share** (Zoom "Screen 1", Teams "Screen", Meet "Entire Screen") captures
  the composited desktop via DXGI Desktop Duplication or `Windows.Graphics.Capture` on the display.
  Every visible top-level window, including a layered always-on-top overlay, is composited in. ✅
- **Window / application share** captures a single `HWND`'s own surface. Zemo's overlay is a
  *different* window, so it is structurally absent from that stream. ❌

### Requirements this generates

- **R4.1** — First-run onboarding must state, in one sentence, "share your whole screen, not a
  window" and show it visually. This is the #1 predictable support issue.
- **R4.2** — Zemo's own **toolbar pill must be excluded from capture** via
  `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)`, so remote participants see the
  annotations but never the presenter's controls. The annotation overlay must explicitly **not**
  set this affinity.
  *Requires Windows 10 2004 (build 19041) or later.* On older builds the call is a no-op and the
  toolbar will appear in the share — detect this at startup and warn once.
- **R4.3** — Document the limitation in the README and in-app Help. Do not hide it.
- **R4.4 — Share detection is advisory only.** Zemo may detect that a conferencing app has begun
  sharing (process + capture-session heuristics) for **one purpose only**: showing a one-time,
  dismissible hint when the share looks like a *window* share rather than a monitor share.
  Detection must **never** change Zemo's state — it does not arm draw mode, does not show the
  toolbar, does not grab input. See R5.6.
- **V2 escape hatch** — a virtual camera / virtual display output that composites desktop +
  annotations into a shareable source, making window-share irrelevant. Parked, see §10.

### Spike required before build (see §9 R1)

Klipp proves the overlay is captured by **gdigrab** (a display-capture path). Teams and Zoom use
different capture backends. A ~1 hour spike must confirm a layered, always-on-top, `WS_EX_LAYERED`
overlay appears in a real Teams and Zoom monitor-share before any feature work starts. If it does
not, the whole product design changes and the virtual-camera path becomes v1.

---

## 5. Interaction model — the core of the product

This is the part Klipp never had to solve: Klipp's overlay only exists during a recording session.
Zemo's overlay exists *all the time*, on top of apps the user is actively driving. The rapid toggle
between "using my computer" and "drawing on my computer" **is** the product.

### 5.1 Two states

```
            Ctrl+Alt+Z  (or click a toolbar tool)
  ┌─────────────────┐ ──────────────────────────▶ ┌─────────────────┐
  │   PASS-THROUGH  │                             │   DRAW MODE     │
  │   (default)     │ ◀────────────────────────── │   (armed)       │
  └─────────────────┘   Esc  /  Ctrl+Alt+Z        └─────────────────┘
   • fully click-through                           • overlay captures mouse
   • WS_EX_TRANSPARENT on                          • WS_EX_TRANSPARENT off
   • existing marks still visible                  • crosshair cursor
   • zero input interference                       • active tool lit in pill
```

- **R5.1** — Default state on launch is PASS-THROUGH. Zemo never starts armed.
- **R5.2** — Entering DRAW MODE is a single global hotkey, available from any focused app.
- **R5.3** — `Esc` always returns to PASS-THROUGH. It never clears or deletes marks.
- **R5.4** — Existing annotations remain **visible** in PASS-THROUGH. The presenter keeps talking
  over an arrow they drew ten seconds ago while clicking through it normally. This is the behaviour
  that separates Zemo from modal tools like ZoomIt.
- **R5.5** — State is visible at a glance: the toolbar pill changes colour, and a thin accent border
  (2px, inset) frames the screen while armed, so the presenter never wonders which state they're in.
- **R5.6 — No auto-arm.** Zemo enters DRAW MODE only on explicit user action — the global hotkey,
  a toolbar tool click, or the tray menu. It never arms itself in response to a detected screen
  share, a call starting, or any other ambient signal. *Rationale:* auto-arming would begin
  intercepting clicks while the presenter is still arranging windows and clicking "Share", which
  breaks G2 outright. Predictability beats the half-second saved.
- **R5.7 — Panic hotkey.** `Ctrl+Alt+Q` returns to PASS-THROUGH, clears all marks, cancels spotlight
  and zoom, and restores a pristine desktop from any state, including a broken one.

### 5.2 Annotation lifetime — sticky, manual clear

- **R5.8** — Marks persist until explicitly cleared. They do not expire by default.
- **R5.9** — `Ctrl+Alt+C` clears all marks. The clear is undoable for 5 seconds, and that
  undo window is **global**: `Ctrl+Z` reverses a clear even from pass-through, without arming
  first. (All other undo remains armed-only per R5.11.)
- **R5.10** — Optional **auto-fade** setting: marks fade out after *N* seconds (default off; N
  configurable 3–30s, default 8s). Fade is a 400ms opacity ramp, not a pop.
- **R5.11** — Undo/redo (`Ctrl+Z` / `Ctrl+Shift+Z`) while armed, minimum 50 steps.
- **R5.12 — Marks are screen-anchored, not window-anchored.** A mark is stored in absolute screen
  coordinates and stays where it was drawn. It does **not** follow the window beneath it when that
  window is moved, resized, or scrolled. *Rationale:* this matches the real loop (point, talk,
  clear) and avoids per-mark `HWND` tracking, which would still break on scroll — the window moves
  but its content doesn't. Window-following is parked in §10. The one exception is zoom, where
  marks anchor to screen coordinates so they land correctly when zoom is released (R6.14).

### 5.3 Toolbar — a separate always-on-top pill window

- **R5.13** — The toolbar is its **own small frameless window**, not a region of the overlay.
  *Rationale:* Klipp hit exactly this — in-overlay buttons are unclickable whenever the overlay is
  click-through, and making the overlay interactive to click a button breaks the pass-through
  contract. See `docs/plans/06-pill-buttons-blocked-by-overlay.md` in the Klipp repo.
- **R5.14** — Draggable, snaps to screen edges, remembers position across sessions.
- **R5.15** — Collapsible to a single ~40px puck; auto-collapses after 4s of no interaction while
  armed so it doesn't sit in the middle of the shared screen.
- **R5.16** — Excluded from screen capture per R4.2.
- **R5.17** — Contents: tool buttons, colour swatches, stroke width, clear, undo, settings, quit.

---

## 6. Features — v1 scope

### 6.1 Annotation tools (must-have)

| Tool | Key | Behaviour |
|---|---|---|
| **Arrow** | `A` | **Head-first** — see R6.0. **The flagship tool; it is the default on first launch.** |
| **Rectangle** | `R` | Drag corner to corner. Outline only; `Shift` constrains to square. |
| **Ellipse** | `O` | Drag bounding box; `Shift` constrains to circle. |
| **Line** | `L` | Straight line; `Shift` snaps to 15° increments. |
| **Pen (freehand)** | `P` | Smoothed freehand stroke. |
| **Highlighter** | `H` | Wide, ~40% alpha, multiply blend. |
| **Text** | `T` | Click to place, type inline, `Esc`/click-away to commit. Default 24px, high-contrast with a subtle contrasting outline so it reads over any background. |
| **Step numbers** | `N` | Click to drop an auto-incrementing numbered badge (1, 2, 3…) for "do this, then this" walkthroughs. Counter resets on clear. |
| **Eraser** | `E` | Click a mark to delete it (object eraser, not pixel eraser). |

#### R6.0 — Arrow direction: head-first, tail-second

**The first click places the arrow's point (head). The second click/release places its tail.**
This is the inverse of every drawing app, and it is deliberate.

*Rationale:* the presenter already knows what they are pointing **at** — their attention and their
cursor are on the target. Forcing them to first guess where the tail should start, then drag to the
target, means aiming twice and thinking backwards mid-sentence. Head-first lets them click the thing
they're talking about immediately and then sweep the tail out into whatever empty space is free.

- **R6.0.1** — Interaction supports both idioms: *click-release-click* (click head, move, click tail)
  and *press-drag-release* (press on head, drag out, release at tail). Both produce an identical arrow.
- **R6.0.2** — During the drag, a live preview arrow renders with its **head pinned at the first point**,
  so the direction is unambiguous before commit.
- **R6.0.3** — `Esc` or right-click mid-draw cancels the in-progress arrow without committing.
- **R6.0.4** — `Shift` while placing the tail snaps the shaft to 15° increments.
- **R6.0.5** — Filled triangular head; head size scales with stroke width.
- **R6.0.6** — A **"tail-first (classic)"** toggle exists in Settings for users who find the inverse
  jarring. Default is **head-first**. This also applies to the Line tool's optional end-cap.

**Styling**

- **R6.1** — 8 preset colours on `1`–`8`: red, orange, yellow, green, cyan, blue, magenta, white.
  Default red. Colours chosen for contrast against both light and dark application backgrounds.
- **R6.2** — Stroke width cycles with `[` / `]`, 4 steps (2/4/6/10px at 100% scale).
- **R6.3** — Every mark gets a 1px dark outer stroke at ~30% alpha so red-on-red or
  white-on-white stays legible. Non-negotiable for a tool that draws over arbitrary UI.
- **R6.4** — Colour and width are sticky per tool and persist across sessions.

### 6.2 Presenter extras (confirmed in v1 scope)

**Cursor highlight & click ripples** — `Ctrl+Alt+H`
- **R6.5** — Soft radial halo follows the cursor (size/colour/opacity configurable).
- **R6.6** — Click ripples: expanding ring on click, distinct colour for left vs right button.
- **R6.7** — Works in PASS-THROUGH mode (this is its whole point — you're driving your app normally).
- *Reuses Klipp's `WH_MOUSE_LL` global mouse hook verbatim — see §8.3.*

**Spotlight / dim-the-rest** — `Ctrl+Alt+S`
- **R6.8** — Screen dims to a configurable opacity (default 60% black) except a bright region.
- **R6.9** — Region follows the cursor as a circle (scroll wheel resizes), **or** is pinned as a
  rectangle by dragging. `Esc` or the hotkey again releases it.
- **R6.10** — Annotations drawn while spotlight is active render *above* the dim layer.

**Screen zoom / magnifier** — `Ctrl+Alt+X`
- **R6.11** — Zoom into the live screen around the cursor, 2×–8×, scroll wheel or `+`/`-` to adjust.
- **R6.12** — **Live, not frozen.** The magnified content must keep updating — a frozen zoom (ZoomIt's
  behaviour) is unusable for demoing an app that is actively doing something.
- **R6.13** — Pan by moving the cursor; `Esc` exits and restores 1×.
- **R6.14** — Annotation tools remain usable while zoomed; marks anchor to **screen coordinates**,
  so they stay on the right UI element when zoom is released.
- ⚠️ **Highest-risk item in v1.** Requires continuous desktop capture (`Windows.Graphics.Capture`)
  rendered into the overlay at 30–60fps. Scheduled last (§9 Phase 4) so it can slip to v1.1 without
  blocking release. If the performance budget (§7) cannot be met, ship it frozen-frame and say so.

### 6.3 Shortcuts

Defaults are chosen to avoid known conferencing-app bindings. **All shortcuts must be remappable
from Settings in v1** — the collision surface is too large to guarantee a clean default set.

| Shortcut | Action | Scope |
|---|---|---|
| `Ctrl+Alt+Z` | Toggle draw mode | Global |
| `Esc` | Exit to pass-through (or cancel in-progress shape) | While armed |
| `Ctrl+Alt+C` | Clear all marks | Global |
| `Ctrl+Alt+H` | Toggle cursor highlight | Global |
| `Ctrl+Alt+S` | Toggle spotlight | Global |
| `Ctrl+Alt+X` | Toggle zoom | Global |
| `Ctrl+Alt+Q` | **Panic** — reset everything | Global |
| `Ctrl+Alt+T` | Show/hide toolbar | Global |
| `A R O L P H T N E` | Select tool | While armed |
| `1`–`8` | Colour | While armed |
| `[` `]` | Stroke width | While armed |
| `Ctrl+Z` / `Ctrl+Shift+Z` | Undo / redo | While armed |

**Known conflicts to avoid in defaults** — Teams: `Ctrl+Shift+M/O/E/Space`. Zoom: `Alt+A/V/S/R`,
`Alt+Shift+S`. Meet (Chrome): `Ctrl+D`, `Ctrl+E`.

Klipp's `Ctrl+Shift+S` / `Ctrl+Shift+E` are listed for completeness only — per A5, the two apps are
not expected to run simultaneously, so this is not a design constraint.

**AltGr caution** — every global default is `Ctrl+Alt+<letter>`, and Windows synthesizes
`Ctrl+Alt` from **AltGr** on non-US keyboard layouts. On those layouts `Ctrl+Alt+Z/S/X/C/Q` can
fire while the user types ordinary special characters. R6.15 remapping is the mitigation; consider
detecting a non-US layout at first run and proposing an alternative default set.

- **R6.15** — Settings must detect and warn on a shortcut that fails to register (another app owns it).
- **R6.16** — While armed, single-letter tool keys are swallowed by the overlay and must **not**
  leak to the underlying app.

### 6.4 Settings

- **R6.17** — Launch on Windows startup (opt-in), start minimised to tray.
- **R6.18** — System tray icon: toggle draw mode, toolbar, settings, quit.
- **R6.19** — Settings persisted to disk as JSON; theme follows system dark/light.
- **R6.20** — Configurable: all shortcuts, default tool, default colour/width, arrow direction
  (R6.0.6), auto-fade on/off + duration, spotlight dim level, highlight appearance, zoom max level.

---

## 7. Non-functional requirements

| | Requirement |
|---|---|
| **N1 — Latency** | Hotkey → armed and drawable in **< 100ms**. Pointer-to-ink latency < 30ms. |
| **N2 — Idle cost** | < 1% CPU and < 150MB RAM when idle in pass-through. It runs for the whole workday. |
| **N3 — Active cost** | < 10% CPU while drawing; < 25% CPU while zoom is live at 60fps. |
| **N4 — Input integrity** | Zero measurable input lag added to the underlying app in pass-through. |
| **N5 — DPI** | Correct on 100/125/150/200% scaling. All geometry in physical pixels. |
| **N6 — Startup** | Cold start to tray-ready < 2s. |
| **N7 — Stability** | No state where the desktop is left dimmed or click-blocking. Panic hotkey is the guaranteed escape. A crash must not leave an orphaned always-on-top window. |
| **N8 — Install** | Single unsigned NSIS installer + portable exe, < 15MB. No runtime downloads (contrast: Klipp's FFmpeg fetch). |

---

## 8. Technical approach

### 8.1 Stack

| Layer | Choice | Why |
|---|---|---|
| Shell | **Tauri v2** | Native WebView2, ~10× smaller than Electron, and we already have a working transparent-overlay implementation in it |
| Backend | **Rust** | Direct Win32 access for layered windows, hooks, display affinity |
| Frontend | **React 19 + TypeScript** | Known quantity, fast iteration |
| Canvas | **Konva + react-konva** | Already validated for annotation in Klipp — see its `docs/architecture/01-konvajs-canvas-choice.md` |
| State | **Zustand + Immer** | Minimal boilerplate |
| Styling | **Tailwind v4 + Radix** | Toolbar and settings only; the overlay itself is canvas |
| Shortcuts | `@tauri-apps/plugin-global-shortcut` | Already wrapped in Klipp's `useGlobalShortcut.ts` |

### 8.2 Window topology

```
┌─ overlay (1 per monitor; v1 = primary only) ──────────────────┐
│  frameless · transparent · always_on_top · skip_taskbar       │
│  WS_EX_LAYERED always on; WS_EX_TRANSPARENT toggled per mode  │
│  NOT capture-excluded  →  participants see the ink            │
│  Konva stage, full monitor bounds, physical pixels            │
└───────────────────────────────────────────────────────────────┘
┌─ toolbar pill ───────────┐   ┌─ settings (on demand) ─┐
│ frameless · always_on_top│   │ normal decorated window│
│ WDA_EXCLUDEFROMCAPTURE   │   └────────────────────────┘
└──────────────────────────┘
      + system tray
```

### 8.3 Inherited solutions (the reason this is a weeks project, not a months one)

Five hard Windows problems are already solved in the Klipp codebase and port directly:

| Problem | Klipp source | Note |
|---|---|---|
| **WebView2 won't honour transparency** via Tauri config alone | `src-tauri/src/commands/overlay.rs` → `apply_webview_transparency()` | Casts the controller to `ICoreWebView2Controller2` and sets `DefaultBackgroundColor` with `A: 0`. Must be re-applied after any resize. |
| **Click-through toggle destroys transparency** | `overlay.rs` → `set_click_through_preserving_layered()` | Tauri's `set_ignore_cursor_events` strips `WS_EX_LAYERED` along with `WS_EX_TRANSPARENT`. Flip only `WS_EX_TRANSPARENT` via `SetWindowLongPtrW`, force `WS_EX_LAYERED` on. **This is the single most important thing to port** — it is exactly Zemo's core state toggle. |
| **Frameless windows have invisible ~8px chrome** | `overlay.rs` → outer/inner position delta compensation | `set_position` is outer coords, WebView2 renders inner. Measure the delta and re-position so the inner surface lands on the true monitor bounds. |
| **Detecting clicks anywhere on the desktop** | `overlay.rs` → `mouse_proc` / `start_mouse_hook` with `SetWindowsHookExW(WH_MOUSE_LL)` | Emits a Tauri `mouse-click` event. Powers R6.6 ripples directly. |
| **Buttons inside a click-through overlay are dead** | `docs/plans/06-pill-buttons-blocked-by-overlay.md` | Drove the separate-pill-window decision in R5.13. |

### 8.4 New work not inherited from Klipp

- Full-monitor overlay sizing and DPI handling (Klipp's overlay is region-sized)
- The pass-through ⇄ draw-mode state machine, its visual affordances, and the panic path
- Head-first arrow interaction and its live preview (R6.0)
- Global keyboard capture while armed, including swallowing keys from the underlying app (R6.16)
- `SetWindowDisplayAffinity` on the toolbar (R4.2)
- Spotlight dim layer with a punch-out mask
- Live screen zoom via `Windows.Graphics.Capture` (R6.11–R6.14) — genuinely new, genuinely hard
- Step-number tool, shortcut remapping UI, startup/tray integration

---

## 9. Assumptions, risks, and phasing

### Assumptions

- **A1** — Windows 10/11 x64 only for v1. macOS is a v2 project with a completely different
  overlay implementation (`NSWindow` + `CGWindowLevel`) and a screen-recording permission flow.
- **A2** — **Primary monitor only** for v1. Multi-monitor spawns one overlay per display and is a
  v2 plan doc. *Documented limitation: if you share a non-primary display, annotations won't appear
  there.* First-run onboarding must say so.
- **A3** — No code signing for v1; SmartScreen warning is accepted (same posture as Klipp).
- **A4** — Single-user, local-only. No network calls at all.
- **A5** — **Zemo and Klipp are independent products and are not expected to run at the same time.**
  No shared tray, no shared shortcut registry, no shared crate. Klipp code is a *reference to copy
  from* (§8.3), not a dependency. If both do end up running, R6.15's registration-failure warning
  is the only mitigation required.
- **A6** — **MIT licensed, open-source, free.** Public repo, GitHub releases, no accounts, no
  licence keys, no paid tier. Same posture as Klipp.

### Risks

| # | Risk | Severity | Mitigation |
|---|---|---|---|
| **R1** | **Overlay doesn't appear in Teams/Zoom monitor share.** The product's existence premise. Klipp only proves it for gdigrab. | 🔴 Critical | **Spike before any feature work** (§4). Build a 50-line Tauri app with a red always-on-top layered square; join a real Teams call and a real Zoom call; share the monitor; confirm a second participant sees it. Record the result in `docs/plans/01-share-visibility-spike.md`. |
| **R2** | Live zoom can't hit 30fps within the CPU budget | 🟠 High | Phased last; fallback is frozen-frame zoom, shipped with the limitation documented |
| **R3** | Default shortcuts collide with a user's conferencing app | 🟠 High | Remappable from v1 (R6.15); defaults avoid known bindings; registration failures surfaced in UI |
| **R4** | Presenter shares a *window*, sees nothing, concludes the app is broken | 🟠 High | Onboarding (R4.1), README, in-app Help; consider detecting a running conferencing app and showing a one-time hint |
| **R5** | Click-through toggle races leave the desktop input-blocked | 🟠 High | Panic hotkey (R5.7); a watchdog that forces pass-through if the overlay is armed with no input for 10 minutes |
| **R6** | Full-screen exclusive games/apps punch through always-on-top | 🟡 Medium | Out of scope — Zemo targets desktop app sharing |
| **R7** | Some conferencing apps' capture paths exclude layered windows | 🟡 Medium | Covered by the R1 spike; if it bites only one vendor, document it |
| **R8** | Head-first arrows confuse users coming from other tools | 🟢 Low | Live preview with the head pinned (R6.0.2) makes direction obvious; classic mode available (R6.0.6) |

### Phasing

| Phase | Scope | Exit criteria |
|---|---|---|
| **0 — Spike** | R1 share-visibility proof; nothing else | A second participant on a real Teams **and** Zoom call sees the overlay. **Hard gate — no further work until green.** |
| **1 — Skeleton** | Tauri shell, transparent full-monitor overlay, pass-through ⇄ draw state machine, panic hotkey, tray | Can toggle in/out of draw mode 100× with transparency and click-through intact every time |
| **2 — Annotate** | Arrow (head-first), rect, ellipse, line, pen, highlighter, text, step numbers, eraser; colours, widths, undo, clear; toolbar pill | A full demo walkthrough is possible using only hotkeys |
| **3 — Presenter extras** | Cursor highlight, click ripples, spotlight | Each toggles cleanly and composites correctly with annotations |
| **4 — Zoom** | Live magnifier (R6.11–R6.14) | 30fps at 4× within budget, or documented frozen-frame fallback |
| **5 — Polish & ship** | Settings UI, shortcut remapping, onboarding, startup/tray, installer | N1–N8 met; 60-minute call with no degradation |

---

## 10. Future / v2

- **Multi-monitor** — one overlay window per display, synchronised state, cursor-follow arming
- **Virtual camera output** — composite desktop + annotations into a virtual webcam/display so
  annotations survive window-share and work in any app (removes the §4 constraint entirely)
- **macOS support**
- **Saved stamps / reusable callouts** — pre-made badges, logos, "TODO" markers
- **Snapshot the annotated screen** to clipboard — the one Klipp feature worth re-importing
- **Window-anchored marks** — track the `HWND` under each mark and translate it when the window
  moves or resizes, so annotations survive window rearrangement during long walkthroughs (see R5.12)
- **Shape recognition** — rough circle snaps to a clean ellipse
- **Presenter laser pointer** — fading trail stroke, no persistence
- **Collaborative annotation** — participants draw back (needs a network layer; large)
- **Code signing + auto-update**

---

## 11. Resolved decisions

These were open at draft time and are now settled. Recorded here so the reasoning survives.

| # | Question | Decision | Where it lives |
|---|---|---|---|
| 1 | Auto-arm when a screen share is detected? | **No.** Hotkey/toolbar/tray only. Share detection is kept, but strictly advisory — its sole job is the one-time "you're sharing a window, not a screen" hint. Auto-arming would intercept clicks while the presenter is still setting up, violating G2. | R4.4, R5.6 |
| 2 | Should marks follow the window underneath them? | **No.** Marks are screen-anchored. Window tracking is real Win32 work that still breaks on scroll, and the actual usage loop is point → talk → clear. Parked as a v2 item. | R5.12, §10 |
| 3 | Licensing model? | **MIT, open-source, free.** Public repo, GitHub releases, no accounts, no licence keys, no paid tier — same posture as Klipp. The §2 non-goals against licensing servers and telemetry are therefore permanent, not provisional. | A6, §2 |
| 4 | Coexistence with Klipp? | **Not a concern.** The two won't run simultaneously. No shared tray, registry, or crate; Klipp is a source to copy from, not a dependency. | A5, §6.3 |

### Still genuinely open

- Exact detection heuristic for "this is a window share, not a monitor share" (R4.4) — may need
  the Phase 0 spike's findings before it can be specified.
- Whether the Phase 0 result forces the virtual-camera path into v1 (§4, R1).

---

## Appendix A — Reference repo map

Klipp lives at `D:\FuselabsAI\Projects\SnippingZo`. Files worth reading before Phase 1:

| Path | Why |
|---|---|
| `src-tauri/src/commands/overlay.rs` | Transparency, click-through, chrome compensation, mouse hook — the single most valuable file |
| `src/overlay/OverlayApp.tsx` | Transparent overlay React root, interactivity toggling via `set_overlay_interactive` |
| `src/hooks/useGlobalShortcut.ts` | Global shortcut register/unregister wrapper — portable as-is |
| `src/components/canvas/AnnotationCanvas.tsx` | Konva shape drawing, drag-to-create patterns |
| `src/stores/canvasStore.ts` | Zustand + Immer annotation state and undo history |
| `src-tauri/capabilities/overlay.json` | Tauri v2 permission scoping for the overlay window |
| `docs/plans/06-pill-buttons-blocked-by-overlay.md` | Why the toolbar is a separate window |
| `docs/plans/08-webview-content-drift-root-cause.md` | WebView2 content-shift gotcha |
| `docs/architecture/01-konvajs-canvas-choice.md` | Canvas library rationale |

<div align="center">

# Zemo

**Draw on your screen during a video call.**

A transparent, always-on-top annotation layer for Windows. Point at things with arrows, sketch
shapes, highlight and spotlight — live, on top of whatever you're already sharing. No recording,
no accounts, no vendor lock-in.

![License: MIT](https://img.shields.io/badge/license-MIT-blue)
![Status](https://img.shields.io/badge/status-pre--alpha-orange)
![Tauri v2](https://img.shields.io/badge/built%20with-Tauri%20v2-24C8DB?logo=tauri)
![Rust](https://img.shields.io/badge/backend-Rust-dea584?logo=rust&logoColor=white)
![React 19](https://img.shields.io/badge/frontend-React%2019-61DAFB?logo=react&logoColor=white)

</div>

---

> ⚠️ **Pre-alpha.** What exists today is the Phase 0 spike — a throwaway app built to answer one
> question that grew into a working annotation prototype. It is usable, not finished.
> See **[docs/PRD.md](docs/PRD.md)** for the product definition and the [roadmap](#roadmap) below.

## Download

Grab the [latest prerelease](https://github.com/alemhar/zemo/releases/latest):

| Artifact | Best for |
|---|---|
| **`Zemo_<version>_x64-setup.exe`** (NSIS) | Most people — small wizard, Start-menu shortcut, clean uninstall. Installs per-user, so **no admin rights needed**. |
| **`Zemo_<version>_x64_en-US.msi`** | IT admins — Group Policy, Intune, SCCM |
| **`Zemo.exe`** | No install at all — download and run from anywhere |

> **Windows will warn you on first run.** The builds are **unsigned**, so SmartScreen shows
> "Windows protected your PC". Click **"More info" → "Run anyway"**. This is expected for any
> unsigned binary and is not a sign anything is wrong — code signing is on the roadmap but needs a
> paid certificate.

---

## The problem

When you share your screen on a call, you lose the ability to *point at something*. You end up
saying "the button on the left… no, the other left… under the blue header", or wiggling your mouse
and hoping. Zemo gives you an arrow.

## Planned features

**Annotation** — arrow, rectangle, ellipse, line, freehand pen, highlighter, text, auto-numbered
step badges, object eraser. Eight colours, four stroke widths, undo/redo.

**Head-first arrows** — the first click places the arrow's *point*, the second places its tail.
Inverted from every other drawing app, on purpose: your cursor is already on the thing you're
talking about, so you shouldn't have to aim twice and think backwards mid-sentence. A classic
tail-first mode is available in settings.

**Presenter extras** — cursor halo with click ripples, spotlight (dim everything but one region),
and a live screen magnifier that keeps updating rather than freezing the frame.

**Stays out of your way** — the overlay is fully click-through until you press the hotkey. Marks
you've already drawn stay visible while you keep driving your apps normally. One panic hotkey
(`Ctrl+Alt+Q`) restores a clean desktop from any state.

## ⚠️ Important limitation

**Your annotations are only visible to other participants if you share an entire screen / monitor
— not a single window or application.**

This isn't a bug we can fix. Monitor-share captures the composited desktop, so the overlay is
included. Window-share captures one window's own surface, and Zemo's overlay is a different
window, so it's structurally absent from that stream. A virtual-camera output that would remove
this limitation is on the v2 list.

## Roadmap

| Phase | Scope | Status |
|---|---|---|
| **0** | Spike: confirm the overlay actually appears in a real Teams/Zoom monitor share | 🟡 [Built](spikes/00-share-visibility/) — local capture passes, live-call tests pending |
| **1** | Transparent full-monitor overlay, pass-through ⇄ draw state machine, tray | ⬜ |
| **2** | Annotation tools, colours, undo, toolbar pill | ⬜ |
| **3** | Cursor highlight, click ripples, spotlight | ⬜ |
| **4** | Live screen zoom | ⬜ |
| **5** | Settings, shortcut remapping, onboarding, installer | ⬜ |

Phase 0 is a hard gate — it validates the product's core premise before any feature work begins.

## Tech stack

Tauri v2 · Rust · React 19 · TypeScript · Konva · Zustand · Tailwind

The hard parts of the Windows overlay (WebView2 transparency via the `ICoreWebView2Controller2`
COM interface, and a click-through toggle that preserves `WS_EX_LAYERED`) are already solved in
[Klipp](https://github.com/alemhar/klipp), this project's sibling. See §8.3 of the PRD.

## Scope

Zemo draws pixels on a transparent window. That's the whole product. Explicitly **not** included:
screen recording, FFmpeg, webcam overlay, OCR, cloud sync, accounts, or telemetry.

**Windows 10/11 only.** A macOS port is perfectly feasible — all the UI code is portable, and the
platform layer is roughly a week's work (`NSWindow` levels, ScreenCaptureKit, the Screen Recording
permission flow, notarization). It isn't planned, because proving the concept properly on one
platform beats half-proving it on two. **If you want macOS, open an issue** — that's the signal
that would move it.

## Contributing

Pre-alpha. The two most useful things right now:

1. **Tell us whether it works on your calls.** Grab the
   [latest release](https://github.com/alemhar/zemo/releases/latest), share your whole screen on
   Teams, Zoom or Meet, draw an arrow, and report whether the other person could see it. That is
   the one question the project still cannot answer on its own.
2. **Feedback on [the PRD](docs/PRD.md)** — it is the product definition, open to argument.

## License

MIT

# Plan 01 — Phase 0: Share visibility spike

| | |
|---|---|
| **Status** | 🟡 Partially validated — local proxy passes, live-call tests pending |
| **Gate** | Hard. No Phase 1 work until the live-call tests are green. |
| **Code** | Promoted to the app itself — [`src-tauri/`](../../src-tauri/) and [`ui/`](../../ui/). The capture probe survives as `src-tauri/src/bin/capture_probe.rs`. |
| **PRD refs** | §4, §9 R1, R4.2 |

---

## Why this exists

Zemo's entire premise is that a transparent, always-on-top overlay drawn on the desktop shows up
in the video feed that call participants see. If it doesn't, the product does not work and the
architecture has to change to a virtual-camera approach.

[Klipp](https://github.com/alemhar/klipp) proves the overlay technique is captured by FFmpeg's
**gdigrab**. That is *a* display-capture path, but it is not the one Teams and Zoom use — they sit
on DXGI Desktop Duplication and/or `Windows.Graphics.Capture`. Those APIs have their own rules
about layered windows and display affinity. Until a second human on a real call confirms they can
see the overlay, the premise is an assumption.

This spike exists to turn that assumption into a fact before any feature code is written.

## What the spike does

A two-window throwaway Tauri app:

- **`overlay`** — frameless, transparent, always-on-top, `WS_EX_LAYERED`, click-through, sized to
  the full primary monitor. Explicitly **not** capture-excluded. Draws a red frame, corner
  brackets, a sweeping dot, a head-first arrow, and a clock ticking in milliseconds.
- **`control`** — a normal window carrying the test console. Capture-**excluded** via
  `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`, per PRD R4.2.

Three deliberate design choices make the result trustworthy:

| Choice | Why |
|---|---|
| **Millisecond clock** | A viewer seeing a frozen clock is looking at a stale frame or a thumbnail, not a live share. Distinguishes "overlay isn't captured" from "share isn't live". |
| **Toggleable overlay exclusion** | The **positive control**. If excluding the overlay makes it vanish for the viewer and un-excluding brings it back, the share is provably capturing our window — so a negative result is a real negative, not a misconfigured call. |
| **Remote colour change** | Presenter changes the accent colour and the viewer confirms over voice. Second, independent liveness check. |

## Running it

```bash
cd src-tauri
cargo run
```

No npm install, no Tauri CLI — the frontend is static HTML served from `ui/`.

> To screenshot the control panel for a bug report, launch with `ZEMO_SPIKE_NO_EXCLUDE=1` —
> capture exclusion blocks ordinary screenshot tools too, so the panel would otherwise come out
> blank or missing.
>
> **Quit with the panel's "Quit spike" button**, not Ctrl+C. Hard-killing the process leaves a
> stale lock in the WebView2 user-data folder, and the next launch fails with
> `0x800700AA "The requested resource is in use."` Recovery:
> `Remove-Item -Recurse -Force "$env:LOCALAPPDATA\com.alemhar.zemo.spike"`

## Invisible window chrome — and why the spike checks for it

On Windows a frameless window still has a non-client area: an **invisible resize border**,
typically ~8px on left/right/bottom and 0 on top. `set_position` positions the **outer** rect
(border included), but WebView2 renders inside the **inner** (client) rect. Ask for the window at
(0, 0) and the content can land at (8, 0) — the overlay ends up shifted off the monitor edge.

For a spike that is cosmetic. For Zemo it is a correctness bug: every annotation would land a few
pixels away from where the user pointed, and the screen-edge frame would be clipped.

The offset **varies by Windows version, DPI and theme**, so it cannot be hardcoded. The fix
(ported from Klipp) is to measure it per machine — `inner_position - outer_position` — and shift
the outer window back by that delta so the inner rect lands exactly on the monitor.

Measuring alone isn't enough, though: on a machine where compensation doesn't converge, a tester
would see a subtly misaligned overlay and have no idea anything was wrong. So the control panel
has an **Overlay geometry** card that verifies the *rendered* result rather than trusting the
compensation:

- reports the measured chrome offset, and says plainly when a machine has none
- reports the **per-edge gap** between the overlay and the monitor — all zeros means a true fit
- gives a pass/fail verdict, and on failure prints concrete remediation: re-fit, drop to 100%
  scaling, then record the machine's build/scale/gaps as a Phase 1 finding

**Run this card before going on a call.** A misaligned overlay invalidates the visibility result.

## Test protocol

Needs a second participant — a phone on the call is enough.

1. Launch the spike. In the **Overlay geometry** card, confirm the verdict reads `aligned` and all
   four edge gaps are `0`. If not, follow the remediation steps the card prints. Visually confirm
   the red frame touches all four screen edges.
2. Join a call and share **Screen / Entire Screen — not a window**.
3. **Test A — the overlay is visible.** Ask the viewer: *can you see a red frame and a counting
   clock?* Also ask whether the clock is **moving**.
4. **Positive control.** Press *Exclude overlay from capture*. The viewer should watch it vanish,
   then reappear when you turn it off. If it never vanishes, the share isn't capturing our window
   at all and Test A's result means nothing.
5. **Liveness.** Change the accent colour; the viewer confirms the new colour out loud.
6. **Test B — the control panel is hidden.** Ask whether they can see the dark control panel.
   They should **not**. Then toggle *Excluded from capture* off — it should appear for them,
   proving exclusion is doing real work rather than silently no-op'ing.
7. **Test C — window share (expected failure).** Switch to sharing a single window. The viewer
   should lose the overlay. This confirms the §4 limitation is real and worth documenting loudly.
8. Repeat for the other platform.

## Results

### Local proxy — GDI `BitBlt` / `CopyFromScreen` ✅

Run 2026-11-11 on Windows 10 Pro 19045, 1600×900 @ 100%. A full-screen GDI capture was taken
while the spike was running — the same class of capture path as gdigrab.

| Check | Result |
|---|---|
| Overlay present in the captured frame | ✅ frame, corners, headline, arrow and clock all captured |
| Overlay covers the full monitor | ✅ geometry check `aligned` — overlay 1600×900 @ (0,0) vs monitor 1600×900 @ (0,0), edge gaps L/T/R/B all 0 |
| Invisible window chrome | ✅ **none on this machine** (`dx=0 dy=0`) — compensation path untested here, so it still needs a machine that has an offset |
| Transparency genuinely per-pixel | ✅ desktop and terminal fully legible through the overlay |
| WebView2 transparency COM call | ✅ no `cast`/`SetDefaultBackgroundColor` errors logged |
| Control panel **absent** from the captured frame | ✅ `WDA_EXCLUDEFROMCAPTURE` held, despite the window being on-screen and always-on-top |
| `SetWindowDisplayAffinity` supported | ✅ build 10.0.19045.6456 ≥ 19041; logged `control panel excluded from capture` |

This is a genuine signal on both halves of R4.2 — the overlay is captured, the excluded window is
not — but **it is a proxy, not the answer.** GDI capture is not what Teams and Zoom use.

> Evidence screenshot is deliberately **not committed**: it is a capture of a live personal
> desktop, and this repo is public.

### Teams — monitor share ⬜ pending

| Test | Expected | Result |
|---|---|---|
| A — overlay visible to viewer | visible, clock moving | ⬜ |
| Positive control — exclusion hides it | vanishes, then returns | ⬜ |
| Liveness — colour change seen | confirmed by viewer | ⬜ |
| B — control panel hidden | not visible | ⬜ |
| C — window share | overlay NOT visible | ⬜ |

### Zoom — monitor share ⬜ pending

| Test | Expected | Result |
|---|---|---|
| A — overlay visible to viewer | visible, clock moving | ⬜ |
| Positive control — exclusion hides it | vanishes, then returns | ⬜ |
| Liveness — colour change seen | confirmed by viewer | ⬜ |
| B — control panel hidden | not visible | ⬜ |
| C — window share | overlay NOT visible | ⬜ |

### Google Meet (Chrome) ⬜ optional

Browser `getDisplayMedia` is a third capture path and worth one pass if a tester is handy.

## Decision rules

| Outcome | What it means |
|---|---|
| **Test A passes on both** | 🟢 Premise confirmed. Proceed to Phase 1 as specified in the PRD. |
| **Test A passes, Test B fails** | 🟡 Proceed, but R4.2 is unachievable on that platform — the toolbar will be visible to viewers. Redesign it to be tiny and unobtrusive, or move controls to hotkeys only. |
| **Test A fails on one platform** | 🟡 Ship for the platform that works, document the other. Reprioritise the virtual-camera path from §10 into v1.x. |
| **Test A fails on both** | 🔴 Stop. The overlay architecture cannot deliver the product. Virtual camera becomes the v1 architecture and the PRD needs a substantial rewrite. |

## Bugs found while building the spike

| Bug | Cause | Fix | Carry into Phase 1 |
|---|---|---|---|
| Control panel showed `no window labelled 'overlay'`; diagnostics stuck on "loading…" | The control window's scripts ran before Tauri had finished creating the overlay window — a startup race. | Overlay declared first in `tauri.conf.json`, **plus** a retry wrapper in the panel's JS, since creation order is not a contract. | Any cross-window command needs to tolerate the target not existing yet. Don't rely on config order. |
| Second launch failed with `0x800700AA "The requested resource is in use."` | A hard-killed process left a stale lock in the WebView2 user-data folder. | Delete `%LOCALAPPDATA%\com.alemhar.zemo.spike`. | The real app should use a per-launch user-data folder or shut its webviews down cleanly, or users will hit this after any crash. |

## Follow-ups regardless of outcome

- Feed the Test C result into the R4.4 detection heuristic — knowing exactly what a window share
  looks like from our side is what makes the "you're sharing a window" hint possible.
- The `0x800700AA` stale-lock failure is worth remembering for Phase 1: the real app should either
  use a per-launch user-data folder or shut down its webviews cleanly on exit.
- `set_click_through_preserving_layered` and `apply_webview_transparency` ported from Klipp without
  modification and worked first try. They are safe to lift wholesale into Phase 1.
- **The chrome-compensation path is still unproven**, because this machine reports a zero offset.
  Run the geometry card on a machine at 125%/150% scaling before trusting it — that is where a
  non-zero offset is most likely to appear.
- Promote the geometry check into the real app as a startup self-test: silently misplacing every
  annotation by 8px is exactly the kind of bug that ships unnoticed.

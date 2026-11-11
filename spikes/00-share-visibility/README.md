# Spike 00 — Share visibility

Throwaway Tauri app that answers one question: **does a transparent, always-on-top overlay
actually appear in a Teams/Zoom monitor share?** Everything in Zemo depends on yes.

```bash
cargo run
```

No npm install and no Tauri CLI needed — the frontend is static HTML in `ui/`.

Before going on a call, press **Check geometry** in the control panel. It verifies the overlay's
rendered area actually covers the monitor — frameless Windows windows have an invisible resize
border that shifts WebView2 content off the screen edge, and the amount varies by Windows version,
DPI and theme. The card reports the measured offset, the per-edge gap, and what to do if it's off.

To screenshot the control panel for a bug report, launch with `ZEMO_SPIKE_NO_EXCLUDE=1` — capture
exclusion blocks ordinary screenshot tools too.

**Quit using the control panel's "Quit spike" button.** Hard-killing the process leaves a stale
WebView2 lock and the next launch fails with `0x800700AA`. If that happens:

```powershell
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\com.alemhar.zemo.spike"
```

Full test protocol, decision rules, and the results table live in
[`docs/plans/01-share-visibility-spike.md`](../../docs/plans/01-share-visibility-spike.md).

## Files

| Path | What |
|---|---|
| `src/main.rs` | Everything. Win32 + WebView2 helpers ported from Klipp, plus the test commands. |
| `ui/overlay.html` | The transparent overlay — red frame, ms clock, sweeping dot, head-first arrow. |
| `ui/control.html` | Test console: toggles, colour swatches, diagnostics, the protocol. |

This is deliberately one Rust file and two HTML files. It is not a skeleton for the real app —
Phase 1 starts fresh. The parts worth lifting are `apply_webview_transparency` and
`set_click_through_preserving_layered`.

# macOS suggestion tooltip — spec

Status: draft → implemented (branch `feat/macos-popup`).
Tracks the "macOS | noop today" row in `crates/poltertype-popup/src/lib.rs`.

## Goal

Feature parity with the Windows/Linux suggestion tooltip on macOS:
the popup near the text being typed, spelling variants as clickable
rows, keyboard accept chord unchanged (engine-side, already
cross-platform), click-to-accept, hover highlight, self-hide timeout.

## Why it was missing

Two independent gaps, both ending in a noop:

1. `poltertype-popup` had no macOS backend — `create_for_platform`
   fell into the `#[cfg(not(any(linux, windows)))]` arm returning
   `NoopPopup`.
2. `poltertype-input`'s focus tracker had no macOS implementation
   (`NoopFocusTracker`), so even with a popup the anchor would always
   degrade to `ScreenBottom`.

This spec covers both.

## Non-negotiable invariants (from the crate docs)

- **Never take keyboard focus.** The user is mid-typing.
- **Never log the words being shown.**
- **The engine's hot path never blocks on window-system I/O** —
  `show`/`hide` are fire-and-forget.
- `#[cfg(target_os)]` stays inside the platform-island crates
  (`poltertype-popup`, `poltertype-input`).

## Design: the panel (`poltertype-popup/src/macos/`)

### Where the Windows/Linux guarantees map on macOS

| Guarantee | Windows | macOS |
|---|---|---|
| Never activated | `WS_EX_NOACTIVATE` | `NSPanel` + `NSWindowStyleMask.NonactivatingPanel` |
| Above the focused app | `WS_EX_TOPMOST` | `NSStatusWindowLevel` |
| Invisible to app switcher | `WS_EX_TOOLWINDOW` | `collectionBehavior`: `canJoinAllSpaces | stationary | ignoresCycle`; app is `LSUIElement` |
| Per-pixel alpha panel | `WS_EX_LAYERED` + `UpdateLayeredWindow` | borderless non-opaque window, pixels via `CALayer.contents` |
| Click without focus steal | hit-test in message loop | `mouseDown` on the content view — a non-activating panel delivers clicks without moving key focus |

### Threading model — different from Windows/Linux, deliberately

AppKit window objects may only be touched on the main thread, and the
app's main thread is owned by the tao event loop (`NSApplication`
runloop, which also drains the GCD main queue). So unlike the other
backends there is **no popup thread**:

- `MacosPopup` is a zero-sized handle; `show(model)` / `hide()` wrap
  the command in a closure and `dispatch2::DispatchQueue::main()
  .exec_async(...)` it. Fire-and-forget, engine never blocks — the
  invariant holds by construction.
- All state (panel, renderer, current model, row hit-boxes, deadline)
  lives in a main-thread `thread_local! RefCell<Option<PanelState>>`.
- Rendering (the shared CPU renderer in `crate::render`) runs inside
  the dispatched closure on the main thread. It is a ≤ 340 px panel
  of ≤ 9 rows — single-digit milliseconds, once per offer; not an
  animation path.
- The timeout is `DispatchQueue::main().exec_after(timeout, …)`
  guarded by the engine generation stamp; a stale timer fires into a
  generation mismatch and does nothing.
- Clicks/hover arrive as `NSView` callbacks on the main thread:
  hit-test against the stored row rects, send `PopupUiEvent` over the
  channel, hide via `orderOut`.

Panel creation is also dispatched async at `try_new` — `create_popup`
is called *before* `event_loop.run()`, so a synchronous hop to the
main queue would deadlock; async enqueue is correct because the main
queue is FIFO and any `show` dispatch lands after creation.

### Pixels

`crate::render` produces premultiplied RGBA — exactly
`kCGBitmapByteOrder32Big | kCGImageAlphaPremultipliedLast`, no channel
swap needed. The frame becomes a `CGImage` set as the content view's
layer `contents` with `contentsScale = backingScaleFactor` of the
screen the anchor lands on (Retina = 2.0, matching the Windows
per-monitor DPI path).

### Coordinates

macOS has two global spaces. We standardise on the **Core Graphics /
AX space (top-left origin, y down, logical points)** for everything
internal — anchors arrive that way from the AX focus tracker, and
`CGDisplay` bounds/union drive placement via the shared
`crate::place::place_near_point`. The single conversion to AppKit's
bottom-left space happens at `setFrame` time:
`appkit_y = primary_screen_height − cg_y − height`.

Placement mirrors the Windows backend: `Point` → shared side-picker
above/below/right/left the caret; `WindowRect` → bottom-centre, 96 px
above the window's bottom edge; `ScreenBottom` → bottom-centre of the
union; all clamped to the display union.

### Subclassing

One `objc2::define_class!` NSView subclass (`PopupView`) with ivars
for the event channel, the current row rects (logical px), and the
generation. Overrides: `isFlipped` (top-down coordinates so the shared
hit-test works unmodified), `mouseDown`, `mouseMoved`, `mouseExited`,
`updateTrackingAreas` (a visible-rect tracking area — borderless
windows get no motion events otherwise).

## Design: the focus tracker (`poltertype-input/src/focus/macos_impl.rs`)

Raw FFI to HIServices (`ApplicationServices` framework), same pattern
as the existing `macos/listener.rs` FFI glue — no new crate deps
beyond `libc` (for `proc_pidpath`):

- `AXUIElementCreateSystemWide` → `kAXFocusedApplicationAttribute`
  → `AXUIElementGetPid` + `proc_pidpath` ⇒ `focused_exe`.
- Focused app element → `kAXFocusedWindowAttribute` →
  `kAXPositionAttribute` / `kAXSizeAttribute` (AXValue-wrapped
  `CGPoint`/`CGSize`) ⇒ `focused_window_geometry`. `output`/`output_*`
  are `None`/`0` — that field is Wayland-only by contract.
- System-wide `kAXFocusedUIElementAttribute` →
  `kAXSelectedTextRangeAttribute` (AXValue `CFRange`) → parameterized
  `kAXBoundsForRangeParameterizedAttribute` ⇒ caret `CGRect` in global
  CG coordinates; converted to window-relative for `CaretHint`
  (`age = 0` — it is a live query, not a cached sample).

Accessibility permission is already a hard requirement of the app
(the `CGEventTap` listener needs it), so these calls either work or
return `kAXErrorAPIDisabled`, which we map to `None` — the anchor
chain then degrades exactly as it does on GNOME Wayland.

Every AX object is `CFRelease`d; no ObjC objects cross threads
(the tracker methods are called on the engine thread and are
self-contained).

## Files touched

- `crates/poltertype-popup/Cargo.toml` — macOS target deps:
  `objc2 0.6`, `objc2-foundation 0.3`, `objc2-app-kit 0.3`,
  `block2 0.6`, `dispatch2 0.3`, `core-graphics 0.25`
  (all already in the lockfile via tao / poltertype-input).
- `crates/poltertype-popup/src/macos/{mod,panel,popup}.rs` — new.
- `crates/poltertype-popup/src/lib.rs` — `mod macos`, platform table,
  `place`/`render` cfg widened to `any(linux, windows, macos)`.
- `crates/poltertype-popup/src/factory.rs` — macOS arm.
- `crates/poltertype-input/src/focus/{mod,factory}.rs` — macOS arm.
- `crates/poltertype-input/src/focus/macos_impl.rs` — new.
- `crates/poltertype-input/Cargo.toml` — `libc` for macOS.

## Verification

- `cargo check --target x86_64-apple-darwin -p poltertype-popup -p poltertype-input`
  (the macOS CI lane does the same on aarch64).
- `cargo clippy --workspace --all-targets` + `cargo test --workspace`
  on Linux — no regressions outside the macOS islands.
- Runtime verification on a Mac: typing a known typo shows the panel
  above the caret; click accepts; timeout self-hides; focus never
  leaves the editor (type-through test).

## Explicitly out of scope

- Trackpad/mouse-wheel scrolling inside the panel (≤ 9 rows fit).
- Caret anchoring in apps that expose no AX text attributes — falls
  back to window-bottom, by design.
- Animated show/hide.

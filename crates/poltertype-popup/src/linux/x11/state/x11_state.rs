//! The `X11State` struct: connection, visual and window state.
//! Behaviour lives in the sibling files, one `impl` per concern.

use crossbeam_channel::Sender;
use tracing::warn;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, ColormapAlloc, ConnectionExt, ImageOrder, VisualClass, Window,
};
use x11rb::rust_connection::RustConnection;

use crate::enums::PopupUiEvent;
use crate::renderer::Renderer;

use super::types::{Atoms, VisualPick, WinView};

pub(super) struct X11State {
    pub(super) conn: RustConnection,
    pub(super) root: Window,
    pub(super) screen_w: u16,
    pub(super) screen_h: u16,
    pub(super) visual: VisualPick,
    pub(super) lsb_first: bool,
    pub(super) atoms: Option<Atoms>,
    pub(super) events: Sender<PopupUiEvent>,
    pub(super) renderer: Renderer,
    pub(super) win: Option<WinView>,
}

impl X11State {
    pub(super) fn new(
        conn: RustConnection,
        screen_num: usize,
        events: Sender<PopupUiEvent>,
    ) -> Option<Self> {
        let setup = conn.setup();
        let lsb_first = setup.image_byte_order == ImageOrder::LSB_FIRST;
        let screen = setup.roots.get(screen_num)?;
        let (root, screen_w, screen_h) =
            (screen.root, screen.width_in_pixels, screen.height_in_pixels);
        let (root_visual, root_depth) = (screen.root_visual, screen.root_depth);

        // Prefer a 32-bit TrueColor visual (real transparency); fall
        // back to the root visual and an opaque panel.
        let argb_visual = screen
            .allowed_depths
            .iter()
            .filter(|d| d.depth == 32)
            .flat_map(|d| d.visuals.iter())
            .find(|v| v.class == VisualClass::TRUE_COLOR)
            .map(|v| v.visual_id);
        // We upload 4 bytes per pixel; every current server advertises
        // 32 bpp for both depth-24 and depth-32 ZPixmaps.
        let bpp_ok = |depth: u8| {
            setup
                .pixmap_formats
                .iter()
                .any(|f| f.depth == depth && f.bits_per_pixel == 32)
        };
        let visual = match argb_visual {
            Some(visual) if bpp_ok(32) => {
                let colormap = conn.generate_id().ok()?;
                conn.create_colormap(ColormapAlloc::NONE, colormap, root, visual)
                    .ok()?;
                VisualPick {
                    depth: 32,
                    visual,
                    colormap: Some(colormap),
                    argb: true,
                }
            }
            _ => {
                if !bpp_ok(root_depth) {
                    warn!(
                        depth = root_depth,
                        "no 32-bpp ZPixmap format for root depth"
                    );
                    return None;
                }
                VisualPick {
                    depth: root_depth,
                    visual: root_visual,
                    colormap: None,
                    argb: false,
                }
            }
        };

        let atoms = resolve_atoms(&conn);
        Some(Self {
            conn,
            root,
            screen_w,
            screen_h,
            visual,
            lsb_first,
            atoms,
            events,
            renderer: Renderer::new(),
            win: None,
        })
    }
}

fn resolve_atoms(conn: &RustConnection) -> Option<Atoms> {
    let intern = |name: &str| -> Option<Atom> {
        conn.intern_atom(false, name.as_bytes())
            .ok()?
            .reply()
            .ok()
            .map(|r| r.atom)
    };
    Some(Atoms {
        window_type: intern("_NET_WM_WINDOW_TYPE")?,
        window_type_tooltip: intern("_NET_WM_WINDOW_TYPE_TOOLTIP")?,
        wm_state: intern("_NET_WM_STATE")?,
        wm_state_above: intern("_NET_WM_STATE_ABOVE")?,
    })
}

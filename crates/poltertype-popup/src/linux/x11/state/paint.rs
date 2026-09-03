//! Uploading the rendered pixmap to the server — one `PutImage` per
//! paint, since the popup is tiny.

use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};

use super::x11_state::X11State;

/// Opaque panel background for servers without a 32-bit ARGB visual.
const OPAQUE_BG: (u8, u8, u8) = (0x16, 0x16, 0x1E);

impl X11State {
    /// Upload the rendered pixmap. One `PutImage` — the popup is tiny.
    pub(super) fn paint(&self) -> Result<(), ReplyOrIdError> {
        let Some(view) = &self.win else {
            return Ok(());
        };
        let w = view.rendered.pixmap.width() as u16;
        let h = view.rendered.pixmap.height() as u16;
        let data = self.upload_bytes(&view.rendered.pixmap);
        self.conn.put_image(
            ImageFormat::Z_PIXMAP,
            view.window,
            view.gc,
            w,
            h,
            0,
            0,
            0,
            self.visual.depth,
            &data,
        )?;
        Ok(())
    }

    /// tiny-skia premultiplied RGBA → server pixel bytes.
    ///
    /// 32-bit ARGB visuals conventionally expect premultiplied alpha
    /// when a compositor runs, so the bytes pass through reordered.
    /// For 24-bit, composite over the opaque panel colour first.
    fn upload_bytes(&self, pixmap: &tiny_skia::Pixmap) -> Vec<u8> {
        let src = pixmap.data();
        let mut out = Vec::with_capacity(src.len());
        for px in src.chunks_exact(4) {
            let (r, g, b, a) = (px[0], px[1], px[2], px[3]);
            let (r, g, b, a) = if self.visual.argb {
                (r, g, b, a)
            } else {
                let inv = 255 - u16::from(a);
                (
                    b_add(r, OPAQUE_BG.0, inv),
                    b_add(g, OPAQUE_BG.1, inv),
                    b_add(b, OPAQUE_BG.2, inv),
                    0,
                )
            };
            if self.lsb_first {
                out.extend_from_slice(&[b, g, r, a]);
            } else {
                out.extend_from_slice(&[a, r, g, b]);
            }
        }
        out
    }
}

/// `src + bg × inv / 255` for one premultiplied channel.
fn b_add(src: u8, bg: u8, inv: u16) -> u8 {
    src.saturating_add(((u16::from(bg) * inv + 127) / 255) as u8)
}

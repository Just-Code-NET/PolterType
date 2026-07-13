//! The PolterType mark — an indigo keycap haunted by a small ghost —
//! as an `iced::canvas` program. A 1:1 port of the site's
//! `GhostMark.astro` SVG (viewBox `0 0 64 64`, relative path
//! commands resolved to absolute points), so the window and
//! poltertype.com show the same face. Vector, so it stays crisp at
//! any scale / DPI without pulling an SVG renderer into the binary.

use iced::mouse::Cursor;
use iced::widget::canvas::{self, Frame, Geometry, LineCap, Path, Stroke};
use iced::{Point, Rectangle, Renderer, Size, Theme};

use super::consts::{MARK_FACE, MARK_GHOST, MARK_KEYCAP_FACE, MARK_KEYCAP_TOP};

/// Canvas program painting the mark into its bounds. Use as
/// `Canvas::new(GhostMark).width(n).height(n)` — the drawing scales
/// to the smaller of the two dimensions.
pub struct GhostMark;

impl<Message> canvas::Program<Message> for GhostMark {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        // An INFINITE frame size instead of the idiomatic
        // `bounds.size()` works around an iced 0.13 tiny-skia bug:
        // the compositor treats this frame's clip rectangle — which
        // the frame stores in canvas-LOCAL coordinates, i.e.
        // `(0, 0, w, h)` — as a window-GLOBAL mask region. Any canvas
        // sitting away from the window's top-left corner gets its
        // curves masked out (whether a given fill survives depends on
        // exact-f32 bounds equality, which is why the keycap
        // rectangles rendered while the ghost vanished). An infinite
        // clip degenerates the mask to the enclosing layer's own clip
        // (`Item::Live` uses `Rectangle::INFINITE` the same way), so
        // scissoring stays correct — we only lose clipping to the
        // canvas box itself, which this drawing never exceeds.
        // Upstream reworked the pipeline in iced 0.14; drop this when
        // the workspace moves.
        let mut frame = Frame::new(renderer, Size::INFINITY);
        // Author coordinates below are the SVG's 64×64 viewBox.
        frame.scale(bounds.width.min(bounds.height) / 64.0);

        // Keycap: side layer peeking out below the top layer gives the
        // "physical key" depth, then the inner face inset on top.
        frame.fill(
            &Path::rounded_rectangle(Point::new(2.0, 6.0), Size::new(60.0, 54.0), 12.0.into()),
            MARK_KEYCAP_FACE,
        );
        frame.fill(
            &Path::rounded_rectangle(Point::new(2.0, 2.0), Size::new(60.0, 54.0), 12.0.into()),
            MARK_KEYCAP_TOP,
        );
        frame.fill(
            &Path::rounded_rectangle(Point::new(6.0, 6.0), Size::new(52.0, 46.0), 9.0.into()),
            MARK_KEYCAP_FACE,
        );

        // Ghost body: domed head, wavy skirt.
        let ghost = Path::new(|b| {
            b.move_to(Point::new(32.0, 14.0));
            b.bezier_curve_to(
                Point::new(22.6, 14.0),
                Point::new(17.0, 21.2),
                Point::new(17.0, 30.2),
            );
            b.line_to(Point::new(17.0, 44.0));
            b.bezier_curve_to(
                Point::new(17.0, 45.8),
                Point::new(19.0, 46.4),
                Point::new(20.4, 45.4),
            );
            b.line_to(Point::new(23.2, 43.4));
            b.line_to(Point::new(26.6, 46.0));
            b.bezier_curve_to(
                Point::new(27.5, 46.7),
                Point::new(28.7, 46.7),
                Point::new(29.6, 46.0),
            );
            b.line_to(Point::new(32.0, 44.0));
            b.line_to(Point::new(34.4, 46.0));
            b.bezier_curve_to(
                Point::new(35.3, 46.7),
                Point::new(36.5, 46.7),
                Point::new(37.4, 46.0),
            );
            b.line_to(Point::new(40.8, 43.4));
            b.line_to(Point::new(43.6, 45.4));
            b.bezier_curve_to(
                Point::new(45.0, 46.4),
                Point::new(47.0, 45.8),
                Point::new(47.0, 44.0),
            );
            b.line_to(Point::new(47.0, 30.2));
            b.bezier_curve_to(
                Point::new(47.0, 21.2),
                Point::new(41.4, 14.0),
                Point::new(32.0, 14.0),
            );
            b.close();
        });
        frame.fill(&ghost, MARK_GHOST);

        // Face: two eyes, one smile.
        frame.fill(&Path::circle(Point::new(26.5, 30.0), 2.6), MARK_FACE);
        frame.fill(&Path::circle(Point::new(37.5, 30.0), 2.6), MARK_FACE);
        let smile = Path::new(|b| {
            b.move_to(Point::new(29.5, 36.5));
            b.bezier_curve_to(
                Point::new(31.1, 37.8),
                Point::new(32.9, 37.8),
                Point::new(34.5, 36.5),
            );
        });
        frame.stroke(
            &smile,
            Stroke {
                style: canvas::stroke::Style::Solid(MARK_FACE),
                width: 2.0,
                line_cap: LineCap::Round,
                ..Stroke::default()
            },
        );

        vec![frame.into_geometry()]
    }
}

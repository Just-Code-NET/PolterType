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
        // Canvas-local, which is what a canvas program is supposed to
        // draw in. Under iced 0.13 this was not usable: the tiny-skia
        // compositor took the frame's canvas-local clip and applied it
        // as a window-global mask, so any canvas away from the window's
        // top-left had its fills masked away — the mark drew as an
        // empty square. The workaround clipped to window-global bounds
        // and cancelled the translation the frame baked in.
        //
        // The idiomatic frame, which iced 0.13 could not be given: its
        // tiny-skia compositor took the canvas-local clip and applied
        // it as a window-global mask, so any canvas away from the
        // window's top-left drew as an empty square, and the mark went
        // through a raw frame clipped to window-global bounds instead.
        // 0.14 fixed the mask, and all three raw-frame spellings were
        // checked on screen before this one: they draw the mark at the
        // window's origin, or masked down to a sliver, or not at all.
        let mut frame = Frame::new(renderer, bounds.size());
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

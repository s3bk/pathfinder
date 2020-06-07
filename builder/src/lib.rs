pub use pathfinder_geometry::{
    vector::Vector2F,
    transform2d::Transform2F,
    rect::RectF,
};
pub use pathfinder_content::{
    outline::{Outline, ArcDirection, Contour},
};

#[derive(Copy, Clone)]
enum PathState {
    // nothing has ben drawn yet. only move_to is valid
    Empty,

    // we have a starting point, but it is not connected to a previous path
    Start(Vector2F),

    // out starting point is the end of the last path
    End(Vector2F)
}

#[derive(Clone)]
pub struct PathBuilder {
    outline: Outline,
    contour: Contour,
    state: PathState,
}
impl PathBuilder {
    #[inline]
    pub fn new() -> Self {
        PathBuilder {
            outline: Outline::new(),
            contour: Contour::new(),
            state: PathState::Empty
        }
    }

    #[inline]
    fn start(&mut self) {
        match self.state {
            PathState::Empty => panic!("no starting point set. call move_to first"),
            PathState::Start(p) => {
                // copy the contour instead of allocating a new buffer with unknown size each time
                // that way we reuse one buffer for each contour (of unknown length) and only need one allocation per contour
                // (instead of growing and reallocating every contour a bunch of times)
                if !self.contour.is_empty() {
                    self.outline.push_contour(self.contour.clone());
                    self.contour.clear();
                }
                self.contour.push_endpoint(p);
            }
            PathState::End(_) => {}
        }
    }

    #[inline]
    pub fn move_to(&mut self, p: Vector2F) {
        self.state = PathState::Start(p);
    }
    #[inline]
    pub fn line_to(&mut self, p: Vector2F) {
        self.start();
        self.contour.push_endpoint(p);
        self.state = PathState::End(p);
    }
    #[inline]
    pub fn quadratic_curve_to(&mut self, c: Vector2F, p: Vector2F) {
        self.start();
        self.contour.push_quadratic(c, p);
        self.state = PathState::End(p);
    }
    #[inline]
    pub fn cubic_curve_to(&mut self, c1: Vector2F, c2: Vector2F, p: Vector2F) {
        self.start();
        self.contour.push_cubic(c1, c2, p);
        self.state = PathState::End(p);
    }
    #[inline]
    pub fn rect(&mut self, rect: RectF) {
        self.move_to(rect.origin());
        self.line_to(rect.upper_right());
        self.line_to(rect.lower_right());
        self.line_to(rect.lower_left());
        self.close();
        self.state = PathState::End(rect.lower_left());
    }
    #[inline]
    pub fn circle(&mut self, center: Vector2F, radius: f32) {
        self.ellipse(center, Vector2F::splat(radius), 0.0);
    }
    #[inline]
    pub fn ellipse(&mut self, center: Vector2F, radius: Vector2F, phi: f32) {
        let transform = Transform2F::from_translation(center)
            * Transform2F::from_rotation(phi)
            * Transform2F::from_scale(radius);
        self.contour.push_arc(&transform, 0.0, 2.0 * core::f32::consts::PI, ArcDirection::CCW);
        self.contour.close();
    }
    #[inline]
    pub fn close(&mut self) {
        self.contour.close();
    }
    #[inline]
    pub fn into_outline(mut self) -> Outline {
        if !self.contour.is_empty() {
            self.outline.push_contour(self.contour);
        }
        self.outline
    }
    #[inline]
    pub fn take(&mut self) -> Outline {
        if !self.contour.is_empty() {
            self.outline.push_contour(self.contour.clone());
            self.contour.clear();
        }
        
        let outline = self.outline.clone();
        self.outline.clear();

        self.state = match self.state {
            PathState::End(p) => PathState::Start(p),
            s => s
        };

        outline
    }
    #[inline]
    pub fn clear(&mut self) {
        self.contour.clear();
        self.outline.clear();
        self.state = PathState::Empty;
    }

    #[inline]
    pub fn pos(&self) -> Option<Vector2F> {
        match self.state {
            PathState::Empty => None,
            PathState::Start(p) => Some(p),
            PathState::End(p) => Some(p)
        }
    }
}

#[derive(Copy, Clone)]
enum DrawMode {
    None,
    Fill(PaintId),
    Stroke(PaintId, StrokeStyle),
    StrokeThenFill(PaintId, StrokeStyle, PaintId),
    FillThenStroke(PaintId, PaintId, StrokeStyle)
}

pub struct PathStyle {
    mode: DrawMode,
    fill_rule: FillRule
}
impl PathStyle {
    pub fn draw(&self, scene: &mut Scene, path: Outline, clip: Option<ClipPathId>) {
        let style = self;
        let build_stroke = |path, paint, stroke| {
            let mut stroke_to_fill = OutlineStrokeToFill::new(path, stroke);
            stroke_to_fill.offset();
            let outline = stroke_to_fill.into_outline();
            let mut draw_path = DrawPath::new(outline, paint);
            draw_path.set_fill_rule(style.fill_rule);
            draw_path.set_clip_path(clip);
            draw_path
        };
        let build_fill = |path, paint| {
            let mut draw_path = DrawPath::new(path, paint);
            draw_path.set_fill_rule(style.fill_rule);
            draw_path.set_clip_path(clip);
            draw_path
        };
        
        match style.mode {
            DrawMode::None => {},
            DrawMode::Fill(paint) => {
                scene.push_draw_path(build_fill(path, paint));
            }
            DrawMode::Stroke(paint, stroke) => {
                scene.push_draw_path(build_stroke(&path, paint, stroke));
            }
            DrawMode::FillThenStroke(fill_paint, stroke_paint, stroke) => {
                scene.push_draw_path(build_fill(path.clone(), fill_paint));
                scene.push_draw_path(build_stroke(&path, stroke_paint, stroke));
            }
            DrawMode::StrokeThenFill(fill_paint, stroke, stroke_paint) => {
                scene.push_draw_path(build_stroke(&path, stroke_paint, stroke));
                scene.push_draw_path(build_fill(path, fill_paint));
            }
        }
    }
}
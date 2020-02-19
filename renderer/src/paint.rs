// pathfinder/renderer/src/paint.rs
//
// Copyright © 2020 The Pathfinder Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::allocator::{TextureAllocator, TextureLocation};
use crate::gpu_data::PaintData;
use crate::tiles::{TILE_HEIGHT, TILE_WIDTH};
use hashbrown::HashMap;
use pathfinder_color::ColorU;
use pathfinder_content::gradient::{Gradient, GradientGeometry};
use pathfinder_content::pattern::Pattern;
use pathfinder_geometry::rect::{RectF, RectI};
use pathfinder_geometry::transform2d::{Matrix2x2F, Transform2F};
use pathfinder_geometry::util;
use pathfinder_geometry::vector::{Vector2F, Vector2I};
use pathfinder_simd::default::F32x4;
use std::fmt::{self, Debug, Formatter};

const INITIAL_PAINT_TEXTURE_LENGTH: u32 = 1024;

// The size of a gradient tile.
//
// TODO(pcwalton): Choose this size dynamically!
const GRADIENT_TILE_LENGTH: u32 = 256;

const SOLID_COLOR_TILE_LENGTH: u32 = 16;
const MAX_SOLID_COLORS_PER_TILE: u32 = SOLID_COLOR_TILE_LENGTH * SOLID_COLOR_TILE_LENGTH;

#[derive(Clone)]
pub struct Palette {
    pub(crate) paints: Vec<Paint>,
    cache: HashMap<Paint, PaintId>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Paint {
    Color(ColorU),
    Gradient(Gradient),
    Pattern(Pattern),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PaintId(pub u16);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct GradientId(pub u32);

impl Debug for Paint {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match *self {
            Paint::Color(color) => color.fmt(formatter),
            Paint::Gradient(_) => {
                // TODO(pcwalton)
                write!(formatter, "(gradient)")
            }
            Paint::Pattern(ref pattern) => pattern.fmt(formatter),
        }
    }
}

impl Palette {
    #[inline]
    pub fn new() -> Palette {
        Palette { paints: vec![], cache: HashMap::new() }
    }
}

impl Paint {
    #[inline]
    pub fn black() -> Paint {
        Paint::Color(ColorU::black())
    }

    #[inline]
    pub fn transparent_black() -> Paint {
        Paint::Color(ColorU::transparent_black())
    }

    pub fn is_opaque(&self) -> bool {
        match *self {
            Paint::Color(color) => color.is_opaque(),
            Paint::Gradient(ref gradient) => {
                gradient.stops().iter().all(|stop| stop.color.is_opaque())
            }
            Paint::Pattern(ref pattern) => pattern.image.is_opaque(),
        }
    }

    pub fn is_fully_transparent(&self) -> bool {
        match *self {
            Paint::Color(color) => color.is_opaque(),
            Paint::Gradient(ref gradient) => {
                gradient.stops().iter().all(|stop| stop.color.is_fully_transparent())
            }
            Paint::Pattern(_) => {
                // TODO(pcwalton): Should we support this?
                false
            }
        }
    }

    #[inline]
    pub fn is_color(&self) -> bool {
        match *self {
            Paint::Color(_) => true,
            Paint::Gradient(_) | Paint::Pattern(_) => false,
        }
    }

    pub fn set_opacity(&mut self, alpha: f32) {
        if alpha == 1.0 {
            return;
        }

        match *self {
            Paint::Color(ref mut color) => color.a = (color.a as f32 * alpha).round() as u8,
            Paint::Gradient(ref mut gradient) => gradient.set_opacity(alpha),
            Paint::Pattern(ref mut pattern) => pattern.image.set_opacity(alpha),
        }
    }

    pub fn apply_transform(&mut self, transform: &Transform2F) {
        if transform.is_identity() {
            return;
        }

        match *self {
            Paint::Color(_) => {}
            Paint::Gradient(ref mut gradient) => {
                match *gradient.geometry_mut() {
                    GradientGeometry::Linear(ref mut line) => {
                        *line = *transform * *line;
                    }
                    GradientGeometry::Radial {
                        ref mut line,
                        ref mut start_radius,
                        ref mut end_radius,
                    } => {
                        *line = *transform * *line;

                        // FIXME(pcwalton): This is wrong; I think the transform can make the
                        // radial gradient into an ellipse.
                        *start_radius *= util::lerp(transform.matrix.m11(),
                                                    transform.matrix.m22(),
                                                    0.5);
                        *end_radius *= util::lerp(transform.matrix.m11(),
                                                  transform.matrix.m22(),
                                                  0.5);
                    }
                }
            }
            Paint::Pattern(_) => {
                // TODO(pcwalton): Implement this.
            }
        }
    }
}

pub struct PaintInfo {
    /// The data that is sent to the renderer.
    pub data: PaintData,
    /// The metadata for each paint.
    ///
    /// The indices of this vector are paint IDs.
    pub metadata: Vec<PaintMetadata>,
}

// TODO(pcwalton): Add clamp/repeat options.
#[derive(Debug)]
pub struct PaintMetadata {
    /// The rectangle within the texture atlas.
    pub tex_rect: RectI,
    /// The transform to apply to screen coordinates to translate them into UVs.
    pub tex_transform: Transform2F,
    /// True if this paint is fully opaque.
    pub is_opaque: bool,
}

impl Palette {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn push_paint(&mut self, paint: &Paint) -> PaintId {
        if let Some(paint_id) = self.cache.get(paint) {
            return *paint_id;
        }

        let paint_id = PaintId(self.paints.len() as u16);
        self.cache.insert((*paint).clone(), paint_id);
        self.paints.push((*paint).clone());
        paint_id
    }

    pub fn build_paint_info(&self, view_box_size: Vector2I) -> PaintInfo {
        let mut allocator = TextureAllocator::new(INITIAL_PAINT_TEXTURE_LENGTH);
        let mut metadata = vec![];

        // Assign paint locations.
        let mut solid_color_tile_builder = SolidColorTileBuilder::new();
        for paint in &self.paints {
            let tex_location = match paint {
                Paint::Color(_) => solid_color_tile_builder.allocate(&mut allocator),
                Paint::Gradient(_) => {
                    // TODO(pcwalton): Optimize this:
                    // 1. Use repeating/clamp on the sides.
                    // 2. Choose an optimal size for the gradient that minimizes memory usage while
                    //    retaining quality.
                    allocator.allocate(Vector2I::splat(GRADIENT_TILE_LENGTH as i32))
                             .expect("Failed to allocate space for the gradient!")
                }
                Paint::Pattern(ref pattern) => {
                    allocator.allocate(pattern.image.size())
                             .expect("Failed to allocate space for the image!")
                }
            };

            metadata.push(PaintMetadata {
                tex_rect: tex_location.rect,
                tex_transform: Transform2F::default(),
                is_opaque: paint.is_opaque(),
            });
        }

        // Calculate texture transforms.
        let texture_length = allocator.size();
        let texture_scale = allocator.scale();
        for (paint, metadata) in self.paints.iter().zip(metadata.iter_mut()) {
            metadata.tex_transform = match paint {
                Paint::Color(_) => {
                    let vector = rect_to_inset_uv(metadata.tex_rect, texture_length).origin();
                    Transform2F { matrix: Matrix2x2F(F32x4::default()), vector }
                }
                Paint::Gradient(_) => {
                    let texture_origin_uv = rect_to_uv(metadata.tex_rect, texture_length).origin();
                    let gradient_tile_scale = GRADIENT_TILE_LENGTH as f32 * texture_scale;
                    Transform2F::from_translation(texture_origin_uv) *
                        Transform2F::from_scale(Vector2F::splat(gradient_tile_scale) /
                                                view_box_size.to_f32())
                }
                Paint::Pattern(_) => {
                    let texture_origin_uv = rect_to_uv(metadata.tex_rect, texture_length).origin();
                    Transform2F::from_translation(texture_origin_uv) *
                        Transform2F::from_uniform_scale(texture_scale)
                }
            }
        }

        // Render the actual texels.
        //
        // TODO(pcwalton): This is slow. Do more on GPU.
        let texture_area = texture_length as usize * texture_length as usize;
        let mut texels = vec![ColorU::default(); texture_area];
        for (paint, metadata) in self.paints.iter().zip(metadata.iter()) {
            match paint {
                Paint::Color(color) => {
                    put_pixel(metadata.tex_rect.origin(), *color, &mut texels, texture_length);
                }
                Paint::Gradient(ref gradient) => {
                    self.render_gradient(gradient,
                                         metadata.tex_rect,
                                         &metadata.tex_transform,
                                         &mut texels,
                                         texture_length);
                }
                Paint::Pattern(ref pattern) => {
                    self.render_pattern(pattern, metadata.tex_rect, &mut texels, texture_length);
                }
            }
        }

        let size = Vector2I::splat(texture_length as i32);
        return PaintInfo { data: PaintData { size, texels }, metadata };
    }

    // TODO(pcwalton): This is slow. Do on GPU instead.
    fn render_gradient(&self,
                       gradient: &Gradient,
                       tex_rect: RectI,
                       tex_transform: &Transform2F,
                       texels: &mut [ColorU],
                       texture_length: u32) {
        match *gradient.geometry() {
            GradientGeometry::Linear(gradient_line) => {
                // FIXME(pcwalton): Paint transparent if gradient line has zero size, per spec.
                let gradient_line = *tex_transform * gradient_line;

                // TODO(pcwalton): Optimize this:
                // 1. Calculate ∇t up front and use differencing in the inner loop.
                // 2. Go four pixels at a time with SIMD.
                for y in 0..(GRADIENT_TILE_LENGTH as i32) {
                    for x in 0..(GRADIENT_TILE_LENGTH as i32) {
                        let point = tex_rect.origin() + Vector2I::new(x, y);
                        let vector = point.to_f32().scale(1.0 / texture_length as f32) -
                            gradient_line.from();

                        let mut t = gradient_line.vector().projection_coefficient(vector);
                        t = util::clamp(t, 0.0, 1.0);

                        put_pixel(point, gradient.sample(t), texels, texture_length);
                    }
                }
            }

            GradientGeometry::Radial { line: gradient_line, start_radius, end_radius } => {
                // FIXME(pcwalton): Paint transparent if line has zero size and radii are equal,
                // per spec.
                let tex_transform_inv = tex_transform.inverse();

                // FIXME(pcwalton): This is not correct. Follow the spec.
                let center = gradient_line.midpoint();

                // TODO(pcwalton): Optimize this:
                // 1. Calculate ∇t up front and use differencing in the inner loop, if possible.
                // 2. Go four pixels at a time with SIMD.
                for y in 0..(GRADIENT_TILE_LENGTH as i32) {
                    for x in 0..(GRADIENT_TILE_LENGTH as i32) {
                        let point = tex_rect.origin() + Vector2I::new(x, y);
                        let vector = tex_transform_inv *
                            point.to_f32().scale(1.0 / texture_length as f32);

                        let t = util::clamp((vector - center).length(), start_radius, end_radius) /
                            (end_radius - start_radius);

                        put_pixel(point, gradient.sample(t), texels, texture_length);
                    }
                }
            }
        }
    }

    fn render_pattern(&self,
                      pattern: &Pattern,
                      tex_rect: RectI,
                      texels: &mut [ColorU],
                      texture_length: u32) {
        let image_size = pattern.image.size();
        for y in 0..image_size.y() {
            let dest_origin = tex_rect.origin() + Vector2I::new(0, y);
            let dest_start_index = paint_texel_index(dest_origin, texture_length);
            let src_start_index = y as usize * image_size.x() as usize;
            let dest_end_index = dest_start_index + image_size.x() as usize;
            let src_end_index = src_start_index + image_size.x() as usize;
            texels[dest_start_index..dest_end_index].copy_from_slice(
                &pattern.image.pixels()[src_start_index..src_end_index]);
        }
    }
}

impl PaintMetadata {
    // TODO(pcwalton): Apply clamp/repeat to tile rect.
    pub(crate) fn calculate_tex_coords(&self, tile_position: Vector2I) -> Vector2F {
        let tile_size = Vector2I::new(TILE_WIDTH as i32, TILE_HEIGHT as i32);
        let tex_coords = self.tex_transform * tile_position.scale_xy(tile_size).to_f32();
        tex_coords
    }
}

fn paint_texel_index(position: Vector2I, texture_length: u32) -> usize {
    position.y() as usize * texture_length as usize + position.x() as usize
}

fn put_pixel(position: Vector2I, color: ColorU, texels: &mut [ColorU], texture_length: u32) {
    texels[paint_texel_index(position, texture_length)] = color
}

fn rect_to_uv(rect: RectI, texture_length: u32) -> RectF {
    rect.to_f32().scale(1.0 / texture_length as f32)
}

fn rect_to_inset_uv(rect: RectI, texture_length: u32) -> RectF {
    rect_to_uv(rect, texture_length).contract(Vector2F::splat(0.5 / texture_length as f32))
}

// Solid color allocation

struct SolidColorTileBuilder(Option<SolidColorTileBuilderData>);

struct SolidColorTileBuilderData {
    tile_location: TextureLocation,
    next_index: u32,
}

impl SolidColorTileBuilder {
    fn new() -> SolidColorTileBuilder {
        SolidColorTileBuilder(None)
    }

    fn allocate(&mut self, allocator: &mut TextureAllocator) -> TextureLocation {
        if self.0.is_none() {
            // TODO(pcwalton): Handle allocation failure gracefully!
            self.0 = Some(SolidColorTileBuilderData {
                tile_location: allocator.allocate(Vector2I::splat(SOLID_COLOR_TILE_LENGTH as i32))
                                        .expect("Failed to allocate a solid color tile!"),
                next_index: 0,
            });
        }

        let (location, tile_full);
        {
            let mut data = self.0.as_mut().unwrap();
            let subtile_origin = Vector2I::new((data.next_index % SOLID_COLOR_TILE_LENGTH) as i32,
                                               (data.next_index / SOLID_COLOR_TILE_LENGTH) as i32);
            location = TextureLocation {
                rect: RectI::new(data.tile_location.rect.origin() + subtile_origin,
                                 Vector2I::splat(1)),
            };
            data.next_index += 1;
            tile_full = data.next_index == MAX_SOLID_COLORS_PER_TILE;
        }

        if tile_full {
            self.0 = None;
        }

        location
    }
}

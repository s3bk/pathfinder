// pathfinder/renderer/src/gpu_data.rs
//
// Copyright © 2020 The Pathfinder Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Packed data ready to be sent to the GPU.

use crate::options::BoundingQuad;
use pathfinder_color::ColorU;
use pathfinder_content::effects::{BlendMode, Effects};
use pathfinder_content::fill::FillRule;
use pathfinder_content::render_target::RenderTargetId;
use pathfinder_geometry::line_segment::{LineSegmentU4, LineSegmentU8};
use pathfinder_geometry::rect::RectI;
use pathfinder_geometry::vector::Vector2I;
use pathfinder_gpu::TextureSamplingFlags;
use std::fmt::{Debug, Formatter, Result as DebugResult};
use std::time::Duration;

pub enum RenderCommand {
    // Starts rendering a frame.
    Start {
        /// The number of paths that will be rendered.
        path_count: usize,

        /// A bounding quad for the scene.
        bounding_quad: BoundingQuad,

        /// Whether the framebuffer we're rendering to must be readable.
        ///
        /// This is needed if a path that renders directly to the output framebuffer (i.e. not to a
        /// render target) uses one of the more exotic blend modes.
        needs_readable_framebuffer: bool,
    },

    // Allocates texture pages for the frame.
    AllocateTexturePages(Vec<TexturePageDescriptor>),

    // Uploads data to a texture page.
    UploadTexelData { texels: Vec<ColorU>, location: TextureLocation },

    // Associates a render target with a texture page.
    //
    // TODO(pcwalton): Add a rect to this so we can render to subrects of a page.
    DeclareRenderTarget { id: RenderTargetId, location: TextureLocation },

    // Adds fills to the queue.
    AddFills(Vec<FillBatchPrimitive>),

    // Flushes the queue of fills.
    FlushFills,

    // Render fills to a set of mask tiles.
    RenderMaskTiles { tiles: Vec<MaskTile>, fill_rule: FillRule },

    // Pushes a render target onto the stack. Draw commands go to the render target on top of the
    // stack.
    PushRenderTarget(RenderTargetId),

    // Pops a render target from the stack.
    PopRenderTarget,

    // Draws a batch of alpha tiles to the render target on top of the stack.
    DrawAlphaTiles(AlphaTileBatch),

    // Draws a batch of solid tiles to the render target on top of the stack.
    DrawSolidTiles(SolidTileBatch),

    // Presents a rendered frame.
    Finish { build_time: Duration },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TexturePageId(pub u32);

#[derive(Clone, Debug)]
pub struct TexturePageDescriptor {
    pub size: Vector2I,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TextureLocation {
    pub page: TexturePageId,
    pub rect: RectI,
}

#[derive(Clone, Debug)]
pub struct AlphaTileBatch {
    pub tiles: Vec<AlphaTile>,
    pub color_texture_page: TexturePageId,
    pub blend_mode: BlendMode,
    pub sampling_flags: TextureSamplingFlags,
}

#[derive(Clone, Debug)]
pub struct SolidTileBatch {
    pub tiles: Vec<SolidTile>,
    pub color_texture_page: TexturePageId,
    pub sampling_flags: TextureSamplingFlags,
    pub effects: Effects,
}

#[derive(Clone, Copy, Debug)]
pub struct FillObjectPrimitive {
    pub px: LineSegmentU4,
    pub subpx: LineSegmentU8,
    pub tile_x: i16,
    pub tile_y: i16,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct TileObjectPrimitive {
    /// If `u16::MAX`, then this is a solid tile.
    pub alpha_tile_index: u16,
    pub backdrop: i8,
}

// FIXME(pcwalton): Move `subpx` before `px` and remove `repr(packed)`.
#[derive(Clone, Copy, Debug, Default)]
#[repr(packed)]
pub struct FillBatchPrimitive {
    pub px: LineSegmentU4,
    pub subpx: LineSegmentU8,
    pub alpha_tile_index: u16,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct SolidTileVertex {
    pub tile_x: i16,
    pub tile_y: i16,
    pub color_u: f32,
    pub color_v: f32,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct MaskTile {
    pub upper_left: MaskTileVertex,
    pub upper_right: MaskTileVertex,
    pub lower_left: MaskTileVertex,
    pub lower_right: MaskTileVertex,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct AlphaTile {
    pub upper_left: AlphaTileVertex,
    pub upper_right: AlphaTileVertex,
    pub lower_left: AlphaTileVertex,
    pub lower_right: AlphaTileVertex,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct SolidTile {
    pub upper_left: SolidTileVertex,
    pub upper_right: SolidTileVertex,
    pub lower_left: SolidTileVertex,
    pub lower_right: SolidTileVertex,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct MaskTileVertex {
    pub mask_u: u16,
    pub mask_v: u16,
    pub fill_u: u16,
    pub fill_v: u16,
    pub backdrop: i16,
    pub object_index: u16,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct AlphaTileVertex {
    pub tile_x: i16,
    pub tile_y: i16,
    pub mask_u: u16,
    pub mask_v: u16,
    pub color_u: f32,
    pub color_v: f32,
    pub object_index: u16,
    pub opacity: u8,
    pub pad: u8,
}

impl Debug for RenderCommand {
    fn fmt(&self, formatter: &mut Formatter) -> DebugResult {
        match *self {
            RenderCommand::Start { .. } => write!(formatter, "Start"),
            RenderCommand::AllocateTexturePages(ref pages) => {
                write!(formatter, "AllocateTexturePages(x{})", pages.len())
            }
            RenderCommand::UploadTexelData { ref texels, location } => {
                write!(formatter, "UploadTexelData({:?}, {:?})", texels, location)
            }
            RenderCommand::DeclareRenderTarget { id, location } => {
                write!(formatter, "DeclareRenderTarget({:?}, {:?})", id, location)
            }
            RenderCommand::AddFills(ref fills) => write!(formatter, "AddFills(x{})", fills.len()),
            RenderCommand::FlushFills => write!(formatter, "FlushFills"),
            RenderCommand::RenderMaskTiles { ref tiles, fill_rule } => {
                write!(formatter, "RenderMaskTiles(x{}, {:?})", tiles.len(), fill_rule)
            }
            RenderCommand::PushRenderTarget(render_target_id) => {
                write!(formatter, "PushRenderTarget({:?})", render_target_id)
            }
            RenderCommand::PopRenderTarget => write!(formatter, "PopRenderTarget"),
            RenderCommand::DrawAlphaTiles(ref batch) => {
                write!(formatter,
                       "DrawAlphaTiles(x{}, {:?}, {:?}, {:?})",
                       batch.tiles.len(),
                       batch.color_texture_page,
                       batch.blend_mode,
                       batch.sampling_flags)
            }
            RenderCommand::DrawSolidTiles(ref batch) => {
                write!(formatter,
                       "DrawSolidTiles(x{}, {:?}, {:?})",
                       batch.tiles.len(),
                       batch.color_texture_page,
                       batch.sampling_flags)
            }
            RenderCommand::Finish { .. } => write!(formatter, "Finish"),
        }
    }
}

// pathfinder/renderer/src/builder.rs
//
// Copyright © 2019 The Pathfinder Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Packs data onto the GPU.

use crate::concurrent::executor::Executor;
use crate::gpu::renderer::MASK_TILES_ACROSS;
use crate::gpu_data::{AlphaTile, AlphaTileVertex, FillBatchPrimitive, MaskTile, MaskTileVertex};
use crate::gpu_data::{RenderCommand, TileObjectPrimitive};
use crate::options::{PreparedBuildOptions, RenderCommandListener};
use crate::paint::{PaintInfo, PaintMetadata};
use crate::scene::Scene;
use crate::tile_map::DenseTileMap;
use crate::tiles::{self, TILE_HEIGHT, TILE_WIDTH, Tiler, TilingPathInfo};
use crate::z_buffer::ZBuffer;
use pathfinder_content::fill::FillRule;
use pathfinder_geometry::line_segment::{LineSegment2F, LineSegmentU4, LineSegmentU8};
use pathfinder_geometry::vector::{Vector2F, Vector2I};
use pathfinder_geometry::rect::{RectF, RectI};
use pathfinder_geometry::util;
use pathfinder_simd::default::{F32x4, I32x4};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::u16;

#[cfg(target_arch = "wasm32")]
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

pub(crate) struct SceneBuilder<'a, L: RenderCommandListener> {
    scene: &'a Scene,
    built_options: &'a PreparedBuildOptions,

    next_alpha_tile_index: AtomicUsize,
    next_mask_tile_index: AtomicUsize,

    pub(crate) z_buffer: ZBuffer,
    pub(crate) listener: L,
}

#[derive(Debug)]
pub(crate) struct ObjectBuilder {
    pub built_path: BuiltPath,
    pub fills: Vec<FillBatchPrimitive>,
    pub bounds: RectF,
}

#[derive(Debug)]
pub(crate) struct BuiltPath {
    pub mask_tiles: Vec<MaskTile>,
    pub alpha_tiles: Vec<AlphaTile>,
    pub tiles: DenseTileMap<TileObjectPrimitive>,
    pub fill_rule: FillRule,
}

impl<'a, L: RenderCommandListener> SceneBuilder<'a, L> {
    pub(crate) fn new(
        scene: &'a Scene,
        built_options: &'a PreparedBuildOptions,
        listener: L,
    ) -> SceneBuilder<'a, L> {
        let effective_view_box = scene.effective_view_box(built_options);
        SceneBuilder {
            scene,
            built_options,

            next_alpha_tile_index: AtomicUsize::new(0),
            next_mask_tile_index: AtomicUsize::new(0),

            z_buffer: ZBuffer::new(effective_view_box),
            listener,
        }
    }

    pub fn build<E>(&mut self, executor: &E) where E: Executor {
        #[cfg(not(target_arch = "wasm32"))]
        let start_time = Instant::now();

        let bounding_quad = self.built_options.bounding_quad();

        let clip_path_count = self.scene.clip_paths.len();
        let draw_path_count = self.scene.paths.len();
        let total_path_count = clip_path_count + draw_path_count;
        self.listener.send(RenderCommand::Start { bounding_quad, path_count: total_path_count });

        let PaintInfo {
            data: paint_data,
            metadata: paint_metadata,
        } = self.scene.build_paint_info();
        self.listener.send(RenderCommand::AddPaintData(paint_data));

        let effective_view_box = self.scene.effective_view_box(self.built_options);

        let built_clip_paths = executor.build_vector(clip_path_count, |path_index| {
            self.build_clip_path(path_index, effective_view_box, &self.built_options, &self.scene)
        });

        let built_draw_paths = executor.build_vector(draw_path_count, |path_index| {
            self.build_draw_path(path_index,
                                 effective_view_box,
                                 &self.built_options,
                                 &self.scene,
                                 &paint_metadata,
                                 &built_clip_paths)
        });

        self.finish_building(&paint_metadata, built_clip_paths, built_draw_paths);

        #[cfg(not(target_arch = "wasm32"))]
        let build_time = Instant::now() - start_time;

        #[cfg(target_arch = "wasm32")]
        let build_time = Duration::from_millis(0);

        self.listener.send(RenderCommand::Finish { build_time });
    }

    fn build_clip_path(
        &self,
        path_index: usize,
        view_box: RectF,
        built_options: &PreparedBuildOptions,
        scene: &Scene,
    ) -> BuiltPath {
        let path_object = &scene.clip_paths[path_index];
        let outline = scene.apply_render_options(path_object.outline(), built_options);

        let mut tiler = Tiler::new(self,
                                   &outline,
                                   path_object.fill_rule(),
                                   view_box,
                                   path_index as u16,
                                   TilingPathInfo::Clip);

        tiler.generate_tiles();

        self.listener.send(RenderCommand::AddFills(tiler.object_builder.fills));
        tiler.object_builder.built_path
    }

    fn build_draw_path(
        &self,
        path_index: usize,
        view_box: RectF,
        built_options: &PreparedBuildOptions,
        scene: &Scene,
        paint_metadata: &[PaintMetadata],
        built_clip_paths: &[BuiltPath],
    ) -> BuiltPath {
        let path_object = &scene.paths[path_index];
        let outline = scene.apply_render_options(path_object.outline(), built_options);
        let paint_id = path_object.paint();

        let built_clip_path =
            path_object.clip_path().map(|clip_path_id| &built_clip_paths[clip_path_id.0 as usize]);

        let mut tiler = Tiler::new(self,
                                   &outline,
                                   path_object.fill_rule(),
                                   view_box,
                                   path_index as u16,
                                   TilingPathInfo::Draw {
            paint_metadata: &paint_metadata[paint_id.0 as usize],
            built_clip_path,
        });

        tiler.generate_tiles();

        self.listener.send(RenderCommand::AddFills(tiler.object_builder.fills));
        tiler.object_builder.built_path
    }

    fn cull_tiles(&self, built_clip_paths: Vec<BuiltPath>, built_draw_paths: Vec<BuiltPath>)
                  -> CulledTiles {
        let mut culled_tiles = CulledTiles {
            mask_winding_tiles: vec![],
            mask_evenodd_tiles: vec![],
            alpha_tiles: vec![],
        };

        for built_clip_path in built_clip_paths {
            culled_tiles.push_mask_tiles(&built_clip_path);
        }

        for built_draw_path in built_draw_paths {
            culled_tiles.push_mask_tiles(&built_draw_path);

            for alpha_tile in built_draw_path.alpha_tiles {
                let alpha_tile_coords = alpha_tile.upper_left.tile_position();
                if self.z_buffer.test(alpha_tile_coords,
                                      alpha_tile.upper_left.object_index as u32) {
                    culled_tiles.alpha_tiles.push(alpha_tile);
                }
            }
        }

        culled_tiles
    }

    fn pack_tiles(&mut self, paint_metadata: &[PaintMetadata], culled_tiles: CulledTiles) {
        let path_count = self.scene.paths.len() as u32;
        let solid_tiles = self.z_buffer.build_solid_tiles(&self.scene.paths,
                                                          paint_metadata,
                                                          0..path_count);

        if !culled_tiles.mask_winding_tiles.is_empty() {
            self.listener.send(RenderCommand::RenderMaskTiles {
                tiles: culled_tiles.mask_winding_tiles,
                fill_rule: FillRule::Winding,
            });
        }
        if !culled_tiles.mask_evenodd_tiles.is_empty() {
            self.listener.send(RenderCommand::RenderMaskTiles {
                tiles: culled_tiles.mask_evenodd_tiles,
                fill_rule: FillRule::EvenOdd,
            });
        }

        if !solid_tiles.is_empty() {
            self.listener.send(RenderCommand::DrawSolidTiles(solid_tiles));
        }
        if !culled_tiles.alpha_tiles.is_empty() {
            self.listener.send(RenderCommand::DrawAlphaTiles(culled_tiles.alpha_tiles));
        }
    }

    fn finish_building(&mut self,
                       paint_metadata: &[PaintMetadata],
                       built_clip_paths: Vec<BuiltPath>,
                       built_draw_paths: Vec<BuiltPath>) {
        self.listener.send(RenderCommand::FlushFills);
        let culled_tiles = self.cull_tiles(built_clip_paths, built_draw_paths);
        self.pack_tiles(paint_metadata, culled_tiles);
    }

    pub(crate) fn allocate_mask_tile_index(&self) -> u16 {
        // FIXME(pcwalton): Check for overflow!
        self.next_mask_tile_index.fetch_add(1, Ordering::Relaxed) as u16
    }
}

struct CulledTiles {
    mask_winding_tiles: Vec<MaskTile>,
    mask_evenodd_tiles: Vec<MaskTile>,
    alpha_tiles: Vec<AlphaTile>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TileStats {
    pub solid_tile_count: u32,
    pub alpha_tile_count: u32,
}

// Utilities for built objects

impl ObjectBuilder {
    pub(crate) fn new(bounds: RectF, fill_rule: FillRule) -> ObjectBuilder {
        let tile_rect = tiles::round_rect_out_to_tile_bounds(bounds);
        let tiles = DenseTileMap::new(tile_rect);
        ObjectBuilder {
            built_path: BuiltPath { mask_tiles: vec![], alpha_tiles: vec![], tiles, fill_rule },
            bounds,
            fills: vec![],
        }
    }

    #[inline]
    pub(crate) fn tile_rect(&self) -> RectI {
        self.built_path.tiles.rect
    }

    fn add_fill<L: RenderCommandListener>(
        &mut self,
        scene_builder: &SceneBuilder<L>,
        segment: LineSegment2F,
        tile_coords: Vector2I,
    ) {
        debug!("add_fill({:?} ({:?}))", segment, tile_coords);

        // Ensure this fill is in bounds. If not, cull it.
        if self.tile_coords_to_local_index(tile_coords).is_none() {
            return;
        };

        debug_assert_eq!(TILE_WIDTH, TILE_HEIGHT);

        // Compute the upper left corner of the tile.
        let tile_size = F32x4::splat(TILE_WIDTH as f32);
        let tile_upper_left = tile_coords.to_f32().0.to_f32x4().xyxy() * tile_size;

        // Convert to 4.8 fixed point.
        let segment = (segment.0 - tile_upper_left) * F32x4::splat(256.0);
        let (min, max) = (F32x4::default(), F32x4::splat((TILE_WIDTH * 256 - 1) as f32));
        let segment = segment.clamp(min, max).to_i32x4();
        let (from_x, from_y, to_x, to_y) = (segment[0], segment[1], segment[2], segment[3]);

        // Cull degenerate fills.
        if from_x == to_x {
            debug!("... culling!");
            return;
        }

        // Allocate global tile if necessary.
        let alpha_tile_index = self.get_or_allocate_alpha_tile_index(scene_builder, tile_coords);

        // Pack whole pixels.
        let px = (segment & I32x4::splat(0xf00)).to_u32x4();
        let px = (px >> 8).to_i32x4() | (px >> 4).to_i32x4().yxwz();

        // Pack instance data.
        debug!("... OK, pushing");
        self.fills.push(FillBatchPrimitive {
            px: LineSegmentU4 { from: px[0] as u8, to: px[2] as u8 },
            subpx: LineSegmentU8 {
                from_x: from_x as u8,
                from_y: from_y as u8,
                to_x:   to_x   as u8,
                to_y:   to_y   as u8,
            },
            alpha_tile_index,
        });
    }

    fn get_or_allocate_alpha_tile_index<L: RenderCommandListener>(
        &mut self,
        scene_builder: &SceneBuilder<L>,
        tile_coords: Vector2I,
    ) -> u16 {
        let local_tile_index = self.built_path.tiles.coords_to_index_unchecked(tile_coords);
        let alpha_tile_index = self.built_path.tiles.data[local_tile_index].alpha_tile_index;
        if alpha_tile_index != !0 {
            return alpha_tile_index;
        }

        // FIXME(pcwalton): Check for overflow!
        let alpha_tile_index = scene_builder
            .next_alpha_tile_index
            .fetch_add(1, Ordering::Relaxed) as u16;
        self.built_path.tiles.data[local_tile_index].alpha_tile_index = alpha_tile_index;
        alpha_tile_index
    }

    pub(crate) fn add_active_fill<L: RenderCommandListener>(
        &mut self,
        scene_builder: &SceneBuilder<L>,
        left: f32,
        right: f32,
        mut winding: i32,
        tile_coords: Vector2I,
    ) {
        let tile_origin_y = (tile_coords.y() * TILE_HEIGHT as i32) as f32;
        let left = Vector2F::new(left, tile_origin_y);
        let right = Vector2F::new(right, tile_origin_y);

        let segment = if winding < 0 {
            LineSegment2F::new(left, right)
        } else {
            LineSegment2F::new(right, left)
        };

        debug!(
            "... emitting active fill {} -> {} winding {} @ tile {:?}",
            left.x(),
            right.x(),
            winding,
            tile_coords
        );

        while winding != 0 {
            self.add_fill(scene_builder, segment, tile_coords);
            if winding < 0 {
                winding += 1
            } else {
                winding -= 1
            }
        }
    }

    pub(crate) fn generate_fill_primitives_for_line<L: RenderCommandListener>(
        &mut self,
        scene_builder: &SceneBuilder<L>,
        mut segment: LineSegment2F,
        tile_y: i32,
    ) {
        debug!(
            "... generate_fill_primitives_for_line(): segment={:?} tile_y={} ({}-{})",
            segment,
            tile_y,
            tile_y as f32 * TILE_HEIGHT as f32,
            (tile_y + 1) as f32 * TILE_HEIGHT as f32
        );

        let winding = segment.from_x() > segment.to_x();
        let (segment_left, segment_right) = if !winding {
            (segment.from_x(), segment.to_x())
        } else {
            (segment.to_x(), segment.from_x())
        };

        // FIXME(pcwalton): Optimize this.
        let segment_tile_left = f32::floor(segment_left) as i32 / TILE_WIDTH as i32;
        let segment_tile_right =
            util::alignup_i32(f32::ceil(segment_right) as i32, TILE_WIDTH as i32);
        debug!(
            "segment_tile_left={} segment_tile_right={} tile_rect={:?}",
            segment_tile_left,
            segment_tile_right,
            self.tile_rect()
        );

        for subsegment_tile_x in segment_tile_left..segment_tile_right {
            let (mut fill_from, mut fill_to) = (segment.from(), segment.to());
            let subsegment_tile_right =
                ((i32::from(subsegment_tile_x) + 1) * TILE_HEIGHT as i32) as f32;
            if subsegment_tile_right < segment_right {
                let x = subsegment_tile_right;
                let point = Vector2F::new(x, segment.solve_y_for_x(x));
                if !winding {
                    fill_to = point;
                    segment = LineSegment2F::new(point, segment.to());
                } else {
                    fill_from = point;
                    segment = LineSegment2F::new(segment.from(), point);
                }
            }

            let fill_segment = LineSegment2F::new(fill_from, fill_to);
            let fill_tile_coords = Vector2I::new(subsegment_tile_x, tile_y);
            self.add_fill(scene_builder, fill_segment, fill_tile_coords);
        }
    }

    #[inline]
    pub(crate) fn tile_coords_to_local_index(&self, coords: Vector2I) -> Option<u32> {
        self.built_path.tiles.coords_to_index(coords).map(|index| index as u32)
    }

    #[inline]
    pub(crate) fn local_tile_index_to_coords(&self, tile_index: u32) -> Vector2I {
        self.built_path.tiles.index_to_coords(tile_index as usize)
    }

    pub(crate) fn push_mask_tile(mask_tiles: &mut Vec<MaskTile>,
                                 fill_tile: &TileObjectPrimitive,
                                 mask_tile_index: u16,
                                 object_index: u16) {
        mask_tiles.push(MaskTile {
            upper_left: MaskTileVertex::new(mask_tile_index,
                                            fill_tile.alpha_tile_index as u16,
                                            Vector2I::default(),
                                            object_index,
                                            fill_tile.backdrop as i16),
            upper_right: MaskTileVertex::new(mask_tile_index,
                                             fill_tile.alpha_tile_index as u16,
                                             Vector2I::new(1, 0),
                                             object_index,
                                             fill_tile.backdrop as i16),
            lower_left: MaskTileVertex::new(mask_tile_index,
                                            fill_tile.alpha_tile_index as u16,
                                            Vector2I::new(0, 1),
                                            object_index,
                                            fill_tile.backdrop as i16),
            lower_right: MaskTileVertex::new(mask_tile_index,
                                             fill_tile.alpha_tile_index as u16,
                                             Vector2I::splat(1),
                                             object_index,
                                             fill_tile.backdrop as i16),
        });
    }

    pub(crate) fn push_alpha_tile(alpha_tiles: &mut Vec<AlphaTile>,
                                  mask_tile_index: u16,
                                  tile_coords: Vector2I,
                                  object_index: u16,
                                  paint_metadata: &PaintMetadata) {
        alpha_tiles.push(AlphaTile {
            upper_left: AlphaTileVertex::new(tile_coords,
                                             mask_tile_index,
                                             Vector2I::default(),
                                             object_index,
                                             paint_metadata),
            upper_right: AlphaTileVertex::new(tile_coords,
                                              mask_tile_index,
                                              Vector2I::new(1, 0),
                                              object_index,
                                              paint_metadata),
            lower_left: AlphaTileVertex::new(tile_coords,
                                             mask_tile_index,
                                             Vector2I::new(0, 1),
                                             object_index,
                                             paint_metadata),
            lower_right: AlphaTileVertex::new(tile_coords,
                                              mask_tile_index,
                                              Vector2I::splat(1),
                                              object_index,
                                              paint_metadata),
        });
    }
}

impl MaskTileVertex {
    #[inline]
    fn new(mask_index: u16,
           fill_index: u16,
           tile_offset: Vector2I,
           object_index: u16,
           backdrop: i16)
           -> MaskTileVertex {
        let mask_uv = calculate_mask_uv(mask_index, tile_offset);
        let fill_uv = calculate_mask_uv(fill_index, tile_offset);
        MaskTileVertex {
            mask_u: mask_uv.x() as u16,
            mask_v: mask_uv.y() as u16,
            fill_u: fill_uv.x() as u16,
            fill_v: fill_uv.y() as u16,
            backdrop,
            object_index,
        }
    }
}

impl AlphaTileVertex {
    #[inline]
    fn new(tile_origin: Vector2I,
           tile_index: u16,
           tile_offset: Vector2I,
           object_index: u16,
           paint_metadata: &PaintMetadata)
           -> AlphaTileVertex {
        let tile_position = tile_origin + tile_offset;
        let color_uv = paint_metadata.calculate_tex_coords(tile_position).scale(65535.0).to_i32();
        let mask_uv = calculate_mask_uv(tile_index, tile_offset);

        AlphaTileVertex {
            tile_x: tile_position.x() as i16,
            tile_y: tile_position.y() as i16,
            color_u: color_uv.x() as u16,
            color_v: color_uv.y() as u16,
            mask_u: mask_uv.x() as u16,
            mask_v: mask_uv.y() as u16,
            object_index,
            pad: 0,
        }
    }

    #[inline]
    pub fn tile_position(&self) -> Vector2I {
        Vector2I::new(self.tile_x as i32, self.tile_y as i32)
    }
}

fn calculate_mask_uv(tile_index: u16, tile_offset: Vector2I) -> Vector2I {
    let mask_u = tile_index as i32 % MASK_TILES_ACROSS as i32;
    let mask_v = tile_index as i32 / MASK_TILES_ACROSS as i32;
    let mask_scale = 65535.0 / MASK_TILES_ACROSS as f32;
    let mask_uv = Vector2I::new(mask_u, mask_v) + tile_offset;
    mask_uv.to_f32().scale(mask_scale).to_i32()
}

impl CulledTiles {
    fn push_mask_tiles(&mut self, built_path: &BuiltPath) {
        match built_path.fill_rule {
            FillRule::Winding => self.mask_winding_tiles.extend_from_slice(&built_path.mask_tiles),
            FillRule::EvenOdd => self.mask_evenodd_tiles.extend_from_slice(&built_path.mask_tiles),
        }
    }
}

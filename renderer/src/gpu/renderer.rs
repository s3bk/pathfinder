// pathfinder/renderer/src/gpu/renderer.rs
//
// Copyright © 2020 The Pathfinder Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::gpu::debug::DebugUIPresenter;
use crate::gpu::mem::{ClipVertexStorage, DiceMetadataStorage, FillVertexStorage, FirstTile};
use crate::gpu::mem::{StorageAllocators, StorageID, TextureCache, TexturePage, TileVertexStorage};
use crate::gpu::options::{DestFramebuffer, RendererLevel, RendererOptions};
use crate::gpu::perf::{PendingTimer, RenderStats, RenderTime, TimerFuture, TimerQueryCache};
use crate::gpu::shaders::{BlitBufferVertexArray, BlitProgram, BlitVertexArray, ClearProgram};
use crate::gpu::shaders::{ClearVertexArray, ClipTileCombineProgram, ClipTileCopyProgram};
use crate::gpu::shaders::{CopyTileProgram, D3D11Programs, FillProgram, MAX_FILLS_PER_BATCH};
use crate::gpu::shaders::{PROPAGATE_WORKGROUP_SIZE, ReprojectionProgram, ReprojectionVertexArray};
use crate::gpu::shaders::{SORT_WORKGROUP_SIZE, StencilProgram, StencilVertexArray};
use crate::gpu::shaders::{TileProgram, TileProgramCommon};
use crate::gpu_data::{BackdropInfo, Clip, DiceMetadata, Fill, PathSource, PrepareTilesBatch};
use crate::gpu_data::{PrepareTilesModalInfo, PropagateMetadata, RenderCommand, SegmentIndices};
use crate::gpu_data::{Segments, TextureLocation, TextureMetadataEntry, TexturePageDescriptor};
use crate::gpu_data::{TexturePageId, TileBatchTexture, TileObjectPrimitive, TilePathInfo};
use crate::options::BoundingQuad;
use crate::paint::PaintCompositeOp;
use crate::tile_map::DenseTileMap;
use crate::tiles::{TILE_HEIGHT, TILE_WIDTH};
use byte_slice_cast::{AsByteSlice, AsSliceOf};
use half::f16;
use pathfinder_color::{self as color, ColorF, ColorU};
use pathfinder_content::effects::{BlendMode, BlurDirection, DefringingKernel};
use pathfinder_content::effects::{Filter, PatternFilter};
use pathfinder_content::render_target::RenderTargetId;
use pathfinder_geometry::line_segment::LineSegment2F;
use pathfinder_geometry::rect::{RectF, RectI};
use pathfinder_geometry::transform2d::Transform2F;
use pathfinder_geometry::transform3d::Transform4F;
use pathfinder_geometry::util;
use pathfinder_geometry::vector::{Vector2F, Vector2I, Vector4F, vec2f, vec2i};
use pathfinder_gpu::{BlendFactor, BlendState, BufferData, BufferTarget, BufferUploadMode};
use pathfinder_gpu::{ClearOps, ComputeDimensions, ComputeState, DepthFunc, DepthState, Device};
use pathfinder_gpu::{ImageAccess, Primitive, RenderOptions, RenderState, RenderTarget};
use pathfinder_gpu::{StencilFunc, StencilState, TextureBinding, TextureDataRef, TextureFormat};
use pathfinder_gpu::{UniformBinding, UniformData};
use pathfinder_resources::ResourceLoader;
use pathfinder_simd::default::{F32x2, F32x4, I32x2};
use std::collections::VecDeque;
use std::f32;
use std::mem;
use std::time::Duration;
use std::u32;
use vec_map::VecMap;

static QUAD_VERTEX_POSITIONS: [u16; 8] = [0, 0, 1, 0, 1, 1, 0, 1];
static QUAD_VERTEX_INDICES: [u32; 6] = [0, 1, 3, 1, 2, 3];

pub(crate) const MASK_TILES_ACROSS: u32 = 256;
pub(crate) const MASK_TILES_DOWN: u32 = 256;

// 1.0 / sqrt(2*pi)
const SQRT_2_PI_INV: f32 = 0.3989422804014327;

const TEXTURE_METADATA_ENTRIES_PER_ROW: i32 = 128;
const TEXTURE_METADATA_TEXTURE_WIDTH:   i32 = TEXTURE_METADATA_ENTRIES_PER_ROW * 4;
const TEXTURE_METADATA_TEXTURE_HEIGHT:  i32 = 65536 / TEXTURE_METADATA_ENTRIES_PER_ROW;

// FIXME(pcwalton): Shrink this again!
const MASK_FRAMEBUFFER_WIDTH:  i32 = TILE_WIDTH as i32      * MASK_TILES_ACROSS as i32;
const MASK_FRAMEBUFFER_HEIGHT: i32 = TILE_HEIGHT as i32 / 4 * MASK_TILES_DOWN as i32;

const COMBINER_CTRL_COLOR_COMBINE_SRC_IN: i32 =     0x1;
const COMBINER_CTRL_COLOR_COMBINE_DEST_IN: i32 =    0x2;

const COMBINER_CTRL_FILTER_RADIAL_GRADIENT: i32 =   0x1;
const COMBINER_CTRL_FILTER_TEXT: i32 =              0x2;
const COMBINER_CTRL_FILTER_BLUR: i32 =              0x3;

const COMBINER_CTRL_COMPOSITE_NORMAL: i32 =         0x0;
const COMBINER_CTRL_COMPOSITE_MULTIPLY: i32 =       0x1;
const COMBINER_CTRL_COMPOSITE_SCREEN: i32 =         0x2;
const COMBINER_CTRL_COMPOSITE_OVERLAY: i32 =        0x3;
const COMBINER_CTRL_COMPOSITE_DARKEN: i32 =         0x4;
const COMBINER_CTRL_COMPOSITE_LIGHTEN: i32 =        0x5;
const COMBINER_CTRL_COMPOSITE_COLOR_DODGE: i32 =    0x6;
const COMBINER_CTRL_COMPOSITE_COLOR_BURN: i32 =     0x7;
const COMBINER_CTRL_COMPOSITE_HARD_LIGHT: i32 =     0x8;
const COMBINER_CTRL_COMPOSITE_SOFT_LIGHT: i32 =     0x9;
const COMBINER_CTRL_COMPOSITE_DIFFERENCE: i32 =     0xa;
const COMBINER_CTRL_COMPOSITE_EXCLUSION: i32 =      0xb;
const COMBINER_CTRL_COMPOSITE_HUE: i32 =            0xc;
const COMBINER_CTRL_COMPOSITE_SATURATION: i32 =     0xd;
const COMBINER_CTRL_COMPOSITE_COLOR: i32 =          0xe;
const COMBINER_CTRL_COMPOSITE_LUMINOSITY: i32 =     0xf;

const COMBINER_CTRL_COLOR_FILTER_SHIFT: i32 =       4;
const COMBINER_CTRL_COLOR_COMBINE_SHIFT: i32 =      6;
const COMBINER_CTRL_COMPOSITE_SHIFT: i32 =          8;

const FILL_INDIRECT_DRAW_PARAMS_INSTANCE_COUNT_INDEX:   usize = 1;
const FILL_INDIRECT_DRAW_PARAMS_ALPHA_TILE_COUNT_INDEX: usize = 4;

const BIN_INDIRECT_DRAW_PARAMS_MICROLINE_COUNT_INDEX:   usize = 3;

const INITIAL_ALLOCATED_MICROLINE_COUNT: u32 = 1024 * 16;
const INITIAL_ALLOCATED_FILL_COUNT: u32 = 1024 * 16;

const LOAD_ACTION_CLEAR: i32 = 0;
const LOAD_ACTION_LOAD:  i32 = 1;

pub struct Renderer<D> where D: Device {
    // Device
    pub device: D,

    // Core data
    dest_framebuffer: DestFramebuffer<D>,
    options: RendererOptions,
    blit_program: BlitProgram<D>,
    clear_program: ClearProgram<D>,
    fill_program: FillProgram<D>,
    tile_program: TileProgram<D>,
    tile_copy_program: CopyTileProgram<D>,
    tile_clip_combine_program: ClipTileCombineProgram<D>,
    tile_clip_copy_program: ClipTileCopyProgram<D>,
    d3d11_programs: Option<D3D11Programs<D>>,
    stencil_program: StencilProgram<D>,
    reprojection_program: ReprojectionProgram<D>,
    quad_vertex_positions_buffer: D::Buffer,
    quad_vertex_indices_buffer: D::Buffer,
    texture_pages: Vec<Option<TexturePage<D>>>,
    render_targets: Vec<RenderTargetInfo>,
    render_target_stack: Vec<RenderTargetId>,
    area_lut_texture: D::Texture,
    gamma_lut_texture: D::Texture,

    // Scene
    front_scene_buffers: Option<SceneBuffers<D>>,
    back_scene_buffers: Option<SceneBuffers<D>>,
    allocated_microline_count: u32,
    allocated_fill_count: u32,

    // Frames
    front_frame: Frame<D>,
    back_frame: Frame<D>,
    front_frame_fence: Option<D::Fence>,

    // Rendering state
    texture_cache: TextureCache<D>,

    // Debug
    pub stats: RenderStats,
    current_cpu_build_time: Option<Duration>,
    current_timer: Option<PendingTimer<D>>,
    pending_timers: VecDeque<PendingTimer<D>>,
    timer_query_cache: TimerQueryCache<D>,
    pub debug_ui_presenter: DebugUIPresenter<D>,

    // Extra info
    flags: RendererFlags,
}

struct Frame<D> where D: Device {
    framebuffer_flags: FramebufferFlags,
    blit_vertex_array: BlitVertexArray<D>,
    blit_buffer_vertex_array: Option<BlitBufferVertexArray<D>>,
    clear_vertex_array: ClearVertexArray<D>,
    storage_allocators: StorageAllocators<D>,
    // Maps tile batch IDs to tile vertex storage IDs.
    tile_batch_info: VecMap<TileBatchInfo>,
    quads_vertex_indices_buffer: D::Buffer,
    quads_vertex_indices_length: usize,
    buffered_fills: Vec<Fill>,
    pending_fills: Vec<Fill>,
    max_alpha_tile_index: u32,
    allocated_alpha_tile_page_count: u32,
    mask_framebuffer: Option<D::Framebuffer>,
    // Temporary place that we copy tiles to in order to perform clips, allocated lazily.
    //
    // TODO(pcwalton): This should be sparse, not dense.
    mask_temp_framebuffer: Option<D::Framebuffer>,
    stencil_vertex_array: StencilVertexArray<D>,
    reprojection_vertex_array: ReprojectionVertexArray<D>,
    dest_blend_framebuffer: D::Framebuffer,
    intermediate_dest_framebuffer: D::Framebuffer,
    texture_metadata_texture: D::Texture,
}

impl<D> Renderer<D> where D: Device {
    pub fn new(device: D,
               resources: &dyn ResourceLoader,
               dest_framebuffer: DestFramebuffer<D>,
               options: RendererOptions)
               -> Renderer<D> {
        let blit_program = BlitProgram::new(&device, resources);
        let clear_program = ClearProgram::new(&device, resources);
        let fill_program = FillProgram::new(&device, resources, options.level);
        let tile_program = TileProgram::new(&device, resources, options.level);
        let tile_copy_program = CopyTileProgram::new(&device, resources);
        let tile_clip_combine_program = ClipTileCombineProgram::new(&device, resources);
        let tile_clip_copy_program = ClipTileCopyProgram::new(&device, resources);
        let stencil_program = StencilProgram::new(&device, resources);
        let reprojection_program = ReprojectionProgram::new(&device, resources);

        let d3d11_programs = match options.level {
            RendererLevel::D3D11 => Some(D3D11Programs::new(&device, resources)),
            RendererLevel::D3D9 => None,
        };

        let area_lut_texture =
            device.create_texture_from_png(resources, "area-lut", TextureFormat::RGBA8);
        let gamma_lut_texture =
            device.create_texture_from_png(resources, "gamma-lut", TextureFormat::R8);

        let quad_vertex_positions_buffer = device.create_buffer(BufferUploadMode::Static);
        device.allocate_buffer(&quad_vertex_positions_buffer,
                               BufferData::Memory(&QUAD_VERTEX_POSITIONS),
                               BufferTarget::Vertex);
        let quad_vertex_indices_buffer = device.create_buffer(BufferUploadMode::Static);
        device.allocate_buffer(&quad_vertex_indices_buffer,
                               BufferData::Memory(&QUAD_VERTEX_INDICES),
                               BufferTarget::Index);

        let window_size = dest_framebuffer.window_size(&device);

        let timer_query_cache = TimerQueryCache::new();
        let debug_ui_presenter = DebugUIPresenter::new(&device,
                                                       resources,
                                                       window_size,
                                                       options.level);

        let front_frame = Frame::new(&device,
                                     &blit_program,
                                     &d3d11_programs,
                                     &clear_program,
                                     &reprojection_program,
                                     &stencil_program,
                                     &quad_vertex_positions_buffer,
                                     &quad_vertex_indices_buffer,
                                     window_size);
        let back_frame = Frame::new(&device,
                                    &blit_program,
                                    &d3d11_programs,
                                    &clear_program,
                                    &reprojection_program,
                                    &stencil_program,
                                    &quad_vertex_positions_buffer,
                                    &quad_vertex_indices_buffer,
                                    window_size);

        Renderer {
            device,

            dest_framebuffer,
            options,
            blit_program,
            clear_program,
            fill_program,
            tile_program,
            tile_copy_program,
            tile_clip_combine_program,
            tile_clip_copy_program,
            d3d11_programs,
            quad_vertex_positions_buffer,
            quad_vertex_indices_buffer,
            texture_pages: vec![],
            render_targets: vec![],
            render_target_stack: vec![],

            front_scene_buffers: None,
            back_scene_buffers: None,
            allocated_fill_count: INITIAL_ALLOCATED_FILL_COUNT,
            allocated_microline_count: INITIAL_ALLOCATED_MICROLINE_COUNT,

            front_frame,
            back_frame,
            front_frame_fence: None,

            area_lut_texture,
            gamma_lut_texture,

            stencil_program,
            reprojection_program,

            stats: RenderStats::default(),
            current_cpu_build_time: None,
            current_timer: None,
            pending_timers: VecDeque::new(),
            timer_query_cache,
            debug_ui_presenter,

            texture_cache: TextureCache::new(),

            flags: RendererFlags::empty(),
        }
    }

    pub fn begin_scene(&mut self) {
        self.back_frame.framebuffer_flags = FramebufferFlags::empty();

        self.device.begin_commands();
        self.current_timer = Some(PendingTimer::new());
        self.stats = RenderStats::default();

        self.back_frame.max_alpha_tile_index = 0;
    }

    pub fn render_command(&mut self, command: &RenderCommand) {
        debug!("render command: {:?}", command);
        match *command {
            RenderCommand::Start { bounding_quad, path_count, needs_readable_framebuffer } => {
                self.start_rendering(bounding_quad, path_count, needs_readable_framebuffer);
            }
            RenderCommand::AllocateTexturePage { page_id, ref descriptor } => {
                self.allocate_texture_page(page_id, descriptor)
            }
            RenderCommand::UploadTexelData { ref texels, location } => {
                self.upload_texel_data(texels, location)
            }
            RenderCommand::DeclareRenderTarget { id, location } => {
                self.declare_render_target(id, location)
            }
            RenderCommand::UploadTextureMetadata(ref metadata) => {
                self.upload_texture_metadata(metadata)
            }
            RenderCommand::AddFills(ref fills) => self.add_fills(fills),
            RenderCommand::FlushFills => {
                self.draw_buffered_fills();
            }
            RenderCommand::UploadScene {
                ref draw_segments,
                ref clip_segments,
            } => self.upload_scene(draw_segments, clip_segments),
            RenderCommand::BeginTileDrawing => {}
            RenderCommand::PushRenderTarget(render_target_id) => {
                self.push_render_target(render_target_id)
            }
            RenderCommand::PopRenderTarget => self.pop_render_target(),
            RenderCommand::PrepareTiles(ref batch) => self.prepare_tiles(batch),
            RenderCommand::DrawTiles(ref batch) => {
                let batch_info = self.back_frame.tile_batch_info[batch.tile_batch_id.0 as usize];
                self.draw_tiles(batch_info.tile_count,
                                batch_info.tile_vertex_storage_id,
                                batch.color_texture,
                                batch.blend_mode,
                                batch.filter,
                                batch_info.z_buffer_storage_id,
                                batch_info.d3d11_info)
            }
            RenderCommand::Finish { cpu_build_time } => {
                self.stats.cpu_build_time = cpu_build_time;
            }
        }
    }

    pub fn end_scene(&mut self) {
        self.clear_dest_framebuffer_if_necessary();
        self.blit_intermediate_dest_framebuffer_if_necessary();

        let old_front_frame_fence = self.front_frame_fence.take();
        self.front_frame_fence = Some(self.device.add_fence());
        self.device.end_commands();

        self.stats.gpu_bytes_allocated += self.front_frame.gpu_bytes_allocated();
        self.stats.gpu_bytes_allocated += self.back_frame.gpu_bytes_allocated();

        self.back_frame.storage_allocators.end_frame();
        self.back_frame.tile_batch_info.clear();

        if let Some(timer) = self.current_timer.take() {
            self.pending_timers.push_back(timer);
        }
        self.current_cpu_build_time = None;

        if let Some(old_front_frame_fence) = old_front_frame_fence {
            self.device.wait_for_fence(&old_front_frame_fence);
        }

        mem::swap(&mut self.front_frame, &mut self.back_frame);
    }

    fn start_rendering(&mut self,
                       bounding_quad: BoundingQuad,
                       path_count: usize,
                       needs_readable_framebuffer: bool) {
        match (&self.dest_framebuffer, &self.tile_program) {
            (&DestFramebuffer::Other(_), _) => {
                self.flags.remove(RendererFlags::INTERMEDIATE_DEST_FRAMEBUFFER_NEEDED);
            }
            (&DestFramebuffer::Default { .. }, &TileProgram::Compute(_)) => {
                self.flags.insert(RendererFlags::INTERMEDIATE_DEST_FRAMEBUFFER_NEEDED);
            }
            _ => {
                self.flags.set(RendererFlags::INTERMEDIATE_DEST_FRAMEBUFFER_NEEDED,
                               needs_readable_framebuffer);
            }
        }

        if self.flags.contains(RendererFlags::USE_DEPTH) {
            self.draw_stencil(&bounding_quad);
        }
        self.stats.path_count = path_count;

        self.render_targets.clear();
    }

    pub fn draw_debug_ui(&self) {
        self.debug_ui_presenter.draw(&self.device);
    }

    pub fn shift_rendering_time(&mut self) -> Option<RenderTime> {
        if let Some(mut pending_timer) = self.pending_timers.pop_front() {
            for old_query in pending_timer.poll(&self.device) {
                self.timer_query_cache.free(old_query);
            }
            if let Some(render_time) = pending_timer.total_time() {
                return Some(render_time);
            }
            self.pending_timers.push_front(pending_timer);
        }
        None
    }

    #[inline]
    pub fn dest_framebuffer(&self) -> &DestFramebuffer<D> {
        &self.dest_framebuffer
    }

    #[inline]
    pub fn replace_dest_framebuffer(
        &mut self,
        new_dest_framebuffer: DestFramebuffer<D>,
    ) -> DestFramebuffer<D> {
        mem::replace(&mut self.dest_framebuffer, new_dest_framebuffer)
    }

    #[inline]
    pub fn level(&self) -> RendererLevel {
        self.options.level
    }

    #[inline]
    pub fn set_options(&mut self, new_options: RendererOptions) {
        self.options = new_options
    }

    #[inline]
    pub fn set_main_framebuffer_size(&mut self, new_framebuffer_size: Vector2I) {
        self.debug_ui_presenter.ui_presenter.set_framebuffer_size(new_framebuffer_size);
    }

    #[inline]
    pub fn disable_depth(&mut self) {
        self.flags.remove(RendererFlags::USE_DEPTH);
    }

    #[inline]
    pub fn enable_depth(&mut self) {
        self.flags.insert(RendererFlags::USE_DEPTH);
    }

    #[inline]
    pub fn quad_vertex_positions_buffer(&self) -> &D::Buffer {
        &self.quad_vertex_positions_buffer
    }

    #[inline]
    pub fn quad_vertex_indices_buffer(&self) -> &D::Buffer {
        &self.quad_vertex_indices_buffer
    }

    fn reallocate_alpha_tile_pages_if_necessary(&mut self, copy_existing: bool) {
        let alpha_tile_pages_needed =
            ((self.back_frame.max_alpha_tile_index + 0xffff) >> 16) as u32;
        if alpha_tile_pages_needed <= self.back_frame.allocated_alpha_tile_page_count {
            return;
        }

        let new_size = vec2i(MASK_FRAMEBUFFER_WIDTH,
                             MASK_FRAMEBUFFER_HEIGHT * alpha_tile_pages_needed as i32);
        let mask_texture = self.device.create_texture(TextureFormat::RGBA16F, new_size);
        let old_mask_framebuffer =
            mem::replace(&mut self.back_frame.mask_framebuffer,
                         Some(self.device.create_framebuffer(mask_texture)));
        self.back_frame.allocated_alpha_tile_page_count = alpha_tile_pages_needed;

        // Copy over existing content if needed.
        let old_mask_framebuffer = match old_mask_framebuffer {
            Some(old_mask_framebuffer) if copy_existing => old_mask_framebuffer,
            Some(_) | None => return,
        };
        let old_mask_texture = self.device.framebuffer_texture(&old_mask_framebuffer);
        let old_size = self.device.texture_size(old_mask_texture);

        let timer_query = self.timer_query_cache.alloc(&self.device);
        self.device.begin_timer_query(&timer_query);

        self.device.draw_elements(6, &RenderState {
            target: &RenderTarget::Framebuffer(self.back_frame.mask_framebuffer.as_ref().unwrap()),
            program: &self.blit_program.program,
            vertex_array: &self.back_frame.blit_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            textures: &[(&self.blit_program.src_texture, old_mask_texture)],
            images: &[],
            storage_buffers: &[],
            uniforms: &[
                (&self.blit_program.framebuffer_size_uniform,
                 UniformData::Vec2(new_size.to_f32().0)),
                (&self.blit_program.dest_rect_uniform,
                 UniformData::Vec4(RectF::new(Vector2F::zero(), old_size.to_f32()).0)),
            ],
            viewport: RectI::new(Vector2I::default(), new_size),
            options: RenderOptions {
                clear_ops: ClearOps {
                    color: Some(ColorF::new(0.0, 0.0, 0.0, 1.0)),
                    ..ClearOps::default()
                },
                ..RenderOptions::default()
            },
        });

        self.device.end_timer_query(&timer_query);
        self.current_timer.as_mut().unwrap().other_times.push(TimerFuture::new(timer_query));
        self.stats.drawcall_count += 1;
    }

    fn allocate_texture_page(&mut self,
                             page_id: TexturePageId,
                             descriptor: &TexturePageDescriptor) {
        // Fill in IDs up to the requested page ID.
        let page_index = page_id.0 as usize;
        while self.texture_pages.len() < page_index + 1 {
            self.texture_pages.push(None);
        }

        // Clear out any existing texture.
        if let Some(old_texture_page) = self.texture_pages[page_index].take() {
            let old_texture = self.device.destroy_framebuffer(old_texture_page.framebuffer);
            self.texture_cache.release_texture(old_texture);
        }

        // Allocate texture.
        let texture_size = descriptor.size;
        let texture = self.texture_cache.create_texture(&mut self.device,
                                                        TextureFormat::RGBA8,
                                                        texture_size);
        let framebuffer = self.device.create_framebuffer(texture);
        self.texture_pages[page_index] = Some(TexturePage {
            framebuffer,
            must_preserve_contents: false,
        });
    }

    fn upload_texel_data(&mut self, texels: &[ColorU], location: TextureLocation) {
        let texture_page = self.texture_pages[location.page.0 as usize]
                               .as_mut()
                               .expect("Texture page not allocated yet!");
        let texture = self.device.framebuffer_texture(&texture_page.framebuffer);
        let texels = color::color_slice_to_u8_slice(texels);
        self.device.upload_to_texture(texture, location.rect, TextureDataRef::U8(texels));
        texture_page.must_preserve_contents = true;
    }

    fn declare_render_target(&mut self,
                             render_target_id: RenderTargetId,
                             location: TextureLocation) {
        while self.render_targets.len() < render_target_id.render_target as usize + 1 {
            self.render_targets.push(RenderTargetInfo {
                location: TextureLocation { page: TexturePageId(!0), rect: RectI::default() },
            });
        }
        let mut render_target = &mut self.render_targets[render_target_id.render_target as usize];
        debug_assert_eq!(render_target.location.page, TexturePageId(!0));
        render_target.location = location;
    }

    fn upload_texture_metadata(&mut self, metadata: &[TextureMetadataEntry]) {
        let padded_texel_size =
            (util::alignup_i32(metadata.len() as i32, TEXTURE_METADATA_ENTRIES_PER_ROW) *
             TEXTURE_METADATA_TEXTURE_WIDTH * 4) as usize;
        let mut texels = Vec::with_capacity(padded_texel_size);
        for entry in metadata {
            let base_color = entry.base_color.to_f32();
            texels.extend_from_slice(&[
                f16::from_f32(entry.color_0_transform.m11()),
                f16::from_f32(entry.color_0_transform.m21()),
                f16::from_f32(entry.color_0_transform.m12()),
                f16::from_f32(entry.color_0_transform.m22()),
                f16::from_f32(entry.color_0_transform.m13()),
                f16::from_f32(entry.color_0_transform.m23()),
                f16::default(),
                f16::default(),
                f16::from_f32(base_color.r()),
                f16::from_f32(base_color.g()),
                f16::from_f32(base_color.b()),
                f16::from_f32(base_color.a()),
                f16::default(),
                f16::default(),
                f16::default(),
                f16::default(),
            ]);
        }
        while texels.len() < padded_texel_size {
            texels.push(f16::default())
        }

        let texture = &mut self.back_frame.texture_metadata_texture;
        let width = TEXTURE_METADATA_TEXTURE_WIDTH;
        let height = texels.len() as i32 / (4 * TEXTURE_METADATA_TEXTURE_WIDTH);
        let rect = RectI::new(Vector2I::zero(), Vector2I::new(width, height));
        self.device.upload_to_texture(texture, rect, TextureDataRef::F16(&texels));
    }

    fn upload_scene(&mut self, draw_segments: &Segments, clip_segments: &Segments) {
        mem::swap(&mut self.front_scene_buffers, &mut self.back_scene_buffers);
        match self.back_scene_buffers {
            None => {
                self.back_scene_buffers = Some(SceneBuffers::new(&self.device,
                                                                 draw_segments,
                                                                 clip_segments))
            }
            Some(ref mut back_scene_buffers) => {
                back_scene_buffers.upload(&self.device, draw_segments, clip_segments)
            }
        } 
    }

    fn allocate_tiles(&mut self, tile_count: u32) -> StorageID {
        let device = &self.device;
        let tile_program = &self.tile_program;
        let tile_copy_program = &self.tile_copy_program;
        let quad_vertex_positions_buffer = &self.quad_vertex_positions_buffer;
        let quad_vertex_indices_buffer = &self.quad_vertex_indices_buffer;
        self.back_frame.storage_allocators.tile_vertex.allocate(tile_count as u64, |size| {
            TileVertexStorage::new(size,
                                   device,
                                   tile_program,
                                   tile_copy_program,
                                   quad_vertex_positions_buffer,
                                   quad_vertex_indices_buffer)
        })
    }

    fn upload_tiles(&mut self, storage_id: StorageID, tiles: &[TileObjectPrimitive]) {
        let vertex_buffer = &self.back_frame
                                 .storage_allocators
                                 .tile_vertex
                                 .get(storage_id)
                                 .vertex_buffer;
        self.device.upload_to_buffer(vertex_buffer, 0, tiles, BufferTarget::Vertex);

        self.ensure_index_buffer(tiles.len());
    }

    fn allocate_tile_link_map(&mut self, tile_count: u32) -> StorageID {
        self.back_frame.storage_allocators.tile_link_map.allocate_buffer(&self.device,
                                                                         tile_count as u64,
                                                                         BufferTarget::Storage)
    }

    fn initialize_tiles(&mut self,
                        tile_storage_id: StorageID,
                        tile_link_map_storage_id: StorageID,
                        tile_count: u32,
                        tile_path_info: &[TilePathInfo]) {
        let init_program = &self.d3d11_programs
                                .as_ref()
                                .expect("Initializing tiles on GPU requires D3D11 programs!")
                                .init_program;

        let path_info_storage_id = self.back_frame
                                       .storage_allocators
                                       .path_info
                                       .allocate_buffer(&self.device,
                                                        tile_path_info.len() as u64,
                                                        BufferTarget::Storage);
        let tile_path_info_buffer = self.back_frame
                                        .storage_allocators
                                        .path_info
                                        .get(path_info_storage_id);
        self.device.upload_to_buffer(&tile_path_info_buffer.buffer,
                                     0,
                                     tile_path_info,
                                     BufferTarget::Storage);

        let tiles_buffer = &self.back_frame
                                .storage_allocators
                                .tile_vertex
                                .get(tile_storage_id)
                                .vertex_buffer;

        // Allocate tile link map.
        let tile_link_map_buffer = &self.back_frame
                                        .storage_allocators
                                        .tile_link_map
                                        .get(tile_link_map_storage_id);

        let timer_query = self.timer_query_cache.alloc(&self.device);
        self.device.begin_timer_query(&timer_query);

        let compute_dimensions = ComputeDimensions { x: (tile_count + 63) / 64, y: 1, z: 1 };
        self.device.dispatch_compute(compute_dimensions, &ComputeState {
            program: &init_program.program,
            textures: &[],
            uniforms: &[
                (&init_program.path_count_uniform, UniformData::Int(tile_path_info.len() as i32)),
                (&init_program.tile_count_uniform, UniformData::Int(tile_count as i32)),
            ],
            images: &[],
            storage_buffers: &[
                (&init_program.tiles_storage_buffer, tiles_buffer),
                (&init_program.tile_path_info_storage_buffer, &tile_path_info_buffer.buffer),
                (&init_program.tile_link_map_storage_buffer, &tile_link_map_buffer.buffer),
            ],
        });

        self.device.end_timer_query(&timer_query);
        self.current_timer.as_mut().unwrap().other_times.push(TimerFuture::new(timer_query));
        self.stats.drawcall_count += 1;
    }

    fn upload_propagate_metadata(&mut self,
                                 propagate_metadata: &[PropagateMetadata],
                                 backdrops: &[BackdropInfo])
                                 -> PropagateMetadataStorageIDs {
        let device = &self.device;
        let propagate_metadata_storage_id = self.back_frame
                                                .storage_allocators
                                                .tile_propagate_metadata
                                                .allocate_buffer(device,
                                                                 propagate_metadata.len() as u64,
                                                                 BufferTarget::Storage);
        let propagate_metadata_buffer = &self.back_frame
                                             .storage_allocators
                                             .tile_propagate_metadata
                                             .get(propagate_metadata_storage_id)
                                             .buffer;
        device.upload_to_buffer(propagate_metadata_buffer,
                                0,
                                propagate_metadata,
                                BufferTarget::Storage);

        let backdrops_storage_id = self.back_frame
                                       .storage_allocators
                                       .backdrops
                                       .allocate_buffer(device,
                                                        backdrops.len() as u64,
                                                        BufferTarget::Storage);

        PropagateMetadataStorageIDs {
             propagate_metadata: propagate_metadata_storage_id,
             backdrops: backdrops_storage_id,
        }
    }

    fn upload_initial_backdrops(&self,
                                backdrops_storage_id: StorageID,
                                backdrops: &[BackdropInfo]) {
        let backdrops_buffer = &self.back_frame
                                    .storage_allocators
                                    .backdrops
                                    .get(backdrops_storage_id)
                                    .buffer;
        self.device.upload_to_buffer(&backdrops_buffer, 0, backdrops, BufferTarget::Storage);
    }

    fn ensure_index_buffer(&mut self, mut length: usize) {
        length = length.next_power_of_two();
        if self.back_frame.quads_vertex_indices_length >= length {
            return;
        }

        // TODO(pcwalton): Generate these with SIMD.
        let mut indices: Vec<u32> = Vec::with_capacity(length * 6);
        for index in 0..(length as u32) {
            indices.extend_from_slice(&[
                index * 4 + 0, index * 4 + 1, index * 4 + 2,
                index * 4 + 1, index * 4 + 3, index * 4 + 2,
            ]);
        }

        self.device.allocate_buffer(&self.back_frame.quads_vertex_indices_buffer,
                                    BufferData::Memory(&indices),
                                    BufferTarget::Index);

        self.back_frame.quads_vertex_indices_length = length;
    }

    fn dice_segments(&mut self,
                     dice_metadata: &[DiceMetadata],
                     batch_segment_count: u32,
                     path_source: PathSource,
                     transform: Transform2F)
                     -> Option<MicrolinesStorage> {
        let dice_compute_program = &self.d3d11_programs
                                        .as_ref()
                                        .expect("Dicing on GPU requires D3D11 programs!")
                                        .dice_compute_program;

        let microlines_storage_id = self.back_frame
                                        .storage_allocators
                                        .microlines
                                        .allocate_buffer(&self.device,
                                                         self.allocated_microline_count as u64,
                                                         BufferTarget::Storage);
        let microlines_buffer = &self.back_frame
                                     .storage_allocators
                                     .microlines
                                     .get(microlines_storage_id)
                                     .buffer;

        let dice_metadata_storage_id = {
            let device = &self.device;
            self.back_frame
                .storage_allocators
                .dice_metadata
                .allocate(dice_metadata.len() as u64,
                          |size| DiceMetadataStorage::new(device, size))
        };
        let dice_metadata_storage = self.back_frame
                                        .storage_allocators
                                        .dice_metadata
                                        .get(dice_metadata_storage_id);

        let back_scene_buffers = self.back_scene_buffers
                                     .as_ref()
                                     .expect("Where's the back scene?");
        let back_scene_source_buffers = match path_source {
            PathSource::Draw => &back_scene_buffers.draw,
            PathSource::Clip => &back_scene_buffers.clip,
        };
        let SceneSourceBuffers {
            ref points_buffer,
            ref point_indices_buffer,
            point_indices_count,
            ..
        } = *back_scene_source_buffers;

        self.device.upload_to_buffer(&dice_metadata_storage.indirect_draw_params_buffer,
                                     0,
                                     &[0, 0, 0, 0, point_indices_count, 0, 0, 0],
                                     BufferTarget::Storage);
        self.device.upload_to_buffer(&dice_metadata_storage.metadata_buffer,
                                     0,
                                     dice_metadata,
                                     BufferTarget::Storage);

        let timer_query = self.timer_query_cache.alloc(&self.device);
        self.device.begin_timer_query(&timer_query);

        let workgroup_count = (batch_segment_count + 63) / 64;
        let compute_dimensions = ComputeDimensions { x: workgroup_count, y: 1, z: 1 };

        self.device.dispatch_compute(compute_dimensions, &ComputeState {
            program: &dice_compute_program.program,
            textures: &[],
            uniforms: &[
                (&dice_compute_program.transform_uniform, UniformData::Mat2(transform.matrix.0)),
                (&dice_compute_program.translation_uniform, UniformData::Vec2(transform.vector.0)),
                (&dice_compute_program.path_count_uniform,
                 UniformData::Int(dice_metadata.len() as i32)),
                (&dice_compute_program.last_batch_segment_index_uniform,
                 UniformData::Int(batch_segment_count as i32)),
                (&dice_compute_program.max_microline_count_uniform,
                 UniformData::Int(self.allocated_microline_count as i32)),
            ],
            images: &[],
            storage_buffers: &[
                (&dice_compute_program.compute_indirect_params_storage_buffer,
                 &dice_metadata_storage.indirect_draw_params_buffer),
                (&dice_compute_program.points_storage_buffer, points_buffer),
                (&dice_compute_program.input_indices_storage_buffer, point_indices_buffer),
                (&dice_compute_program.microlines_storage_buffer, microlines_buffer),
                (&dice_compute_program.dice_metadata_storage_buffer,
                 &dice_metadata_storage.metadata_buffer),
            ],
        });

        self.device.end_timer_query(&timer_query);
        self.current_timer.as_mut().unwrap().dice_times.push(TimerFuture::new(timer_query));
        self.stats.drawcall_count += 1;

        let indirect_compute_params_receiver =
            self.device.read_buffer(&dice_metadata_storage.indirect_draw_params_buffer,
                                    BufferTarget::Storage,
                                    0..32);
        let indirect_compute_params = self.device.recv_buffer(&indirect_compute_params_receiver);
        let indirect_compute_params: &[u32] = indirect_compute_params.as_slice_of().unwrap();

        let microline_count =
            indirect_compute_params[BIN_INDIRECT_DRAW_PARAMS_MICROLINE_COUNT_INDEX];
        if microline_count > self.allocated_microline_count {
            self.allocated_microline_count = microline_count.next_power_of_two();
            return None;
        }

        Some(MicrolinesStorage { storage_id: microlines_storage_id, count: microline_count })
    }

    fn bin_segments_via_compute(&mut self,
                                microlines_storage: &MicrolinesStorage,
                                propagate_metadata_storage_ids: &PropagateMetadataStorageIDs,
                                tile_storage_id: StorageID,
                                tile_link_map_storage_id: StorageID,
                                tile_count: u32)
                                -> Option<FillComputeStorageInfo> {
        let bin_compute_program = &self.d3d11_programs
                                       .as_ref()
                                       .expect("Binning on GPU requires D3D11 programs!")
                                       .bin_compute_program;

        let fill_storage_id = {
            let device = &self.device;
            let fill_program = &self.fill_program;
            let quad_vertex_positions_buffer = &self.quad_vertex_positions_buffer;
            let quad_vertex_indices_buffer = &self.quad_vertex_indices_buffer;
            let renderer_level = self.options.level;
            let allocated_fill_count = self.allocated_fill_count; 
            self.back_frame.storage_allocators.fill_vertex.allocate(allocated_fill_count as u64,
                                                                    |size| {
                FillVertexStorage::new(size,
                                       device,
                                       fill_program,
                                       quad_vertex_positions_buffer,
                                       quad_vertex_indices_buffer,
                                       renderer_level)
            })
        };
        let fill_vertex_storage = self.back_frame
                                      .storage_allocators
                                      .fill_vertex
                                      .get(fill_storage_id);

        let microlines_buffer = &self.back_frame
                                     .storage_allocators
                                     .microlines
                                     .get(microlines_storage.storage_id)
                                     .buffer;

        let alpha_tile_buffer = &self.back_frame
                                     .storage_allocators
                                     .tile_vertex
                                     .get(tile_storage_id)
                                     .vertex_buffer;
        let propagate_metadata_storage_buffer =
            self.back_frame
                .storage_allocators
                .tile_propagate_metadata
                .get(propagate_metadata_storage_ids.propagate_metadata);
        let backdrops_storage_buffer = &self.back_frame
                                            .storage_allocators
                                            .backdrops
                                            .get(propagate_metadata_storage_ids.backdrops)
                                            .buffer;

        let indirect_draw_params_buffer =
            &fill_vertex_storage.indirect_draw_params_buffer
                                .as_ref()
                                .expect("Where's the indirect draw params buffer?");
        let indirect_draw_params = [
            6, 0, 0, 0,
            self.back_frame.max_alpha_tile_index, microlines_storage.count, 0, 0,
        ];
        self.device.upload_to_buffer::<u32>(&indirect_draw_params_buffer,
                                            0,
                                            &indirect_draw_params,
                                            BufferTarget::Storage);

        let mut storage_buffers = vec![
            (&bin_compute_program.metadata_storage_buffer,
             &propagate_metadata_storage_buffer.buffer),
            (&bin_compute_program.fills_storage_buffer, &fill_vertex_storage.vertex_buffer),
            (&bin_compute_program.indirect_draw_params_storage_buffer,
             indirect_draw_params_buffer),
            (&bin_compute_program.tiles_storage_buffer, alpha_tile_buffer),
            (&bin_compute_program.microlines_storage_buffer, microlines_buffer),
            (&bin_compute_program.backdrops_storage_buffer, backdrops_storage_buffer),
        ];

        let tile_link_map_buffer = &self.back_frame
                                        .storage_allocators
                                        .tile_link_map
                                        .get(tile_link_map_storage_id)
                                        .buffer;

        storage_buffers.push((&bin_compute_program.tile_link_map_storage_buffer,
                              &tile_link_map_buffer));

        let timer_query = self.timer_query_cache.alloc(&self.device);
        self.device.begin_timer_query(&timer_query);

        let compute_dimensions = ComputeDimensions {
            x: (microlines_storage.count + 63) / 64,
            y: 1,
            z: 1,
        };

        self.device.dispatch_compute(compute_dimensions, &ComputeState {
            program: &bin_compute_program.program,
            textures: &[],
            uniforms: &[
                (&bin_compute_program.microline_count_uniform,
                 UniformData::Int(microlines_storage.count as i32)),
                (&bin_compute_program.max_fill_count_uniform,
                 UniformData::Int(self.allocated_fill_count as i32)),
            ],
            images: &[],
            storage_buffers: &storage_buffers,
        });

        self.device.end_timer_query(&timer_query);
        self.current_timer.as_mut().unwrap().bin_times.push(TimerFuture::new(timer_query));
        self.stats.drawcall_count += 1;

        let indirect_draw_params_receiver = self.device.read_buffer(&indirect_draw_params_buffer,
                                                                    BufferTarget::Storage,
                                                                    0..32);
        let indirect_draw_params = self.device.recv_buffer(&indirect_draw_params_receiver);
        let indirect_draw_params: &[u32] = indirect_draw_params.as_slice_of().unwrap();

        let needed_fill_count =
            indirect_draw_params[FILL_INDIRECT_DRAW_PARAMS_INSTANCE_COUNT_INDEX];
        if needed_fill_count > self.allocated_fill_count {
            self.allocated_fill_count = needed_fill_count.next_power_of_two();
            return None;
        }

        self.back_frame.max_alpha_tile_index =
            indirect_draw_params[FILL_INDIRECT_DRAW_PARAMS_ALPHA_TILE_COUNT_INDEX];

        self.stats.fill_count += needed_fill_count as usize;

        Some(FillComputeStorageInfo {
            fill_storage_id,
            tile_link_map_storage_id,
            // FIXME(pcwalton): Don't process all tiles!
            first_fill_tile: 0,
            fill_tile_count: tile_count,
        })
    }

    fn add_fills(&mut self, fill_batch: &[Fill]) {
        if fill_batch.is_empty() {
            return;
        }

        self.stats.fill_count += fill_batch.len();

        let preserve_alpha_mask_contents = self.back_frame.max_alpha_tile_index > 0;

        self.back_frame.pending_fills.reserve(fill_batch.len());
        for fill in fill_batch {
            self.back_frame.max_alpha_tile_index =
                self.back_frame.max_alpha_tile_index.max(fill.link + 1);
            self.back_frame.pending_fills.push(*fill);
        }

        self.reallocate_alpha_tile_pages_if_necessary(preserve_alpha_mask_contents);

        if self.back_frame.buffered_fills.len() + self.back_frame.pending_fills.len() >
                MAX_FILLS_PER_BATCH {
            self.draw_buffered_fills();
        }

        self.back_frame.buffered_fills.extend(self.back_frame.pending_fills.drain(..));
    }

    fn draw_buffered_fills(&mut self) {
        if self.back_frame.buffered_fills.is_empty() {
            return;
        }

        match self.fill_program {
            FillProgram::Raster(_) => {
                let fill_storage_info = self.upload_buffered_fills_for_raster();
                self.draw_fills_via_raster(fill_storage_info.fill_storage_id,
                                           fill_storage_info.fill_count);
            }
            FillProgram::Compute(_) => panic!("Can't draw buffered fills in compute!"),
        }
    }

    fn upload_buffered_fills_for_raster(&mut self) -> FillRasterStorageInfo {
        let buffered_fills = &mut self.back_frame.buffered_fills;
        debug_assert!(!buffered_fills.is_empty());

        let storage_id = {
            let device = &self.device;
            let fill_program = &self.fill_program;
            let quad_vertex_positions_buffer = &self.quad_vertex_positions_buffer;
            let quad_vertex_indices_buffer = &self.quad_vertex_indices_buffer;
            let renderer_level = self.options.level;

            self.back_frame.storage_allocators.fill_vertex.allocate(MAX_FILLS_PER_BATCH as u64,
                                                                    |size| {
                FillVertexStorage::new(size,
                                       device,
                                       fill_program,
                                       quad_vertex_positions_buffer,
                                       quad_vertex_indices_buffer,
                                       renderer_level)
            })
        };
        let fill_vertex_storage = self.back_frame.storage_allocators.fill_vertex.get(storage_id);

        debug_assert!(buffered_fills.len() <= u32::MAX as usize);
        self.device.upload_to_buffer(&fill_vertex_storage.vertex_buffer,
                                     0,
                                     &buffered_fills,
                                     BufferTarget::Vertex);

        let fill_count = buffered_fills.len() as u32;
        buffered_fills.clear();

        FillRasterStorageInfo { fill_storage_id: storage_id, fill_count }
    }

    fn draw_fills_via_raster(&mut self, fill_storage_id: StorageID, fill_count: u32) {
        let fill_raster_program = match self.fill_program {
            FillProgram::Raster(ref fill_raster_program) => fill_raster_program,
            _ => unreachable!(),
        };
        let mask_viewport = self.mask_viewport();
        let fill_vertex_storage = self.back_frame
                                      .storage_allocators
                                      .fill_vertex
                                      .get(fill_storage_id);
        let fill_vertex_array =
            fill_vertex_storage.vertex_array.as_ref().expect("Where's the vertex array?");

        let mut clear_color = None;
        if !self.back_frame
                .framebuffer_flags
                .contains(FramebufferFlags::MASK_FRAMEBUFFER_IS_DIRTY) {
            clear_color = Some(ColorF::default());
        };

        let timer_query = self.timer_query_cache.alloc(&self.device);
        self.device.begin_timer_query(&timer_query);

        self.device.draw_elements_instanced(6, fill_count, &RenderState {
            target: &RenderTarget::Framebuffer(self.back_frame
                                                   .mask_framebuffer
                                                   .as_ref()
                                                   .expect("Where's the mask framebuffer?")),
            program: &fill_raster_program.program,
            vertex_array: &fill_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            textures: &[(&fill_raster_program.area_lut_texture, &self.area_lut_texture)],
            uniforms: &[
                (&fill_raster_program.framebuffer_size_uniform,
                 UniformData::Vec2(mask_viewport.size().to_f32().0)),
                (&fill_raster_program.tile_size_uniform,
                 UniformData::Vec2(F32x2::new(TILE_WIDTH as f32, TILE_HEIGHT as f32))),
            ],
            images: &[],
            storage_buffers: &[],
            viewport: mask_viewport,
            options: RenderOptions {
                blend: Some(BlendState {
                    src_rgb_factor: BlendFactor::One,
                    src_alpha_factor: BlendFactor::One,
                    dest_rgb_factor: BlendFactor::One,
                    dest_alpha_factor: BlendFactor::One,
                    ..BlendState::default()
                }),
                clear_ops: ClearOps { color: clear_color, ..ClearOps::default() },
                ..RenderOptions::default()
            },
        });

        self.device.end_timer_query(&timer_query);
        self.current_timer.as_mut().unwrap().raster_times.push(TimerFuture::new(timer_query));
        self.stats.drawcall_count += 1;

        self.back_frame.framebuffer_flags.insert(FramebufferFlags::MASK_FRAMEBUFFER_IS_DIRTY);
    }

    fn draw_fills_via_compute(&mut self,
                              fill_storage_info: FillComputeStorageInfo,
                              tile_storage_id: StorageID) {
        let FillComputeStorageInfo {
            fill_storage_id,
            tile_link_map_storage_id,
            first_fill_tile,
            fill_tile_count,
        } = fill_storage_info;

        let fill_compute_program = match self.fill_program {
            FillProgram::Compute(ref fill_compute_program) => fill_compute_program,
            _ => unreachable!(),
        };

        let fill_vertex_storage = self.back_frame
                                      .storage_allocators
                                      .fill_vertex
                                      .get(fill_storage_id);

        let tile_link_map_buffer = &self.back_frame
                                        .storage_allocators
                                        .tile_link_map
                                        .get(tile_link_map_storage_id)
                                        .buffer;

        let mask_framebuffer = self.back_frame
                                   .mask_framebuffer
                                   .as_ref()
                                   .expect("Where's the mask framebuffer?");
        let image_texture = self.device.framebuffer_texture(mask_framebuffer);

        let timer_query = self.timer_query_cache.alloc(&self.device);
        self.device.begin_timer_query(&timer_query);

        let tiles_buffer = &self.back_frame
                                .storage_allocators
                                .tile_vertex
                                .get(tile_storage_id)
                                .vertex_buffer;

        // This setup is an annoying workaround for the 64K limit of compute invocation in OpenGL.
        let dimensions = ComputeDimensions {
            x: fill_tile_count.min(1 << 15),
            y: (fill_tile_count + (1 << 15) - 1) >> 15,
            z: 1,
        };
        let fill_tile_range = I32x2::new(0, fill_tile_count as i32) +
            I32x2::splat(first_fill_tile as i32);

        self.device.dispatch_compute(dimensions, &ComputeState {
            program: &fill_compute_program.program,
            textures: &[(&fill_compute_program.area_lut_texture, &self.area_lut_texture)],
            images: &[(&fill_compute_program.dest_image, image_texture, ImageAccess::Write)],
            uniforms: &[
                (&fill_compute_program.tile_range_uniform, UniformData::IVec2(fill_tile_range)),
            ],
            storage_buffers: &[
                (&fill_compute_program.fills_storage_buffer, &fill_vertex_storage.vertex_buffer),
                (&fill_compute_program.tile_link_map_storage_buffer, tile_link_map_buffer),
                (&fill_compute_program.tiles_storage_buffer, &tiles_buffer),
            ],
        });

        self.device.end_timer_query(&timer_query);
        self.current_timer.as_mut().unwrap().raster_times.push(TimerFuture::new(timer_query));
        self.stats.drawcall_count += 1;

        self.back_frame.framebuffer_flags.insert(FramebufferFlags::MASK_FRAMEBUFFER_IS_DIRTY);
    }

    fn clip_tiles(&mut self, clip_storage_id: StorageID, max_clipped_tile_count: u32) {
        let mask_framebuffer = self.back_frame
                                   .mask_framebuffer
                                   .as_ref()
                                   .expect("Where's the mask framebuffer?");
        let mask_texture = self.device.framebuffer_texture(mask_framebuffer);
        let mask_texture_size = self.device.texture_size(&mask_texture);

        // Allocate temp mask framebuffer if necessary.
        match self.back_frame.mask_temp_framebuffer {
            Some(ref mask_temp_framebuffer) if
                self.device.texture_size(
                    self.device.framebuffer_texture(
                        mask_temp_framebuffer)).y() >= mask_texture_size.y() => {}
            _ => {
                let mask_temp_texture = self.device.create_texture(TextureFormat::RGBA16F,
                                                                   mask_texture_size);
                self.back_frame.mask_temp_framebuffer =
                    Some(self.device.create_framebuffer(mask_temp_texture));
            }
        }
        let mask_temp_framebuffer = self.back_frame.mask_temp_framebuffer.as_ref().unwrap();

        let clip_vertex_storage = self.back_frame
                                      .storage_allocators
                                      .clip_vertex
                                      .get(clip_storage_id);

        let timer_query = self.timer_query_cache.alloc(&self.device);
        self.device.begin_timer_query(&timer_query);

        // Copy out tiles.
        //
        // TODO(pcwalton): Don't do this on GL4.
        self.device.draw_elements_instanced(6, max_clipped_tile_count * 2, &RenderState {
            target: &RenderTarget::Framebuffer(mask_temp_framebuffer),
            program: &self.tile_clip_copy_program.program,
            vertex_array: &clip_vertex_storage.tile_clip_copy_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            textures: &[
                (&self.tile_clip_copy_program.src_texture,
                 self.device.framebuffer_texture(mask_framebuffer)),
            ],
            images: &[],
            uniforms: &[
                (&self.tile_clip_copy_program.framebuffer_size_uniform,
                 UniformData::Vec2(mask_texture_size.to_f32().0)),
            ],
            storage_buffers: &[],
            viewport: RectI::new(Vector2I::zero(), mask_texture_size),
            options: RenderOptions::default(),
        });

        // Combine clip tiles.
        self.device.draw_elements_instanced(6, max_clipped_tile_count, &RenderState {
            target: &RenderTarget::Framebuffer(mask_framebuffer),
            program: &self.tile_clip_combine_program.program,
            vertex_array: &clip_vertex_storage.tile_clip_combine_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            textures: &[
                (&self.tile_clip_combine_program.src_texture,
                 self.device.framebuffer_texture(&mask_temp_framebuffer)),
            ],
            images: &[],
            uniforms: &[
                (&self.tile_clip_combine_program.framebuffer_size_uniform,
                 UniformData::Vec2(mask_texture_size.to_f32().0)),
            ],
            storage_buffers: &[],
            viewport: RectI::new(Vector2I::zero(), mask_texture_size),
            options: RenderOptions::default(),
        });

        self.device.end_timer_query(&timer_query);
        self.current_timer.as_mut().unwrap().raster_times.push(TimerFuture::new(timer_query));
        self.stats.drawcall_count += 2;
    }

    // Computes backdrops, performs clipping, and populates Z buffers on GPU.
    fn prepare_tiles(&mut self, batch: &PrepareTilesBatch) {
        self.stats.tile_count += batch.tile_count as usize;

        // Upload tiles to GPU or allocate them as appropriate.
        let tile_vertex_storage_id = self.allocate_tiles(batch.tile_count);
        let tile_link_map_storage_id = match batch.modal {
            PrepareTilesModalInfo::CPU(ref cpu_info) => {
                self.upload_tiles(tile_vertex_storage_id, &cpu_info.tiles);
                None
            }
            PrepareTilesModalInfo::GPU(_) => Some(self.allocate_tile_link_map(batch.tile_count)),
        };

        // Fetch and/or allocate clip storage as needed.
        let clip_storage_ids = match batch.clipped_path_info {
            Some(ref clipped_path_info) => {
                let clip_batch_id = clipped_path_info.clip_batch_id;
                let clip_tile_batch_info =
                    self.back_frame.tile_batch_info[clip_batch_id.0 as usize];
                Some(ClipStorageIDs {
                    metadata: clip_tile_batch_info.d3d11_info.as_ref().map(|d3d11_info| {
                        d3d11_info.propagate_metadata_storage_id
                    }),
                    tiles: clip_tile_batch_info.tile_vertex_storage_id,
                    vertices: self.allocate_clip_storage(clipped_path_info.max_clipped_tile_count),
                })
            }
            None => None,
        };

        // Allocate a Z-buffer.
        let z_buffer_storage_id = self.allocate_z_buffer();

        // Propagate backdrops, bin fills, render fills, and/or perform clipping on GPU if
        // necessary.
        let d3d11_info = match batch.modal {
            PrepareTilesModalInfo::CPU(_) => None,
            PrepareTilesModalInfo::GPU(ref gpu_info) => {
                // Allocate space for tile lists.
                let first_tile_map_storage_id = self.allocate_first_tile_map();

                let propagate_metadata_storage_ids =
                    self.upload_propagate_metadata(&gpu_info.propagate_metadata,
                                                   &gpu_info.backdrops);

                // Dice (flatten) segments into microlines. We might have to do this twice if our
                // first attempt runs out of space in the storage buffer.
                let mut microlines_storage = None;
                for _ in 0..2 {
                    microlines_storage = self.dice_segments(&gpu_info.dice_metadata,
                                                            batch.segment_count,
                                                            batch.path_source,
                                                            gpu_info.transform);
                    if microlines_storage.is_some() {
                        break;
                    }
                }
                let microlines_storage =
                    microlines_storage.expect("Ran out of space for microlines when dicing!");

                // Initialize tiles, and bin segments. We might have to do this twice if our first
                // attempt runs out of space in the fill buffer.
                let mut fill_storage_info = None;
                let tile_link_map_storage_id =
                    tile_link_map_storage_id.expect("Where's the tile link map?");
                for _ in 0..2 {
                    self.initialize_tiles(tile_vertex_storage_id,
                                          tile_link_map_storage_id,
                                          batch.tile_count,
                                          &gpu_info.tile_path_info);

                    self.upload_initial_backdrops(propagate_metadata_storage_ids.backdrops,
                                                  &gpu_info.backdrops);

                    fill_storage_info =
                        self.bin_segments_via_compute(&microlines_storage,
                                                      &propagate_metadata_storage_ids,
                                                      tile_vertex_storage_id,
                                                      tile_link_map_storage_id,
                                                      batch.tile_count);
                    if fill_storage_info.is_some() {
                        break;
                    }
                }
                let fill_storage_info =
                    fill_storage_info.expect("Ran out of space for fills when binning!");

                // FIXME(pcwalton): Don't unconditionally pass true for copying here.
                self.reallocate_alpha_tile_pages_if_necessary(true);
                self.draw_fills_via_compute(fill_storage_info, tile_vertex_storage_id);

                self.propagate_tiles(gpu_info.backdrops.len() as u32,
                                     tile_vertex_storage_id,
                                     z_buffer_storage_id,
                                     tile_link_map_storage_id,
                                     first_tile_map_storage_id,
                                     &propagate_metadata_storage_ids,
                                     clip_storage_ids.as_ref());

                self.sort_tiles(tile_link_map_storage_id,
                                first_tile_map_storage_id);

                Some(TileBatchInfoD3D11 {
                    propagate_metadata_storage_id:
                        propagate_metadata_storage_ids.propagate_metadata,
                    tile_link_map_storage_id,
                    first_tile_map_storage_id,
                })
            }
        };

        // Record tile batch info.
        self.back_frame.tile_batch_info.insert(batch.batch_id.0 as usize, TileBatchInfo {
            tile_count: batch.tile_count,
            tile_vertex_storage_id,
            z_buffer_storage_id,
            d3d11_info,
        });

        // Prepare or upload the Z-buffers as necessary.
        match batch.modal {
            PrepareTilesModalInfo::GPU(_) => self.prepare_z_buffer(z_buffer_storage_id),
            PrepareTilesModalInfo::CPU(ref cpu_info) => {
                self.upload_z_buffer(z_buffer_storage_id, &cpu_info.z_buffer)
            }
        }

        // Perform clipping if necessary.
        if let (Some(clip_storage_ids), Some(clipped_path_info)) =
                (clip_storage_ids.as_ref(), batch.clipped_path_info.as_ref()) {
            // Upload clip tiles to GPU if they were computed on CPU.
            if clip_storage_ids.metadata.is_none() {
                let clips = clipped_path_info.clips.as_ref().expect("Where are the clips?");
                self.upload_clip_tiles(clip_storage_ids.vertices, clips);
            }

            self.clip_tiles(clip_storage_ids.vertices, clipped_path_info.max_clipped_tile_count);
        }
    }

    fn tile_transform(&self) -> Transform4F {
        let draw_viewport = self.draw_viewport().size().to_f32();
        let scale = Vector4F::new(2.0 / draw_viewport.x(), -2.0 / draw_viewport.y(), 1.0, 1.0);
        Transform4F::from_scale(scale).translate(Vector4F::new(-1.0, 1.0, 0.0, 1.0))
    }

    fn propagate_tiles(&mut self,
                       column_count: u32,
                       tile_storage_id: StorageID,
                       z_buffer_storage_id: StorageID,
                       tile_link_map_storage_id: StorageID,
                       first_tile_map_storage_id: StorageID,
                       propagate_metadata_storage_ids: &PropagateMetadataStorageIDs,
                       clip_storage_ids: Option<&ClipStorageIDs>) {
        let propagate_program = &self.d3d11_programs
                                     .as_ref()
                                     .expect("GPU tile propagation requires D3D11 programs!")
                                     .propagate_program;

        let alpha_tile_buffer = &self.back_frame
                                     .storage_allocators
                                     .tile_vertex
                                     .get(tile_storage_id)
                                     .vertex_buffer;
        let propagate_metadata_storage_buffer =
            &self.back_frame
                 .storage_allocators
                 .tile_propagate_metadata
                 .get(propagate_metadata_storage_ids.propagate_metadata)
                 .buffer;
        let backdrops_storage_buffer = &self.back_frame
                                            .storage_allocators
                                            .backdrops
                                            .get(propagate_metadata_storage_ids.backdrops)
                                            .buffer;

        // TODO(pcwalton): Zero out the Z-buffer on GPU?
        let z_buffer = self.back_frame.storage_allocators.z_buffers.get(z_buffer_storage_id);
        let z_buffer_size =
            self.device.texture_size(self.device.framebuffer_texture(&z_buffer.framebuffer));
        let tile_area = z_buffer_size.area() as usize;
        self.device.upload_to_buffer::<i32>(z_buffer.buffer
                                                    .as_ref()
                                                    .expect("Where's the Z-buffer?"),
                                            0,
                                            &vec![0; tile_area],
                                            BufferTarget::Storage);

        let tile_link_map_storage_buffer = &self.back_frame
                                                .storage_allocators
                                                .tile_link_map
                                                .get(tile_link_map_storage_id)
                                                .buffer;

        // TODO(pcwalton): Initialize the first tiles buffer on GPU?
        let first_tile_map_storage_buffer = &self.back_frame
                                                 .storage_allocators
                                                 .first_tile_map
                                                 .get(first_tile_map_storage_id)
                                                 .buffer;
        self.device.upload_to_buffer::<FirstTile>(&first_tile_map_storage_buffer,
                                                  0,
                                                  &vec![FirstTile::default(); tile_area],
                                                  BufferTarget::Storage);

        let mut storage_buffers = vec![
            (&propagate_program.draw_metadata_storage_buffer, propagate_metadata_storage_buffer),
            (&propagate_program.backdrops_storage_buffer, &backdrops_storage_buffer),
            (&propagate_program.draw_tiles_storage_buffer, alpha_tile_buffer),
            (&propagate_program.z_buffer_storage_buffer,
             z_buffer.buffer.as_ref().expect("Where's the Z-buffer?")),
            (&propagate_program.tile_link_map_storage_buffer, tile_link_map_storage_buffer),
            (&propagate_program.first_tile_map_storage_buffer, first_tile_map_storage_buffer),
        ];

        if let Some(clip_storage_ids) = clip_storage_ids {
            let clip_metadata_storage_id =
                clip_storage_ids.metadata.expect("Where's the clip metadata storage?");
            let clip_metadata_buffer = &self.back_frame
                                            .storage_allocators
                                            .tile_propagate_metadata
                                            .get(clip_metadata_storage_id)
                                            .buffer;
            let clip_tile_buffer = &self.back_frame
                                        .storage_allocators
                                        .tile_vertex
                                        .get(clip_storage_ids.tiles)
                                        .vertex_buffer;
            let clip_vertex_storage = self.back_frame
                                          .storage_allocators
                                          .clip_vertex
                                          .get(clip_storage_ids.vertices);
            storage_buffers.push((&propagate_program.clip_metadata_storage_buffer,
                                  clip_metadata_buffer));
            storage_buffers.push((&propagate_program.clip_tiles_storage_buffer,
                                  clip_tile_buffer));
            storage_buffers.push((&propagate_program.clip_vertex_storage_buffer,
                                  &clip_vertex_storage.vertex_buffer));
        }

        let timer_query = self.timer_query_cache.alloc(&self.device);
        self.device.begin_timer_query(&timer_query);

        let dimensions = ComputeDimensions {
            x: (column_count + PROPAGATE_WORKGROUP_SIZE - 1) / PROPAGATE_WORKGROUP_SIZE,
            y: 1,
            z: 1,
        };
        self.device.dispatch_compute(dimensions, &ComputeState {
            program: &propagate_program.program,
            textures: &[],
            images: &[],
            uniforms: &[
                (&propagate_program.framebuffer_tile_size_uniform,
                 UniformData::IVec2(self.framebuffer_tile_size().0)),
                (&propagate_program.column_count_uniform, UniformData::Int(column_count as i32)),
            ],
            storage_buffers: &storage_buffers,
        });

        self.device.end_timer_query(&timer_query);
        self.current_timer.as_mut().unwrap().other_times.push(TimerFuture::new(timer_query));
        self.stats.drawcall_count += 1;
    }

    fn sort_tiles(&mut self,
                  tile_link_map_storage_id: StorageID,
                  first_tile_map_storage_id: StorageID) {
        let sort_program = &self.d3d11_programs
                                .as_ref()
                                .expect("Tile sorting requires D3D11 programs!")
                                .sort_program;

        let tile_link_map_buffer = &self.back_frame
                                        .storage_allocators
                                        .tile_link_map
                                        .get(tile_link_map_storage_id)
                                        .buffer;
        let first_tile_map_buffer = &self.back_frame
                                         .storage_allocators
                                         .first_tile_map
                                         .get(first_tile_map_storage_id)
                                         .buffer;

        let tile_count = self.framebuffer_tile_size().area();

        let timer_query = self.timer_query_cache.alloc(&self.device);
        self.device.begin_timer_query(&timer_query);

        let dimensions = ComputeDimensions {
            x: (tile_count as u32 + SORT_WORKGROUP_SIZE - 1) / SORT_WORKGROUP_SIZE,
            y: 1,
            z: 1,
        };
        self.device.dispatch_compute(dimensions, &ComputeState {
            program: &sort_program.program,
            textures: &[],
            images: &[],
            uniforms: &[
                (&sort_program.tile_count_uniform, UniformData::Int(tile_count)),
            ],
            storage_buffers: &[
                (&sort_program.tile_link_map_storage_buffer, &tile_link_map_buffer),
                (&sort_program.first_tile_map_storage_buffer, &first_tile_map_buffer),
            ],
        });

        self.device.end_timer_query(&timer_query);
        self.current_timer.as_mut().unwrap().other_times.push(TimerFuture::new(timer_query));
        self.stats.drawcall_count += 1;
    }

    fn prepare_z_buffer(&mut self, z_buffer_storage_id: StorageID) {
        let blit_buffer_program =
            &self.d3d11_programs
                 .as_ref()
                 .expect("Preparing the Z-buffer on GPU requires D3D11 programs!")
                 .blit_buffer_program;
        let blit_buffer_vertex_array = &self.back_frame  
                                            .blit_buffer_vertex_array
                                            .as_ref()
                                            .expect("GPU tile postprocessing is disabled!")
                                            .vertex_array;

        let z_buffer = self.back_frame.storage_allocators.z_buffers.get(z_buffer_storage_id);

        let timer_query = self.timer_query_cache.alloc(&self.device);
        self.device.begin_timer_query(&timer_query);

        let z_buffer_size = self.framebuffer_tile_size();

        self.device.draw_elements(6, &RenderState {
            target: &RenderTarget::Framebuffer(&z_buffer.framebuffer),
            program: &blit_buffer_program.program,
            vertex_array: blit_buffer_vertex_array,
            primitive: Primitive::Triangles,
            textures: &[],
            images: &[],
            storage_buffers: &[
                (&blit_buffer_program.buffer_storage_buffer,
                 z_buffer.buffer.as_ref().expect("Where's the Z-buffer?")),
            ],
            uniforms: &[
                (&blit_buffer_program.buffer_size_uniform, UniformData::IVec2(z_buffer_size.0)),
                 
            ],
            viewport: RectI::new(Vector2I::zero(), z_buffer_size),
            options: RenderOptions::default(),
        });

        self.device.end_timer_query(&timer_query);
        self.current_timer.as_mut().unwrap().other_times.push(TimerFuture::new(timer_query));
        self.stats.drawcall_count += 1;
    }

    fn allocate_z_buffer(&mut self) -> StorageID {
        self.back_frame.storage_allocators.z_buffers.allocate(&self.device,
                                                              self.level(),
                                                              self.draw_viewport().size())
    }

    fn allocate_first_tile_map(&mut self) -> StorageID {
        let framebuffer_size = self.draw_viewport().size();
        let area = ((framebuffer_size.x() as u64 + TILE_WIDTH as u64 - 1) / TILE_WIDTH as u64) *
                   ((framebuffer_size.y() as u64 + TILE_HEIGHT as u64 - 1) / TILE_HEIGHT as u64);
        self.back_frame
            .storage_allocators
            .first_tile_map
            .allocate_buffer(&self.device, area, BufferTarget::Storage)
    }

    fn upload_z_buffer(&mut self,
                       z_buffer_storage_id: StorageID,
                       z_buffer_map: &DenseTileMap<i32>) {
        let z_buffer = self.back_frame.storage_allocators.z_buffers.get(z_buffer_storage_id);
        let z_buffer_texture = self.device.framebuffer_texture(&z_buffer.framebuffer);
        debug_assert_eq!(z_buffer_map.rect.origin(), Vector2I::default());
        debug_assert_eq!(z_buffer_map.rect.size(), self.device.texture_size(z_buffer_texture));
        let z_data: &[u8] = z_buffer_map.data.as_byte_slice();
        self.device.upload_to_texture(z_buffer_texture,
                                      z_buffer_map.rect,
                                      TextureDataRef::U8(&z_data));
    }

    fn allocate_clip_storage(&mut self, max_clipped_tile_count: u32) -> StorageID {
        let device = &self.device;
        let tile_clip_combine_program = &self.tile_clip_combine_program;
        let tile_clip_copy_program = &self.tile_clip_copy_program;
        let quad_vertex_positions_buffer = &self.quad_vertex_positions_buffer;
        let quad_vertex_indices_buffer = &self.quad_vertex_indices_buffer;
        self.back_frame.storage_allocators.clip_vertex.allocate(max_clipped_tile_count as u64,
                                                                |size| {
            ClipVertexStorage::new(size,
                                   device,
                                   tile_clip_combine_program,
                                   tile_clip_copy_program,
                                   quad_vertex_positions_buffer,
                                   quad_vertex_indices_buffer)
        })
    }

    // Uploads clip tiles from CPU to GPU.
    fn upload_clip_tiles(&mut self, clip_vertex_storage_id: StorageID, clips: &[Clip]) {
        let clip_vertex_storage = self.back_frame
                                      .storage_allocators
                                      .clip_vertex
                                      .get(clip_vertex_storage_id);
        self.device.upload_to_buffer(&clip_vertex_storage.vertex_buffer,
                                     0,
                                     clips,
                                     BufferTarget::Vertex);
    }

    fn draw_tiles(&mut self,
                  tile_count: u32,
                  tile_storage_id: StorageID,
                  color_texture_0: Option<TileBatchTexture>,
                  blend_mode: BlendMode,
                  filter: Filter,
                  z_buffer_storage_id: StorageID,
                  d3d11_info: Option<TileBatchInfoD3D11>) {
        match self.tile_program {
            TileProgram::Raster(_) => {
                self.draw_tiles_via_raster(tile_count,
                                           tile_storage_id,
                                           color_texture_0,
                                           blend_mode,
                                           filter,
                                           z_buffer_storage_id)
            }
            TileProgram::Compute(_) => {
                let d3d11_info = d3d11_info.expect("No D3D11 info present!");
                self.draw_tiles_via_compute(tile_count,
                                            tile_storage_id,
                                            d3d11_info.tile_link_map_storage_id,
                                            d3d11_info.first_tile_map_storage_id,
                                            color_texture_0,
                                            blend_mode,
                                            filter,
                                            z_buffer_storage_id)
            }
        }
    }

    fn draw_tiles_via_raster(&mut self,
                             tile_count: u32,
                             storage_id: StorageID,
                             color_texture_0: Option<TileBatchTexture>,
                             blend_mode: BlendMode,
                             filter: Filter,
                             z_buffer_storage_id: StorageID) {
        // TODO(pcwalton): Disable blend for solid tiles.

        let needs_readable_framebuffer = blend_mode.needs_readable_framebuffer();
        if needs_readable_framebuffer {
            self.copy_alpha_tiles_to_dest_blend_texture(tile_count, storage_id);
        }

        let clear_color = self.clear_color_for_draw_operation();
        let draw_viewport = self.draw_viewport();

        let timer_query = self.timer_query_cache.alloc(&self.device);
        self.device.begin_timer_query(&timer_query);

        let tile_raster_program = match self.tile_program {
            TileProgram::Raster(ref tile_raster_program) => tile_raster_program,
            TileProgram::Compute(_) => unreachable!(),
        };

        let (mut textures, mut uniforms) = (vec![], vec![]);

        self.set_uniforms_for_drawing_tiles(&tile_raster_program.common,
                                            &mut textures,
                                            &mut uniforms,
                                            color_texture_0,
                                            blend_mode,
                                            filter,
                                            z_buffer_storage_id);

        uniforms.push((&tile_raster_program.transform_uniform,
                       UniformData::Mat4(self.tile_transform().to_columns())));

        if needs_readable_framebuffer {
            textures.push((&tile_raster_program.dest_texture,
                           self.device
                               .framebuffer_texture(&self.back_frame.dest_blend_framebuffer)));
        }

        let vertex_array = &self.back_frame
                                .storage_allocators
                                .tile_vertex
                                .get(storage_id)
                                .tile_vertex_array
                                .as_ref()
                                .expect("No tile vertex array present!")
                                .vertex_array;

        self.device.draw_elements_instanced(6, tile_count, &RenderState {
            target: &self.draw_render_target(),
            program: &tile_raster_program.common.program,
            vertex_array,
            primitive: Primitive::Triangles,
            textures: &textures,
            images: &[],
            storage_buffers: &[],
            uniforms: &uniforms,
            viewport: draw_viewport,
            options: RenderOptions {
                blend: blend_mode.to_blend_state(),
                stencil: self.stencil_state(),
                clear_ops: ClearOps { color: clear_color, ..ClearOps::default() },
                ..RenderOptions::default()
            },
        });

        self.device.end_timer_query(&timer_query);
        self.current_timer.as_mut().unwrap().raster_times.push(TimerFuture::new(timer_query));
        self.stats.drawcall_count += 1;

        self.preserve_draw_framebuffer();
    }

    fn draw_tiles_via_compute(&mut self,
                              tile_count: u32,
                              tile_storage_id: StorageID,
                              tile_link_map_storage_id: StorageID,
                              first_tile_map_storage_id: StorageID,
                              color_texture_0: Option<TileBatchTexture>,
                              blend_mode: BlendMode,
                              filter: Filter,
                              z_buffer_storage_id: StorageID) {
        // TODO(pcwalton): Disable blend for solid tiles.

        let needs_readable_framebuffer = blend_mode.needs_readable_framebuffer();
        if needs_readable_framebuffer {
            self.copy_alpha_tiles_to_dest_blend_texture(tile_count, tile_storage_id);
        }

        let timer_query = self.timer_query_cache.alloc(&self.device);
        self.device.begin_timer_query(&timer_query);

        let tile_compute_program = match self.tile_program {
            TileProgram::Compute(ref tile_compute_program) => tile_compute_program,
            TileProgram::Raster(_) => unreachable!(),
        };

        let (mut textures, mut uniforms, mut images) = (vec![], vec![], vec![]);

        self.set_uniforms_for_drawing_tiles(&tile_compute_program.common,
                                            &mut textures,
                                            &mut uniforms,
                                            color_texture_0,
                                            blend_mode,
                                            filter,
                                            z_buffer_storage_id);

        uniforms.push((&tile_compute_program.framebuffer_tile_size_uniform,
                       UniformData::IVec2(self.framebuffer_tile_size().0)));

        match self.draw_render_target() {
            RenderTarget::Default => panic!("Can't draw to the default framebuffer with compute!"),
            RenderTarget::Framebuffer(ref framebuffer) => {
                let dest_texture = self.device.framebuffer_texture(framebuffer);
                images.push((&tile_compute_program.dest_image,
                             dest_texture,
                             ImageAccess::ReadWrite));
            }
        }

        let clear_color = self.clear_color_for_draw_operation();
        match clear_color {
            None => {
                uniforms.push((&tile_compute_program.load_action_uniform,
                               UniformData::Int(LOAD_ACTION_LOAD)));
                uniforms.push((&tile_compute_program.clear_color_uniform,
                               UniformData::Vec4(F32x4::default())));
            }
            Some(clear_color) => {
                uniforms.push((&tile_compute_program.load_action_uniform,
                               UniformData::Int(LOAD_ACTION_CLEAR)));
                uniforms.push((&tile_compute_program.clear_color_uniform,
                               UniformData::Vec4(clear_color.0)));
            }
        }

        let tiles_buffer = &self.back_frame
                                .storage_allocators
                                .tile_vertex
                                .get(tile_storage_id)
                                .vertex_buffer;
        let tile_link_map_storage_buffer = &self.back_frame
                                                .storage_allocators
                                                .tile_link_map
                                                .get(tile_link_map_storage_id)
                                                .buffer;
        let first_tile_map_storage_buffer = &self.back_frame
                                                 .storage_allocators
                                                 .first_tile_map
                                                 .get(first_tile_map_storage_id)
                                                 .buffer;

        let framebuffer_tile_size = self.framebuffer_tile_size().0;
        let compute_dimensions = ComputeDimensions {
            x: framebuffer_tile_size.x() as u32,
            y: framebuffer_tile_size.y() as u32,
            z: 1,
        };

        self.device.dispatch_compute(compute_dimensions, &ComputeState {
            program: &tile_compute_program.common.program,
            textures: &textures,
            images: &images,
            storage_buffers: &[
                (&tile_compute_program.tiles_storage_buffer, tiles_buffer),
                (&tile_compute_program.tile_link_map_storage_buffer, tile_link_map_storage_buffer),
                (&tile_compute_program.first_tile_map_storage_buffer,
                 first_tile_map_storage_buffer),
            ],
            uniforms: &uniforms,
        });

        self.device.end_timer_query(&timer_query);
        self.current_timer.as_mut().unwrap().raster_times.push(TimerFuture::new(timer_query));
        self.stats.drawcall_count += 1;

        self.preserve_draw_framebuffer();
    }

    fn set_uniforms_for_drawing_tiles<'a>(
            &'a self,
            tile_program: &'a TileProgramCommon<D>,
            textures: &mut Vec<TextureBinding<'a, D::TextureParameter, D::Texture>>,
            uniforms: &mut Vec<UniformBinding<'a, D::Uniform>>,
            color_texture_0: Option<TileBatchTexture>,
            blend_mode: BlendMode,
            filter: Filter,
            z_buffer_storage_id: StorageID) {
        let draw_viewport = self.draw_viewport();

        let z_buffer = self.back_frame.storage_allocators.z_buffers.get(z_buffer_storage_id);
        let z_buffer_texture = self.device.framebuffer_texture(&z_buffer.framebuffer);

        textures.push((&tile_program.texture_metadata_texture,
                       &self.back_frame.texture_metadata_texture));
        textures.push((&tile_program.z_buffer_texture, z_buffer_texture));

        uniforms.push((&tile_program.z_buffer_texture_size_uniform,
                       UniformData::IVec2(self.device.texture_size(z_buffer_texture).0)));
        uniforms.push((&tile_program.tile_size_uniform,
                       UniformData::Vec2(F32x2::new(TILE_WIDTH as f32, TILE_HEIGHT as f32))));
        uniforms.push((&tile_program.framebuffer_size_uniform,
                       UniformData::Vec2(draw_viewport.size().to_f32().0)));
        uniforms.push((&tile_program.texture_metadata_size_uniform,
                       UniformData::IVec2(I32x2::new(TEXTURE_METADATA_TEXTURE_WIDTH,
                                                     TEXTURE_METADATA_TEXTURE_HEIGHT))));

        if let Some(ref mask_framebuffer) = self.back_frame.mask_framebuffer {
            let mask_texture = self.device.framebuffer_texture(mask_framebuffer);
            uniforms.push((&tile_program.mask_texture_size_0_uniform,
                           UniformData::Vec2(self.device.texture_size(mask_texture).to_f32().0)));
            textures.push((&tile_program.mask_texture_0, mask_texture));
        }

        // TODO(pcwalton): Refactor.
        let mut ctrl = 0;
        match color_texture_0 {
            Some(color_texture) => {
                let color_texture_page = self.texture_page(color_texture.page);
                let color_texture_size = self.device.texture_size(color_texture_page).to_f32();
                self.device.set_texture_sampling_mode(color_texture_page,
                                                      color_texture.sampling_flags);
                textures.push((&tile_program.color_texture_0, color_texture_page));
                uniforms.push((&tile_program.color_texture_size_0_uniform,
                               UniformData::Vec2(color_texture_size.0)));

                ctrl |= color_texture.composite_op.to_combine_mode() <<
                    COMBINER_CTRL_COLOR_COMBINE_SHIFT;
            }
            None => {
                uniforms.push((&tile_program.color_texture_size_0_uniform,
                               UniformData::Vec2(F32x2::default())));
            }
        }

        ctrl |= blend_mode.to_composite_ctrl() << COMBINER_CTRL_COMPOSITE_SHIFT;

        match filter {
            Filter::None => self.set_uniforms_for_no_filter(tile_program, uniforms),
            Filter::RadialGradient { line, radii, uv_origin } => {
                ctrl |= COMBINER_CTRL_FILTER_RADIAL_GRADIENT << COMBINER_CTRL_COLOR_FILTER_SHIFT;
                self.set_uniforms_for_radial_gradient_filter(tile_program,
                                                             uniforms,
                                                             line,
                                                             radii,
                                                             uv_origin)
            }
            Filter::PatternFilter(PatternFilter::Text {
                fg_color,
                bg_color,
                defringing_kernel,
                gamma_correction,
            }) => {
                ctrl |= COMBINER_CTRL_FILTER_TEXT << COMBINER_CTRL_COLOR_FILTER_SHIFT;
                self.set_uniforms_for_text_filter(tile_program,
                                                  textures,
                                                  uniforms,
                                                  fg_color,
                                                  bg_color,
                                                  defringing_kernel,
                                                  gamma_correction);
            }
            Filter::PatternFilter(PatternFilter::Blur { direction, sigma }) => {
                ctrl |= COMBINER_CTRL_FILTER_BLUR << COMBINER_CTRL_COLOR_FILTER_SHIFT;
                self.set_uniforms_for_blur_filter(tile_program, uniforms, direction, sigma);
            }
        }

        uniforms.push((&tile_program.ctrl_uniform, UniformData::Int(ctrl)));
    }

    fn copy_alpha_tiles_to_dest_blend_texture(&mut self, tile_count: u32, storage_id: StorageID) {
        let draw_viewport = self.draw_viewport();

        let mut textures = vec![];
        let mut uniforms = vec![
            (&self.tile_copy_program.transform_uniform,
             UniformData::Mat4(self.tile_transform().to_columns())),
            (&self.tile_copy_program.tile_size_uniform,
             UniformData::Vec2(F32x2::new(TILE_WIDTH as f32, TILE_HEIGHT as f32))),
        ];

        let draw_framebuffer = match self.draw_render_target() {
            RenderTarget::Framebuffer(framebuffer) => framebuffer,
            RenderTarget::Default => panic!("Can't copy alpha tiles from default framebuffer!"),
        };
        let draw_texture = self.device.framebuffer_texture(&draw_framebuffer);

        textures.push((&self.tile_copy_program.src_texture, draw_texture));
        uniforms.push((&self.tile_copy_program.framebuffer_size_uniform,
                       UniformData::Vec2(draw_viewport.size().to_f32().0)));

        let vertex_array = &self.back_frame
                                .storage_allocators
                                .tile_vertex
                                .get(storage_id)
                                .tile_copy_vertex_array
                                .vertex_array;

        self.device.draw_elements(tile_count * 6, &RenderState {
            target: &RenderTarget::Framebuffer(&self.back_frame.dest_blend_framebuffer),
            program: &self.tile_copy_program.program,
            vertex_array,
            primitive: Primitive::Triangles,
            textures: &textures,
            images: &[],
            storage_buffers: &[],
            uniforms: &uniforms,
            viewport: draw_viewport,
            options: RenderOptions {
                clear_ops: ClearOps {
                    color: Some(ColorF::new(1.0, 0.0, 0.0, 1.0)),
                    ..ClearOps::default()
                },
                ..RenderOptions::default()
            },
        });

        self.stats.drawcall_count += 1;
    }

    fn draw_stencil(&mut self, quad_positions: &[Vector4F]) {
        self.device.allocate_buffer(&self.back_frame.stencil_vertex_array.vertex_buffer,
                                    BufferData::Memory(quad_positions),
                                    BufferTarget::Vertex);

        // Create indices for a triangle fan. (This is OK because the clipped quad should always be
        // convex.)
        let mut indices: Vec<u32> = vec![];
        for index in 1..(quad_positions.len() as u32 - 1) {
            indices.extend_from_slice(&[0, index as u32, index + 1]);
        }
        self.device.allocate_buffer(&self.back_frame.stencil_vertex_array.index_buffer,
                                    BufferData::Memory(&indices),
                                    BufferTarget::Index);

        self.device.draw_elements(indices.len() as u32, &RenderState {
            target: &self.draw_render_target(),
            program: &self.stencil_program.program,
            vertex_array: &self.back_frame.stencil_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            textures: &[],
            images: &[],
            storage_buffers: &[],
            uniforms: &[],
            viewport: self.draw_viewport(),
            options: RenderOptions {
                // FIXME(pcwalton): Should we really write to the depth buffer?
                depth: Some(DepthState { func: DepthFunc::Less, write: true }),
                stencil: Some(StencilState {
                    func: StencilFunc::Always,
                    reference: 1,
                    mask: 1,
                    write: true,
                }),
                color_mask: false,
                clear_ops: ClearOps { stencil: Some(0), ..ClearOps::default() },
                ..RenderOptions::default()
            },
        });

        self.stats.drawcall_count += 1;
    }

    pub fn reproject_texture(&mut self,
                             texture: &D::Texture,
                             old_transform: &Transform4F,
                             new_transform: &Transform4F) {
        let clear_color = self.clear_color_for_draw_operation();

        self.device.draw_elements(6, &RenderState {
            target: &self.draw_render_target(),
            program: &self.reprojection_program.program,
            vertex_array: &self.back_frame.reprojection_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            textures: &[(&self.reprojection_program.texture, texture)],
            images: &[],
            storage_buffers: &[],
            uniforms: &[
                (&self.reprojection_program.old_transform_uniform,
                 UniformData::from_transform_3d(old_transform)),
                (&self.reprojection_program.new_transform_uniform,
                 UniformData::from_transform_3d(new_transform)),
            ],
            viewport: self.draw_viewport(),
            options: RenderOptions {
                blend: BlendMode::SrcOver.to_blend_state(),
                depth: Some(DepthState { func: DepthFunc::Less, write: false, }),
                clear_ops: ClearOps { color: clear_color, ..ClearOps::default() },
                ..RenderOptions::default()
            },
        });

        self.stats.drawcall_count += 1;

        self.preserve_draw_framebuffer();
    }

    pub fn draw_render_target(&self) -> RenderTarget<D> {
        match self.render_target_stack.last() {
            Some(&render_target_id) => {
                let texture_page_id = self.render_target_location(render_target_id).page;
                let framebuffer = self.texture_page_framebuffer(texture_page_id);
                RenderTarget::Framebuffer(framebuffer)
            }
            None => {
                if self.flags.contains(RendererFlags::INTERMEDIATE_DEST_FRAMEBUFFER_NEEDED) {
                    RenderTarget::Framebuffer(&self.back_frame.intermediate_dest_framebuffer)
                } else {
                    match self.dest_framebuffer {
                        DestFramebuffer::Default { .. } => RenderTarget::Default,
                        DestFramebuffer::Other(ref framebuffer) => {
                            RenderTarget::Framebuffer(framebuffer)
                        }
                    }
                }
            }
        }
    }

    fn push_render_target(&mut self, render_target_id: RenderTargetId) {
        self.render_target_stack.push(render_target_id);
    }

    fn pop_render_target(&mut self) {
        self.render_target_stack.pop().expect("Render target stack underflow!");
    }

    fn set_uniforms_for_no_filter<'a>(&'a self,
                                      tile_program: &'a TileProgramCommon<D>,
                                      uniforms: &mut Vec<(&'a D::Uniform, UniformData)>) {
        uniforms.extend_from_slice(&[
            (&tile_program.filter_params_0_uniform, UniformData::Vec4(F32x4::default())),
            (&tile_program.filter_params_1_uniform, UniformData::Vec4(F32x4::default())),
            (&tile_program.filter_params_2_uniform, UniformData::Vec4(F32x4::default())),
        ]);
    }

    fn set_uniforms_for_radial_gradient_filter<'a>(
            &'a self,
            tile_program: &'a TileProgramCommon<D>,
            uniforms: &mut Vec<(&'a D::Uniform, UniformData)>,
            line: LineSegment2F,
            radii: F32x2,
            uv_origin: Vector2F) {
        uniforms.extend_from_slice(&[
            (&tile_program.filter_params_0_uniform,
             UniformData::Vec4(line.from().0.concat_xy_xy(line.vector().0))),
            (&tile_program.filter_params_1_uniform,
             UniformData::Vec4(radii.concat_xy_xy(uv_origin.0))),
            (&tile_program.filter_params_2_uniform, UniformData::Vec4(F32x4::default())),
        ]);
    }

    fn set_uniforms_for_text_filter<'a>(
            &'a self,
            tile_program: &'a TileProgramCommon<D>,
            textures: &mut Vec<TextureBinding<'a, D::TextureParameter, D::Texture>>,
            uniforms: &mut Vec<UniformBinding<'a, D::Uniform>>,
            fg_color: ColorF,
            bg_color: ColorF,
            defringing_kernel: Option<DefringingKernel>,
            gamma_correction: bool) {
        textures.push((&tile_program.gamma_lut_texture, &self.gamma_lut_texture));

        match defringing_kernel {
            Some(ref kernel) => {
                uniforms.push((&tile_program.filter_params_0_uniform,
                               UniformData::Vec4(F32x4::from_slice(&kernel.0))));
            }
            None => {
                uniforms.push((&tile_program.filter_params_0_uniform,
                               UniformData::Vec4(F32x4::default())));
            }
        }

        let mut params_2 = fg_color.0;
        params_2.set_w(gamma_correction as i32 as f32);

        uniforms.extend_from_slice(&[
            (&tile_program.filter_params_1_uniform, UniformData::Vec4(bg_color.0)),
            (&tile_program.filter_params_2_uniform, UniformData::Vec4(params_2)),
        ]);
    }

    fn set_uniforms_for_blur_filter<'a>(&'a self,
                                        tile_program: &'a TileProgramCommon<D>,
                                        uniforms: &mut Vec<(&'a D::Uniform, UniformData)>,
                                        direction: BlurDirection,
                                        sigma: f32) {
        let sigma_inv = 1.0 / sigma;
        let gauss_coeff_x = SQRT_2_PI_INV * sigma_inv;
        let gauss_coeff_y = f32::exp(-0.5 * sigma_inv * sigma_inv);
        let gauss_coeff_z = gauss_coeff_y * gauss_coeff_y;

        let src_offset = match direction {
            BlurDirection::X => vec2f(1.0, 0.0),
            BlurDirection::Y => vec2f(0.0, 1.0),
        };

        let support = f32::ceil(1.5 * sigma) * 2.0;

        uniforms.extend_from_slice(&[
            (&tile_program.filter_params_0_uniform,
             UniformData::Vec4(src_offset.0.concat_xy_xy(F32x2::new(support, 0.0)))),
            (&tile_program.filter_params_1_uniform,
             UniformData::Vec4(F32x4::new(gauss_coeff_x, gauss_coeff_y, gauss_coeff_z, 0.0))),
            (&tile_program.filter_params_2_uniform, UniformData::Vec4(F32x4::default())),
        ]);
    }

    fn clear_dest_framebuffer_if_necessary(&mut self) {
        let background_color = match self.options.background_color {
            None => return,
            Some(background_color) => background_color,
        };

        if self.back_frame
               .framebuffer_flags
               .contains(FramebufferFlags::DEST_FRAMEBUFFER_IS_DIRTY) {
            return;
        }

        let main_viewport = self.main_viewport();
        let uniforms = [
            (&self.clear_program.rect_uniform, UniformData::Vec4(main_viewport.to_f32().0)),
            (&self.clear_program.framebuffer_size_uniform,
             UniformData::Vec2(main_viewport.size().to_f32().0)),
            (&self.clear_program.color_uniform, UniformData::Vec4(background_color.0)),
        ];

        self.device.draw_elements(6, &RenderState {
            target: &RenderTarget::Default,
            program: &self.clear_program.program,
            vertex_array: &self.back_frame.clear_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            textures: &[],
            images: &[],
            storage_buffers: &[],
            uniforms: &uniforms[..],
            viewport: main_viewport,
            options: RenderOptions::default(),
        });

        self.stats.drawcall_count += 1;
    }

    fn blit_intermediate_dest_framebuffer_if_necessary(&mut self) {
        if !self.flags.contains(RendererFlags::INTERMEDIATE_DEST_FRAMEBUFFER_NEEDED) {
            return;
        }

        let main_viewport = self.main_viewport();

        let textures = [
            (&self.blit_program.src_texture,
             self.device.framebuffer_texture(&self.back_frame.intermediate_dest_framebuffer))
        ];

        self.device.draw_elements(6, &RenderState {
            target: &RenderTarget::Default,
            program: &self.blit_program.program,
            vertex_array: &self.back_frame.blit_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            textures: &textures[..],
            images: &[],
            storage_buffers: &[],
            uniforms: &[
                (&self.blit_program.framebuffer_size_uniform,
                 UniformData::Vec2(main_viewport.size().to_f32().0)),
                (&self.blit_program.dest_rect_uniform,
                 UniformData::Vec4(RectF::new(Vector2F::zero(), main_viewport.size().to_f32()).0)),
            ],
            viewport: main_viewport,
            options: RenderOptions {
                clear_ops: ClearOps {
                    color: Some(ColorF::new(0.0, 0.0, 0.0, 1.0)),
                    ..ClearOps::default()
                },
                ..RenderOptions::default()
            },
        });

        self.stats.drawcall_count += 1;
    }

    fn stencil_state(&self) -> Option<StencilState> {
        if !self.flags.contains(RendererFlags::USE_DEPTH) {
            return None;
        }

        Some(StencilState {
            func: StencilFunc::Equal,
            reference: 1,
            mask: 1,
            write: false,
        })
    }

    fn clear_color_for_draw_operation(&self) -> Option<ColorF> {
        let must_preserve_contents = match self.render_target_stack.last() {
            Some(&render_target_id) => {
                let texture_page = self.render_target_location(render_target_id).page;
                self.texture_pages[texture_page.0 as usize]
                    .as_ref()
                    .expect("Draw target texture page not allocated!")
                    .must_preserve_contents
            }
            None => {
                self.back_frame
                    .framebuffer_flags
                    .contains(FramebufferFlags::DEST_FRAMEBUFFER_IS_DIRTY)
            }
        };

        if must_preserve_contents {
            None
        } else if self.render_target_stack.is_empty() {
            self.options.background_color
        } else {
            Some(ColorF::default())
        }
    }

    fn preserve_draw_framebuffer(&mut self) {
        match self.render_target_stack.last() {
            Some(&render_target_id) => {
                let texture_page = self.render_target_location(render_target_id).page;
                self.texture_pages[texture_page.0 as usize]
                    .as_mut()
                    .expect("Draw target texture page not allocated!")
                    .must_preserve_contents = true;
            }
            None => {
                self.back_frame
                    .framebuffer_flags
                    .insert(FramebufferFlags::DEST_FRAMEBUFFER_IS_DIRTY);
            }
        }
    }

    pub fn draw_viewport(&self) -> RectI {
        match self.render_target_stack.last() {
            Some(&render_target_id) => self.render_target_location(render_target_id).rect,
            None => self.main_viewport(),
        }
    }

    fn main_viewport(&self) -> RectI {
        match self.dest_framebuffer {
            DestFramebuffer::Default { viewport, .. } => viewport,
            DestFramebuffer::Other(ref framebuffer) => {
                let size = self
                    .device
                    .texture_size(self.device.framebuffer_texture(framebuffer));
                RectI::new(Vector2I::default(), size)
            }
        }
    }

    fn mask_viewport(&self) -> RectI {
        let page_count = self.back_frame.allocated_alpha_tile_page_count as i32;
        let height = MASK_FRAMEBUFFER_HEIGHT * page_count;
        RectI::new(Vector2I::default(), vec2i(MASK_FRAMEBUFFER_WIDTH, height))
    }

    fn render_target_location(&self, render_target_id: RenderTargetId) -> TextureLocation {
        self.render_targets[render_target_id.render_target as usize].location
    }

    fn texture_page_framebuffer(&self, id: TexturePageId) -> &D::Framebuffer {
        &self.texture_pages[id.0 as usize]
             .as_ref()
             .expect("Texture page not allocated!")
             .framebuffer
    }

    fn texture_page(&self, id: TexturePageId) -> &D::Texture {
        self.device.framebuffer_texture(&self.texture_page_framebuffer(id))
    }

    fn framebuffer_tile_size(&self) -> Vector2I {
        pixel_size_to_tile_size(self.dest_framebuffer.window_size(&self.device))
    }
}

impl<D> Frame<D> where D: Device {
    // FIXME(pcwalton): This signature shouldn't be so big. Make a struct.
    fn new(device: &D,
           blit_program: &BlitProgram<D>,
           d3d11_programs: &Option<D3D11Programs<D>>,
           clear_program: &ClearProgram<D>,
           reprojection_program: &ReprojectionProgram<D>,
           stencil_program: &StencilProgram<D>,
           quad_vertex_positions_buffer: &D::Buffer,
           quad_vertex_indices_buffer: &D::Buffer,
           window_size: Vector2I)
           -> Frame<D> {
        let quads_vertex_indices_buffer = device.create_buffer(BufferUploadMode::Dynamic);

        let blit_vertex_array = BlitVertexArray::new(device,
                                                     &blit_program,
                                                     &quad_vertex_positions_buffer,
                                                     &quad_vertex_indices_buffer);
        let blit_buffer_vertex_array = d3d11_programs.as_ref().map(|tile_post_programs| {
            BlitBufferVertexArray::new(device,
                                       &tile_post_programs.blit_buffer_program,
                                       &quad_vertex_positions_buffer,
                                       &quad_vertex_indices_buffer)
        });
        let clear_vertex_array = ClearVertexArray::new(device,
                                                       &clear_program,
                                                       &quad_vertex_positions_buffer,
                                                       &quad_vertex_indices_buffer);
        let reprojection_vertex_array = ReprojectionVertexArray::new(device,
                                                                     &reprojection_program,
                                                                     &quad_vertex_positions_buffer,
                                                                     &quad_vertex_indices_buffer);
        let stencil_vertex_array = StencilVertexArray::new(device, &stencil_program);

        let storage_allocators = StorageAllocators::new();

        let texture_metadata_texture_size = vec2i(TEXTURE_METADATA_TEXTURE_WIDTH,
                                                  TEXTURE_METADATA_TEXTURE_HEIGHT);
        let texture_metadata_texture = device.create_texture(TextureFormat::RGBA16F,
                                                             texture_metadata_texture_size);

        let intermediate_dest_texture = device.create_texture(TextureFormat::RGBA8, window_size);
        let intermediate_dest_framebuffer = device.create_framebuffer(intermediate_dest_texture);

        let dest_blend_texture = device.create_texture(TextureFormat::RGBA8, window_size);
        let dest_blend_framebuffer = device.create_framebuffer(dest_blend_texture);

        Frame {
            blit_vertex_array,
            blit_buffer_vertex_array,
            clear_vertex_array,
            storage_allocators,
            reprojection_vertex_array,
            stencil_vertex_array,
            quads_vertex_indices_buffer,
            quads_vertex_indices_length: 0,
            texture_metadata_texture,
            buffered_fills: vec![],
            pending_fills: vec![],
            max_alpha_tile_index: 0,
            allocated_alpha_tile_page_count: 0,
            tile_batch_info: VecMap::new(),
            mask_framebuffer: None,
            mask_temp_framebuffer: None,
            intermediate_dest_framebuffer,
            dest_blend_framebuffer,
            framebuffer_flags: FramebufferFlags::empty(),
        }
    }

    fn gpu_bytes_allocated(&self) -> u64 {
        self.storage_allocators.gpu_bytes_allocated()
    }
}

#[derive(Clone, Copy)]
struct TileBatchInfo {
    tile_count: u32,
    tile_vertex_storage_id: StorageID,
    z_buffer_storage_id: StorageID,
    d3d11_info: Option<TileBatchInfoD3D11>,
}

#[derive(Clone, Copy)]
struct TileBatchInfoD3D11 {
    propagate_metadata_storage_id: StorageID,
    tile_link_map_storage_id: StorageID,
    first_tile_map_storage_id: StorageID,
}

// Render stats

bitflags! {
    struct FramebufferFlags: u8 {
        const MASK_FRAMEBUFFER_IS_DIRTY = 0x01;
        const DEST_FRAMEBUFFER_IS_DIRTY = 0x02;
    }
}

struct RenderTargetInfo {
    location: TextureLocation,
}

trait ToBlendState {
    fn to_blend_state(self) -> Option<BlendState>;
}

impl ToBlendState for BlendMode {
    fn to_blend_state(self) -> Option<BlendState> {
        match self {
            BlendMode::Clear => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::Zero,
                    dest_rgb_factor: BlendFactor::Zero,
                    src_alpha_factor: BlendFactor::Zero,
                    dest_alpha_factor: BlendFactor::Zero,
                    ..BlendState::default()
                })
            }
            BlendMode::SrcOver => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::One,
                    dest_rgb_factor: BlendFactor::OneMinusSrcAlpha,
                    src_alpha_factor: BlendFactor::One,
                    dest_alpha_factor: BlendFactor::OneMinusSrcAlpha,
                    ..BlendState::default()
                })
            }
            BlendMode::DestOver => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::OneMinusDestAlpha,
                    dest_rgb_factor: BlendFactor::One,
                    src_alpha_factor: BlendFactor::OneMinusDestAlpha,
                    dest_alpha_factor: BlendFactor::One,
                    ..BlendState::default()
                })
            }
            BlendMode::SrcIn => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::DestAlpha,
                    dest_rgb_factor: BlendFactor::Zero,
                    src_alpha_factor: BlendFactor::DestAlpha,
                    dest_alpha_factor: BlendFactor::Zero,
                    ..BlendState::default()
                })
            }
            BlendMode::DestIn => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::Zero,
                    dest_rgb_factor: BlendFactor::SrcAlpha,
                    src_alpha_factor: BlendFactor::Zero,
                    dest_alpha_factor: BlendFactor::SrcAlpha,
                    ..BlendState::default()
                })
            }
            BlendMode::SrcOut => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::OneMinusDestAlpha,
                    dest_rgb_factor: BlendFactor::Zero,
                    src_alpha_factor: BlendFactor::OneMinusDestAlpha,
                    dest_alpha_factor: BlendFactor::Zero,
                    ..BlendState::default()
                })
            }
            BlendMode::DestOut => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::Zero,
                    dest_rgb_factor: BlendFactor::OneMinusSrcAlpha,
                    src_alpha_factor: BlendFactor::Zero,
                    dest_alpha_factor: BlendFactor::OneMinusSrcAlpha,
                    ..BlendState::default()
                })
            }
            BlendMode::SrcAtop => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::DestAlpha,
                    dest_rgb_factor: BlendFactor::OneMinusSrcAlpha,
                    src_alpha_factor: BlendFactor::DestAlpha,
                    dest_alpha_factor: BlendFactor::OneMinusSrcAlpha,
                    ..BlendState::default()
                })
            }
            BlendMode::DestAtop => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::OneMinusDestAlpha,
                    dest_rgb_factor: BlendFactor::SrcAlpha,
                    src_alpha_factor: BlendFactor::OneMinusDestAlpha,
                    dest_alpha_factor: BlendFactor::SrcAlpha,
                    ..BlendState::default()
                })
            }
            BlendMode::Xor => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::OneMinusDestAlpha,
                    dest_rgb_factor: BlendFactor::OneMinusSrcAlpha,
                    src_alpha_factor: BlendFactor::OneMinusDestAlpha,
                    dest_alpha_factor: BlendFactor::OneMinusSrcAlpha,
                    ..BlendState::default()
                })
            }
            BlendMode::Lighter => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::One,
                    dest_rgb_factor: BlendFactor::One,
                    src_alpha_factor: BlendFactor::One,
                    dest_alpha_factor: BlendFactor::One,
                    ..BlendState::default()
                })
            }
            BlendMode::Copy |
            BlendMode::Darken |
            BlendMode::Lighten |
            BlendMode::Multiply |
            BlendMode::Screen |
            BlendMode::HardLight |
            BlendMode::Overlay |
            BlendMode::ColorDodge |
            BlendMode::ColorBurn |
            BlendMode::SoftLight |
            BlendMode::Difference |
            BlendMode::Exclusion |
            BlendMode::Hue |
            BlendMode::Saturation |
            BlendMode::Color |
            BlendMode::Luminosity => {
                // Blending is done manually in the shader.
                None
            }
        }
    }
}

pub trait BlendModeExt {
    fn needs_readable_framebuffer(self) -> bool;
}

impl BlendModeExt for BlendMode {
    fn needs_readable_framebuffer(self) -> bool {
        match self {
            BlendMode::Clear |
            BlendMode::SrcOver |
            BlendMode::DestOver |
            BlendMode::SrcIn |
            BlendMode::DestIn |
            BlendMode::SrcOut |
            BlendMode::DestOut |
            BlendMode::SrcAtop |
            BlendMode::DestAtop |
            BlendMode::Xor |
            BlendMode::Lighter |
            BlendMode::Copy => false,
            BlendMode::Lighten |
            BlendMode::Darken |
            BlendMode::Multiply |
            BlendMode::Screen |
            BlendMode::HardLight |
            BlendMode::Overlay |
            BlendMode::ColorDodge |
            BlendMode::ColorBurn |
            BlendMode::SoftLight |
            BlendMode::Difference |
            BlendMode::Exclusion |
            BlendMode::Hue |
            BlendMode::Saturation |
            BlendMode::Color |
            BlendMode::Luminosity => true,
        }
    }
}

bitflags! {
    struct RendererFlags: u8 {
        // Whether we need a depth buffer.
        const USE_DEPTH = 0x01;
        // Whether an intermediate destination framebuffer is needed.
        //
        // This will be true if any exotic blend modes are used at the top level (not inside a
        // render target), *and* the output framebuffer is the default framebuffer.
        const INTERMEDIATE_DEST_FRAMEBUFFER_NEEDED = 0x02;
    }
}

trait ToCompositeCtrl {
    fn to_composite_ctrl(&self) -> i32;
}

impl ToCompositeCtrl for BlendMode {
    fn to_composite_ctrl(&self) -> i32 {
        match *self {
            BlendMode::SrcOver |
            BlendMode::SrcAtop |
            BlendMode::DestOver |
            BlendMode::DestOut |
            BlendMode::Xor |
            BlendMode::Lighter |
            BlendMode::Clear |
            BlendMode::Copy |
            BlendMode::SrcIn |
            BlendMode::SrcOut |
            BlendMode::DestIn |
            BlendMode::DestAtop => COMBINER_CTRL_COMPOSITE_NORMAL,
            BlendMode::Multiply => COMBINER_CTRL_COMPOSITE_MULTIPLY,
            BlendMode::Darken => COMBINER_CTRL_COMPOSITE_DARKEN,
            BlendMode::Lighten => COMBINER_CTRL_COMPOSITE_LIGHTEN,
            BlendMode::Screen => COMBINER_CTRL_COMPOSITE_SCREEN,
            BlendMode::Overlay => COMBINER_CTRL_COMPOSITE_OVERLAY,
            BlendMode::ColorDodge => COMBINER_CTRL_COMPOSITE_COLOR_DODGE,
            BlendMode::ColorBurn => COMBINER_CTRL_COMPOSITE_COLOR_BURN,
            BlendMode::HardLight => COMBINER_CTRL_COMPOSITE_HARD_LIGHT,
            BlendMode::SoftLight => COMBINER_CTRL_COMPOSITE_SOFT_LIGHT,
            BlendMode::Difference => COMBINER_CTRL_COMPOSITE_DIFFERENCE,
            BlendMode::Exclusion => COMBINER_CTRL_COMPOSITE_EXCLUSION,
            BlendMode::Hue => COMBINER_CTRL_COMPOSITE_HUE,
            BlendMode::Saturation => COMBINER_CTRL_COMPOSITE_SATURATION,
            BlendMode::Color => COMBINER_CTRL_COMPOSITE_COLOR,
            BlendMode::Luminosity => COMBINER_CTRL_COMPOSITE_LUMINOSITY,
        }
    }
}

trait ToCombineMode {
    fn to_combine_mode(self) -> i32;
}

impl ToCombineMode for PaintCompositeOp {
    fn to_combine_mode(self) -> i32 {
        match self {
            PaintCompositeOp::DestIn => COMBINER_CTRL_COLOR_COMBINE_DEST_IN,
            PaintCompositeOp::SrcIn => COMBINER_CTRL_COLOR_COMBINE_SRC_IN,
        }
    }
}

fn pixel_size_to_tile_size(pixel_size: Vector2I) -> Vector2I {
    // Round up.
    let tile_size = vec2i(TILE_WIDTH as i32 - 1, TILE_HEIGHT as i32 - 1);
    let size = pixel_size + tile_size;
    vec2i(size.x() / TILE_WIDTH as i32, size.y() / TILE_HEIGHT as i32)
}

#[derive(Clone, Copy, Debug)]
struct ClipStorageIDs {
    metadata: Option<StorageID>,
    tiles: StorageID,
    vertices: StorageID,
}

#[derive(Clone)]
struct FillRasterStorageInfo {
    fill_storage_id: StorageID,
    fill_count: u32,
}

#[derive(Clone)]
struct FillComputeStorageInfo {
    fill_storage_id: StorageID,
    tile_link_map_storage_id: StorageID,
    fill_tile_count: u32,
    first_fill_tile: u32,
}

#[derive(Debug)]
struct PropagateMetadataStorageIDs {
    propagate_metadata: StorageID,
    backdrops: StorageID,
}

struct MicrolinesStorage {
    storage_id: StorageID,
    count: u32,
}

struct SceneBuffers<D> where D: Device {
    draw: SceneSourceBuffers<D>,
    clip: SceneSourceBuffers<D>,
}

struct SceneSourceBuffers<D> where D: Device {
    points_buffer: D::Buffer,
    points_capacity: u32,
    point_indices_buffer: D::Buffer,
    point_indices_count: u32,
    point_indices_capacity: u32,
}

impl<D> SceneBuffers<D> where D: Device {
    fn new(device: &D, draw_segments: &Segments, clip_segments: &Segments) -> SceneBuffers<D> {
        SceneBuffers {
            draw: SceneSourceBuffers::new(device, draw_segments),
            clip: SceneSourceBuffers::new(device, clip_segments),
        }
    }

    fn upload(&mut self, device: &D, draw_segments: &Segments, clip_segments: &Segments) {
        self.draw.upload(device, draw_segments);
        self.clip.upload(device, clip_segments);
    }
}

impl<D> SceneSourceBuffers<D> where D: Device {
    fn new(device: &D, segments: &Segments) -> SceneSourceBuffers<D> {
        let mut scene_source_buffers = SceneSourceBuffers {
            points_buffer: device.create_buffer(BufferUploadMode::Dynamic),
            points_capacity: 0,
            point_indices_buffer: device.create_buffer(BufferUploadMode::Dynamic),
            point_indices_count: 0,
            point_indices_capacity: 0,
        };
        scene_source_buffers.upload(device, segments);
        scene_source_buffers
    }

    fn upload(&mut self, device: &D, segments: &Segments) {
        let needed_points_capacity = (segments.points.len() as u32).next_power_of_two();
        let needed_point_indices_capacity = (segments.indices.len() as u32).next_power_of_two();
        if self.points_capacity < needed_points_capacity {
            device.allocate_buffer::<Vector2F>(
                &self.points_buffer,
                BufferData::Uninitialized(needed_points_capacity as usize),
                BufferTarget::Storage);
            self.points_capacity = needed_points_capacity;
        }
        if self.point_indices_capacity < needed_point_indices_capacity {
            device.allocate_buffer::<SegmentIndices>(
                &self.point_indices_buffer,
                BufferData::Uninitialized(needed_point_indices_capacity as usize),
                BufferTarget::Storage);
            self.point_indices_capacity = needed_point_indices_capacity;
        }
        device.upload_to_buffer(&self.points_buffer, 0, &segments.points, BufferTarget::Storage);
        device.upload_to_buffer(&self.point_indices_buffer,
                                0,
                                &segments.indices,
                                BufferTarget::Storage);
        self.point_indices_count = segments.indices.len() as u32;
    }
}

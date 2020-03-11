// pathfinder/renderer/src/gpu/renderer.rs
//
// Copyright © 2020 The Pathfinder Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#[cfg(feature="debug_ui")]
use crate::gpu::debug::DebugUIPresenter;

use crate::gpu::options::{DestFramebuffer, RendererOptions};
use crate::gpu::shaders::{AlphaTileBlendModeProgram, AlphaTileDodgeBurnProgram};
use crate::gpu::shaders::{AlphaTileHSLProgram, AlphaTileOverlayProgram};
use crate::gpu::shaders::{AlphaTileProgram, AlphaTileVertexArray, BlitProgram, BlitVertexArray};
use crate::gpu::shaders::{CopyTileProgram, CopyTileVertexArray, FillProgram, FillVertexArray};
use crate::gpu::shaders::{MAX_FILLS_PER_BATCH, MaskTileProgram, MaskTileVertexArray};
use crate::gpu::shaders::{ReprojectionProgram, ReprojectionVertexArray, SolidTileBlurFilterProgram, SolidTileProgram, SolidTileTextFilterProgram};
use crate::gpu::shaders::{SolidTileVertexArray, StencilProgram, StencilVertexArray};
use crate::gpu_data::{AlphaTile, FillBatchPrimitive, MaskTile, RenderCommand, SolidTile};
use crate::gpu_data::{TextureLocation, TexturePageDescriptor, TexturePageId};
use crate::options::BoundingQuad;
use crate::tiles::{TILE_HEIGHT, TILE_WIDTH};
use pathfinder_color::{self as color, ColorF, ColorU};
use pathfinder_content::effects::{BlendMode, BlurDirection, CompositeOp, DefringingKernel};
use pathfinder_content::effects::{Effects, Filter};
use pathfinder_content::fill::FillRule;
use pathfinder_content::render_target::RenderTargetId;
use pathfinder_geometry::rect::RectI;
use pathfinder_geometry::transform3d::Transform4F;
use pathfinder_geometry::vector::{Vector2F, Vector2I, Vector4F};
use pathfinder_gpu::{BlendFactor, BlendOp, BlendState, BufferData, BufferTarget, BufferUploadMode};
use pathfinder_gpu::{ClearOps, DepthFunc, DepthState, Device, Primitive, RenderOptions};
use pathfinder_gpu::{RenderState, RenderTarget, StencilFunc, StencilState, TextureDataRef};
use pathfinder_gpu::{TextureFormat, TextureSamplingFlags, UniformData};
use pathfinder_resources::ResourceLoader;
use pathfinder_simd::default::{F32x2, F32x4};
use std::cmp;
use std::collections::VecDeque;
use std::f32;
use std::mem;
use std::ops::{Add, Div};
use std::time::Duration;
use std::u32;

static QUAD_VERTEX_POSITIONS: [u16; 8] = [0, 0, 1, 0, 1, 1, 0, 1];
static QUAD_VERTEX_INDICES: [u32; 6] = [0, 1, 3, 1, 2, 3];

pub(crate) const MASK_TILES_ACROSS: u32 = 256;
pub(crate) const MASK_TILES_DOWN: u32 = 256;

// 1.0 / sqrt(2*pi)
const SQRT_2_PI_INV: f32 = 0.3989422804014327;

const TEXTURE_CACHE_SIZE: usize = 8;

// FIXME(pcwalton): Shrink this again!
const MASK_FRAMEBUFFER_WIDTH:  i32 = TILE_WIDTH as i32  * MASK_TILES_ACROSS as i32;
const MASK_FRAMEBUFFER_HEIGHT: i32 = TILE_HEIGHT as i32 * MASK_TILES_DOWN as i32;

const BLEND_TERM_DEST: i32 = 0;
const BLEND_TERM_SRC:  i32 = 1;

const OVERLAY_BLEND_MODE_MULTIPLY:   i32 = 0;
const OVERLAY_BLEND_MODE_SCREEN:     i32 = 1;
const OVERLAY_BLEND_MODE_HARD_LIGHT: i32 = 2;
const OVERLAY_BLEND_MODE_OVERLAY:    i32 = 3;

pub struct Renderer<D>
where
    D: Device,
{
    // Device
    pub device: D,

    // Core data
    dest_framebuffer: DestFramebuffer<D>,
    options: RendererOptions,
    blit_program: BlitProgram<D>,
    fill_program: FillProgram<D>,
    mask_winding_tile_program: MaskTileProgram<D>,
    mask_evenodd_tile_program: MaskTileProgram<D>,
    copy_tile_program: CopyTileProgram<D>,
    alpha_tile_program: AlphaTileProgram<D>,
    alpha_tile_overlay_program: AlphaTileOverlayProgram<D>,
    alpha_tile_dodgeburn_program: AlphaTileDodgeBurnProgram<D>,
    alpha_tile_softlight_program: AlphaTileBlendModeProgram<D>,
    alpha_tile_difference_program: AlphaTileBlendModeProgram<D>,
    alpha_tile_exclusion_program: AlphaTileBlendModeProgram<D>,
    alpha_tile_hsl_program: AlphaTileHSLProgram<D>,
    blit_vertex_array: BlitVertexArray<D>,
    mask_winding_tile_vertex_array: MaskTileVertexArray<D>,
    mask_evenodd_tile_vertex_array: MaskTileVertexArray<D>,
    copy_tile_vertex_array: CopyTileVertexArray<D>,
    alpha_tile_vertex_array: AlphaTileVertexArray<D>,
    alpha_tile_overlay_vertex_array: AlphaTileVertexArray<D>,
    alpha_tile_dodgeburn_vertex_array: AlphaTileVertexArray<D>,
    alpha_tile_softlight_vertex_array: AlphaTileVertexArray<D>,
    alpha_tile_difference_vertex_array: AlphaTileVertexArray<D>,
    alpha_tile_exclusion_vertex_array: AlphaTileVertexArray<D>,
    alpha_tile_hsl_vertex_array: AlphaTileVertexArray<D>,
    area_lut_texture: D::Texture,
    alpha_tile_vertex_buffer: D::Buffer,
    quad_vertex_positions_buffer: D::Buffer,
    quad_vertex_indices_buffer: D::Buffer,
    quads_vertex_indices_buffer: D::Buffer,
    quads_vertex_indices_length: usize,
    fill_vertex_array: FillVertexArray<D>,
    fill_framebuffer: D::Framebuffer,
    mask_framebuffer: D::Framebuffer,
    dest_blend_framebuffer: D::Framebuffer,
    intermediate_dest_framebuffer: D::Framebuffer,
    texture_pages: Vec<TexturePage<D>>,
    render_targets: Vec<RenderTargetInfo>,
    render_target_stack: Vec<RenderTargetId>,

    // This is a dummy texture consisting solely of a single `rgba(0, 0, 0, 255)` texel. It serves
    // as the paint texture when drawing alpha tiles with the Clear blend mode. If this weren't
    // used, then the transparent black paint would zero out the alpha mask.
    clear_paint_texture: D::Texture,

    // Solid tiles
    solid_tile_program: SolidTileProgram<D>,
    solid_tile_blur_filter_program: SolidTileBlurFilterProgram<D>,
    solid_tile_text_filter_program: SolidTileTextFilterProgram<D>,
    solid_tile_vertex_array: SolidTileVertexArray<D>,
    solid_tile_blur_filter_vertex_array: SolidTileVertexArray<D>,
    solid_tile_text_filter_vertex_array: SolidTileVertexArray<D>,
    solid_tile_vertex_buffer: D::Buffer,
    gamma_lut_texture: D::Texture,

    // Stencil shader
    stencil_program: StencilProgram<D>,
    stencil_vertex_array: StencilVertexArray<D>,

    // Reprojection shader
    reprojection_program: ReprojectionProgram<D>,
    reprojection_vertex_array: ReprojectionVertexArray<D>,

    // Rendering state
    framebuffer_flags: FramebufferFlags,
    buffered_fills: Vec<FillBatchPrimitive>,
    texture_cache: TextureCache<D>,

    // Debug
    pub stats: RenderStats,
    current_timers: RenderTimers<D>,
    pending_timers: VecDeque<RenderTimers<D>>,
    free_timer_queries: Vec<D::TimerQuery>,

    #[cfg(feature="debug_ui")]
    pub debug_ui_presenter: DebugUIPresenter<D>,

    // Extra info
    flags: RendererFlags,
}

impl<D> Renderer<D>
where
    D: Device,
{
    pub fn new(device: D,
               resources: &dyn ResourceLoader,
               dest_framebuffer: DestFramebuffer<D>,
               options: RendererOptions)
               -> Renderer<D> {
        let blit_program = BlitProgram::new(&device, resources);
        let fill_program = FillProgram::new(&device, resources);
        let mask_winding_tile_program = MaskTileProgram::new(FillRule::Winding,
                                                             &device,
                                                             resources);
        let mask_evenodd_tile_program = MaskTileProgram::new(FillRule::EvenOdd,
                                                             &device,
                                                             resources);
        let copy_tile_program = CopyTileProgram::new(&device, resources);
        let solid_tile_program = SolidTileProgram::new(&device, resources, "tile_solid");
        let alpha_tile_program = AlphaTileProgram::new(&device, resources);
        let alpha_tile_overlay_program = AlphaTileOverlayProgram::new(&device, resources);
        let alpha_tile_dodgeburn_program = AlphaTileDodgeBurnProgram::new(&device, resources);
        let alpha_tile_softlight_program = AlphaTileBlendModeProgram::new(&device,
                                                                          resources,
                                                                          "tile_alpha_softlight");
        let alpha_tile_difference_program =
            AlphaTileBlendModeProgram::new(&device, resources, "tile_alpha_difference");
        let alpha_tile_exclusion_program = AlphaTileBlendModeProgram::new(&device,
                                                                          resources,
                                                                          "tile_alpha_exclusion");
        let alpha_tile_hsl_program = AlphaTileHSLProgram::new(&device, resources);
        let solid_tile_blur_filter_program = SolidTileBlurFilterProgram::new(&device, resources);
        let solid_tile_text_filter_program = SolidTileTextFilterProgram::new(&device, resources);
        let stencil_program = StencilProgram::new(&device, resources);
        let reprojection_program = ReprojectionProgram::new(&device, resources);

        let area_lut_texture = device.create_texture_from_png(resources, "lut/area");
        let gamma_lut_texture = device.create_texture_from_png(resources, "lut/gamma");

        let alpha_tile_vertex_buffer = device.create_buffer();
        let solid_tile_vertex_buffer = device.create_buffer();
        let quad_vertex_positions_buffer = device.create_buffer();
        device.allocate_buffer(
            &quad_vertex_positions_buffer,
            BufferData::Memory(&QUAD_VERTEX_POSITIONS),
            BufferTarget::Vertex,
            BufferUploadMode::Static,
        );
        let quad_vertex_indices_buffer = device.create_buffer();
        device.allocate_buffer(
            &quad_vertex_indices_buffer,
            BufferData::Memory(&QUAD_VERTEX_INDICES),
            BufferTarget::Index,
            BufferUploadMode::Static,
        );
        let quads_vertex_indices_buffer = device.create_buffer();

        let blit_vertex_array = BlitVertexArray::new(
            &device,
            &blit_program,
            &quad_vertex_positions_buffer,
            &quad_vertex_indices_buffer,
        );
        let fill_vertex_array = FillVertexArray::new(
            &device,
            &fill_program,
            &quad_vertex_positions_buffer,
            &quad_vertex_indices_buffer,
        );
        let mask_winding_tile_vertex_array = MaskTileVertexArray::new(
            &device,
            &mask_winding_tile_program,
            &quads_vertex_indices_buffer,
        );
        let mask_evenodd_tile_vertex_array = MaskTileVertexArray::new(
            &device,
            &mask_evenodd_tile_program,
            &quads_vertex_indices_buffer,
        );
        let copy_tile_vertex_array = CopyTileVertexArray::new(
            &device,
            &copy_tile_program,
            &alpha_tile_vertex_buffer,
            &quads_vertex_indices_buffer,
        );
        let alpha_tile_vertex_array = AlphaTileVertexArray::new(
            &device,
            &alpha_tile_program,
            &alpha_tile_vertex_buffer,
            &quads_vertex_indices_buffer,
        );
        let alpha_tile_overlay_vertex_array = AlphaTileVertexArray::new(
            &device,
            &alpha_tile_overlay_program.alpha_tile_blend_mode_program.alpha_tile_program,
            &alpha_tile_vertex_buffer,
            &quads_vertex_indices_buffer,
        );
        let alpha_tile_dodgeburn_vertex_array = AlphaTileVertexArray::new(
            &device,
            &alpha_tile_dodgeburn_program.alpha_tile_blend_mode_program.alpha_tile_program,
            &alpha_tile_vertex_buffer,
            &quads_vertex_indices_buffer,
        );
        let alpha_tile_softlight_vertex_array = AlphaTileVertexArray::new(
            &device,
            &alpha_tile_softlight_program.alpha_tile_program,
            &alpha_tile_vertex_buffer,
            &quads_vertex_indices_buffer,
        );
        let alpha_tile_difference_vertex_array = AlphaTileVertexArray::new(
            &device,
            &alpha_tile_difference_program.alpha_tile_program,
            &alpha_tile_vertex_buffer,
            &quads_vertex_indices_buffer,
        );
        let alpha_tile_exclusion_vertex_array = AlphaTileVertexArray::new(
            &device,
            &alpha_tile_exclusion_program.alpha_tile_program,
            &alpha_tile_vertex_buffer,
            &quads_vertex_indices_buffer,
        );
        let alpha_tile_hsl_vertex_array = AlphaTileVertexArray::new(
            &device,
            &alpha_tile_hsl_program.alpha_tile_blend_mode_program.alpha_tile_program,
            &alpha_tile_vertex_buffer,
            &quads_vertex_indices_buffer,
        );
        let solid_tile_vertex_array = SolidTileVertexArray::new(
            &device,
            &solid_tile_program,
            &solid_tile_vertex_buffer,
            &quads_vertex_indices_buffer,
        );
        let solid_tile_blur_filter_vertex_array = SolidTileVertexArray::new(
            &device,
            &solid_tile_blur_filter_program.solid_tile_program,
            &solid_tile_vertex_buffer,
            &quads_vertex_indices_buffer,
        );
        let solid_tile_text_filter_vertex_array = SolidTileVertexArray::new(
            &device,
            &solid_tile_text_filter_program.solid_tile_program,
            &solid_tile_vertex_buffer,
            &quads_vertex_indices_buffer,
        );
        let stencil_vertex_array = StencilVertexArray::new(&device, &stencil_program);
        let reprojection_vertex_array = ReprojectionVertexArray::new(
            &device,
            &reprojection_program,
            &quad_vertex_positions_buffer,
            &quad_vertex_indices_buffer,
        );

        let fill_framebuffer_size =
            Vector2I::new(MASK_FRAMEBUFFER_WIDTH, MASK_FRAMEBUFFER_HEIGHT);
        let fill_framebuffer_texture =
            device.create_texture(TextureFormat::R16F, fill_framebuffer_size);
        let fill_framebuffer = device.create_framebuffer(fill_framebuffer_texture);

        let mask_framebuffer_size =
            Vector2I::new(MASK_FRAMEBUFFER_WIDTH, MASK_FRAMEBUFFER_HEIGHT);
        let mask_framebuffer_texture =
            device.create_texture(TextureFormat::R8, mask_framebuffer_size);
        let mask_framebuffer = device.create_framebuffer(mask_framebuffer_texture);

        let window_size = dest_framebuffer.window_size(&device);
        let dest_blend_texture = device.create_texture(TextureFormat::RGBA8, window_size);
        let dest_blend_framebuffer = device.create_framebuffer(dest_blend_texture);
        let intermediate_dest_texture = device.create_texture(TextureFormat::RGBA8, window_size);
        let intermediate_dest_framebuffer = device.create_framebuffer(intermediate_dest_texture);

        let clear_paint_texture =
            device.create_texture_from_data(TextureFormat::RGBA8,
                                            Vector2I::splat(1),
                                            TextureDataRef::U8(&[0, 0, 0, 255]));

        #[cfg(feature="debug_ui")]
        let debug_ui_presenter = DebugUIPresenter::new(&device, resources, window_size);

        Renderer {
            device,

            dest_framebuffer,
            options,
            blit_program,
            fill_program,
            mask_winding_tile_program,
            mask_evenodd_tile_program,
            copy_tile_program,
            solid_tile_program,
            alpha_tile_program,
            alpha_tile_overlay_program,
            alpha_tile_dodgeburn_program,
            alpha_tile_softlight_program,
            alpha_tile_difference_program,
            alpha_tile_exclusion_program,
            alpha_tile_hsl_program,
            blit_vertex_array,
            mask_winding_tile_vertex_array,
            mask_evenodd_tile_vertex_array,
            copy_tile_vertex_array,
            alpha_tile_vertex_array,
            alpha_tile_overlay_vertex_array,
            alpha_tile_dodgeburn_vertex_array,
            alpha_tile_softlight_vertex_array,
            alpha_tile_difference_vertex_array,
            alpha_tile_exclusion_vertex_array,
            alpha_tile_hsl_vertex_array,
            area_lut_texture,
            alpha_tile_vertex_buffer,
            quad_vertex_positions_buffer,
            quad_vertex_indices_buffer,
            quads_vertex_indices_buffer,
            quads_vertex_indices_length: 0,
            fill_vertex_array,
            fill_framebuffer,
            mask_framebuffer,
            dest_blend_framebuffer,
            intermediate_dest_framebuffer,
            texture_pages: vec![],
            render_targets: vec![],
            render_target_stack: vec![],
            clear_paint_texture,

            solid_tile_vertex_array,
            solid_tile_blur_filter_program,
            solid_tile_blur_filter_vertex_array,
            solid_tile_text_filter_program,
            solid_tile_text_filter_vertex_array,
            solid_tile_vertex_buffer,
            gamma_lut_texture,

            stencil_program,
            stencil_vertex_array,

            reprojection_program,
            reprojection_vertex_array,

            stats: RenderStats::default(),
            current_timers: RenderTimers::new(),
            pending_timers: VecDeque::new(),
            free_timer_queries: vec![],

            #[cfg(feature="debug_ui")]
            debug_ui_presenter,

            framebuffer_flags: FramebufferFlags::empty(),
            buffered_fills: vec![],
            texture_cache: TextureCache::new(),

            flags: RendererFlags::empty(),
        }
    }

    pub fn begin_scene(&mut self) {
        self.framebuffer_flags = FramebufferFlags::empty();
        self.device.begin_commands();
        self.stats = RenderStats::default();
    }

    pub fn render_command(&mut self, command: &RenderCommand) {
        match *command {
            RenderCommand::Start { bounding_quad, path_count, needs_readable_framebuffer } => {
                self.start_rendering(bounding_quad, path_count, needs_readable_framebuffer);
            }
            RenderCommand::AllocateTexturePages(ref texture_page_descriptors) => {
                self.allocate_texture_pages(texture_page_descriptors)
            }
            RenderCommand::UploadTexelData { ref texels, location } => {
                self.upload_texel_data(texels, location)
            }
            RenderCommand::DeclareRenderTarget { id, location } => {
                self.declare_render_target(id, location)
            }
            RenderCommand::AddFills(ref fills) => self.add_fills(fills),
            RenderCommand::FlushFills => {
                self.draw_buffered_fills();
                self.begin_composite_timer_query();
            }
            RenderCommand::RenderMaskTiles { tiles: ref mask_tiles, fill_rule } => {
                let count = mask_tiles.len();
                self.upload_mask_tiles(mask_tiles, fill_rule);
                self.draw_mask_tiles(count as u32, fill_rule);
            }
            RenderCommand::PushRenderTarget(render_target_id) => {
                self.push_render_target(render_target_id)
            }
            RenderCommand::PopRenderTarget => self.pop_render_target(),
            RenderCommand::DrawSolidTiles(ref batch) => {
                let count = batch.tiles.len();
                self.stats.solid_tile_count += count;
                self.upload_solid_tiles(&batch.tiles);
                self.draw_solid_tiles(count as u32,
                                      batch.color_texture_page,
                                      batch.sampling_flags,
                                      batch.effects);
            }
            RenderCommand::DrawAlphaTiles(ref batch) => {
                let count = batch.tiles.len();
                self.stats.alpha_tile_count += count;
                self.upload_alpha_tiles(&batch.tiles);
                self.draw_alpha_tiles(count as u32,
                                      batch.color_texture_page,
                                      batch.sampling_flags,
                                      batch.blend_mode)
            }
            RenderCommand::Finish { .. } => {}
        }
    }

    pub fn end_scene(&mut self) {
        self.blit_intermediate_dest_framebuffer_if_necessary();

        self.end_composite_timer_query();
        self.pending_timers.push_back(mem::replace(&mut self.current_timers, RenderTimers::new()));

        self.device.end_commands();
    }

    fn start_rendering(&mut self,
                       bounding_quad: BoundingQuad,
                       path_count: usize,
                       mut needs_readable_framebuffer: bool) {
        if let DestFramebuffer::Other(_) = self.dest_framebuffer {
            needs_readable_framebuffer = false;
        }

        if self.flags.contains(RendererFlags::USE_DEPTH) {
            self.draw_stencil(&bounding_quad);
        }
        self.stats.path_count = path_count;

        self.flags.set(RendererFlags::INTERMEDIATE_DEST_FRAMEBUFFER_NEEDED,
                       needs_readable_framebuffer);
    }

    #[cfg(feature="debug_ui")]
    pub fn draw_debug_ui(&self) {
        self.debug_ui_presenter.draw(&self.device);
    }

    pub fn shift_rendering_time(&mut self) -> Option<RenderTime> {
        let timers = self.pending_timers.front()?;

        // Accumulate stage-0 time.
        let mut total_stage_0_time = Duration::new(0, 0);
        for timer_query in &timers.stage_0 {
            match self.device.try_recv_timer_query(timer_query) {
                None => return None,
                Some(stage_0_time) => total_stage_0_time += stage_0_time,
            }
        }

        // Get stage-1 time.
        let stage_1_time = {
            let stage_1_timer_query = timers.stage_1.as_ref().unwrap();
            match self.device.try_recv_timer_query(stage_1_timer_query) {
                None => return None,
                Some(query) => query,
            }
        };

        // Recycle all timer queries.
        let timers = self.pending_timers.pop_front().unwrap();
        self.free_timer_queries.extend(timers.stage_0.into_iter());
        self.free_timer_queries.push(timers.stage_1.unwrap());

        Some(RenderTime { stage_0: total_stage_0_time, stage_1: stage_1_time })
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
    pub fn set_options(&mut self, new_options: RendererOptions) {
        self.options = new_options
    }

    #[inline]
    pub fn set_main_framebuffer_size(&mut self, new_framebuffer_size: Vector2I) {
        #[cfg(feature="debug_ui")]
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

    fn allocate_texture_pages(&mut self, texture_page_descriptors: &[TexturePageDescriptor]) {
        // Clear out old paint textures.
        for old_texture_page in self.texture_pages.drain(..) {
            let old_texture = self.device.destroy_framebuffer(old_texture_page.framebuffer);
            self.texture_cache.release_texture(old_texture);
        }

        // Clear out old render targets.
        self.render_targets.clear();

        // Allocate textures.
        for texture_page_descriptor in texture_page_descriptors {
            let texture_size = texture_page_descriptor.size;
            let texture = self.texture_cache.create_texture(&mut self.device,
                                                            TextureFormat::RGBA8,
                                                            texture_size);
            let framebuffer = self.device.create_framebuffer(texture);
            self.texture_pages.push(TexturePage { framebuffer, must_preserve_contents: false });
        }
    }

    fn upload_texel_data(&mut self, texels: &[ColorU], location: TextureLocation) {
        let texture_page = &mut self.texture_pages[location.page.0 as usize];
        let texture = self.device.framebuffer_texture(&texture_page.framebuffer);
        let texels = color::color_slice_to_u8_slice(texels);
        self.device.upload_to_texture(texture, location.rect, TextureDataRef::U8(texels));
        texture_page.must_preserve_contents = true;
    }

    fn declare_render_target(&mut self,
                             render_target_id: RenderTargetId,
                             location: TextureLocation) {
        while self.render_targets.len() < render_target_id.0 as usize + 1 {
            self.render_targets.push(RenderTargetInfo {
                location: TextureLocation { page: TexturePageId(!0), rect: RectI::default() },
            });
        }
        let mut render_target = &mut self.render_targets[render_target_id.0 as usize];
        debug_assert_eq!(render_target.location.page, TexturePageId(!0));
        render_target.location = location;
    }

    fn upload_mask_tiles(&mut self, mask_tiles: &[MaskTile], fill_rule: FillRule) {
        let vertex_array = match fill_rule {
            FillRule::Winding => &self.mask_winding_tile_vertex_array,
            FillRule::EvenOdd => &self.mask_evenodd_tile_vertex_array,
        };

        self.device.allocate_buffer(
            &vertex_array.vertex_buffer,
            BufferData::Memory(&mask_tiles),
            BufferTarget::Vertex,
            BufferUploadMode::Dynamic,
        );
        self.ensure_index_buffer(mask_tiles.len());
    }

    fn upload_solid_tiles(&mut self, solid_tiles: &[SolidTile]) {
        self.device.allocate_buffer(
            &self.solid_tile_vertex_buffer,
            BufferData::Memory(&solid_tiles),
            BufferTarget::Vertex,
            BufferUploadMode::Dynamic,
        );
        self.ensure_index_buffer(solid_tiles.len());
    }

    fn upload_alpha_tiles(&mut self, alpha_tiles: &[AlphaTile]) {
        self.device.allocate_buffer(&self.alpha_tile_vertex_buffer,
                                    BufferData::Memory(&alpha_tiles),
                                    BufferTarget::Vertex,
                                    BufferUploadMode::Dynamic);
        self.ensure_index_buffer(alpha_tiles.len());
    }

    fn ensure_index_buffer(&mut self, mut length: usize) {
        length = length.next_power_of_two();
        if self.quads_vertex_indices_length >= length {
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

        self.device.allocate_buffer(
            &self.quads_vertex_indices_buffer,
            BufferData::Memory(&indices),
            BufferTarget::Index,
            BufferUploadMode::Static,
        );

        self.quads_vertex_indices_length = length;
    }

    fn add_fills(&mut self, mut fills: &[FillBatchPrimitive]) {
        if fills.is_empty() {
            return;
        }

        self.stats.fill_count += fills.len();

        while !fills.is_empty() {
            let count = cmp::min(fills.len(), MAX_FILLS_PER_BATCH - self.buffered_fills.len());
            self.buffered_fills.extend_from_slice(&fills[0..count]);
            fills = &fills[count..];
            if self.buffered_fills.len() == MAX_FILLS_PER_BATCH {
                self.draw_buffered_fills();
            }
        }
    }

    fn draw_buffered_fills(&mut self) {
        if self.buffered_fills.is_empty() {
            return;
        }

        self.device.allocate_buffer(
            &self.fill_vertex_array.vertex_buffer,
            BufferData::Memory(&self.buffered_fills),
            BufferTarget::Vertex,
            BufferUploadMode::Dynamic,
        );

        let mut clear_color = None;
        if !self.framebuffer_flags.contains(
                FramebufferFlags::MUST_PRESERVE_FILL_FRAMEBUFFER_CONTENTS) {
            clear_color = Some(ColorF::default());
        };

        let timer_query = self.allocate_timer_query();
        self.device.begin_timer_query(&timer_query);

        debug_assert!(self.buffered_fills.len() <= u32::MAX as usize);
        self.device.draw_elements_instanced(6, self.buffered_fills.len() as u32, &RenderState {
            target: &RenderTarget::Framebuffer(&self.fill_framebuffer),
            program: &self.fill_program.program,
            vertex_array: &self.fill_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            textures: &[&self.area_lut_texture],
            uniforms: &[
                (&self.fill_program.framebuffer_size_uniform,
                 UniformData::Vec2(F32x2::new(MASK_FRAMEBUFFER_WIDTH as f32,
                                              MASK_FRAMEBUFFER_HEIGHT as f32))),
                (&self.fill_program.tile_size_uniform,
                 UniformData::Vec2(F32x2::new(TILE_WIDTH as f32, TILE_HEIGHT as f32))),
                (&self.fill_program.area_lut_uniform, UniformData::TextureUnit(0)),
            ],
            viewport: self.mask_viewport(),
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
        self.current_timers.stage_0.push(timer_query);

        self.framebuffer_flags.insert(FramebufferFlags::MUST_PRESERVE_FILL_FRAMEBUFFER_CONTENTS);
        self.buffered_fills.clear();
    }

    fn tile_transform(&self) -> Transform4F {
        let draw_viewport = self.draw_viewport().size().to_f32();
        let scale = Vector4F::new(2.0 / draw_viewport.x(), -2.0 / draw_viewport.y(), 1.0, 1.0);
        Transform4F::from_scale(scale).translate(Vector4F::new(-1.0, 1.0, 0.0, 1.0))
    }

    fn draw_mask_tiles(&mut self, tile_count: u32, fill_rule: FillRule) {
        let clear_color =
            if self.framebuffer_flags
                   .contains(FramebufferFlags::MUST_PRESERVE_MASK_FRAMEBUFFER_CONTENTS) {
                None
            } else {
                Some(ColorF::new(1.0, 1.0, 1.0, 1.0))
            };

        let (mask_tile_program, mask_tile_vertex_array) = match fill_rule {
            FillRule::Winding => {
                (&self.mask_winding_tile_program, &self.mask_winding_tile_vertex_array)
            }
            FillRule::EvenOdd => {
                (&self.mask_evenodd_tile_program, &self.mask_evenodd_tile_vertex_array)
            }
        };

        self.device.draw_elements(tile_count * 6, &RenderState {
            target: &RenderTarget::Framebuffer(&self.mask_framebuffer),
            program: &mask_tile_program.program,
            vertex_array: &mask_tile_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            textures: &[self.device.framebuffer_texture(&self.fill_framebuffer)],
            uniforms: &[
                (&self.mask_winding_tile_program.fill_texture_uniform,
                 UniformData::TextureUnit(0)),
            ],
            viewport: self.mask_viewport(),
            options: RenderOptions {
                blend: Some(BlendState {
                    src_rgb_factor: BlendFactor::One,
                    src_alpha_factor: BlendFactor::One,
                    dest_rgb_factor: BlendFactor::One,
                    dest_alpha_factor: BlendFactor::One,
                    op: BlendOp::Min,
                    ..BlendState::default()
                }),
                clear_ops: ClearOps { color: clear_color, ..ClearOps::default() },
                ..RenderOptions::default()
            },
        });

        self.framebuffer_flags.insert(FramebufferFlags::MUST_PRESERVE_MASK_FRAMEBUFFER_CONTENTS);
    }

    fn draw_alpha_tiles(&mut self,
                        tile_count: u32,
                        color_texture_page: TexturePageId,
                        sampling_flags: TextureSamplingFlags,
                        blend_mode: BlendMode) {
        let blend_mode_program = BlendModeProgram::from_blend_mode(blend_mode);
        if blend_mode_program.needs_readable_framebuffer() {
            self.copy_alpha_tiles_to_dest_blend_texture(tile_count);
        }

        let clear_color = self.clear_color_for_draw_operation();

        let (alpha_tile_program, alpha_tile_vertex_array) = match blend_mode_program {
            BlendModeProgram::Regular => (&self.alpha_tile_program, &self.alpha_tile_vertex_array),
            BlendModeProgram::Overlay => {
                (&self.alpha_tile_overlay_program.alpha_tile_blend_mode_program.alpha_tile_program,
                 &self.alpha_tile_overlay_vertex_array)
            }
            BlendModeProgram::DodgeBurn => {
                (&self.alpha_tile_dodgeburn_program
                      .alpha_tile_blend_mode_program
                      .alpha_tile_program,
                 &self.alpha_tile_dodgeburn_vertex_array)
            }
            BlendModeProgram::SoftLight => {
                (&self.alpha_tile_softlight_program.alpha_tile_program,
                 &self.alpha_tile_softlight_vertex_array)
            }
            BlendModeProgram::Difference => {
                (&self.alpha_tile_difference_program.alpha_tile_program,
                 &self.alpha_tile_difference_vertex_array)
            }
            BlendModeProgram::Exclusion => {
                (&self.alpha_tile_exclusion_program.alpha_tile_program,
                 &self.alpha_tile_exclusion_vertex_array)
            }
            BlendModeProgram::HSL => {
                (&self.alpha_tile_hsl_program.alpha_tile_blend_mode_program.alpha_tile_program,
                 &self.alpha_tile_hsl_vertex_array)
            }
        };

        let draw_viewport = self.draw_viewport();

        let mut textures = vec![self.device.framebuffer_texture(&self.mask_framebuffer)];
        let mut uniforms = vec![
            (&alpha_tile_program.transform_uniform,
             UniformData::Mat4(self.tile_transform().to_columns())),
            (&alpha_tile_program.tile_size_uniform,
             UniformData::Vec2(F32x2::new(TILE_WIDTH as f32, TILE_HEIGHT as f32))),
            (&alpha_tile_program.stencil_texture_uniform, UniformData::TextureUnit(0)),
            (&alpha_tile_program.framebuffer_size_uniform,
             UniformData::Vec2(draw_viewport.size().to_f32().0)),
        ];

        let paint_texture = match blend_mode {
            BlendMode::Clear => {
                // Use a special dummy paint texture containing `rgba(0, 0, 0, 255)` so that the
                // transparent black paint color doesn't zero out the mask.
                &self.clear_paint_texture
            }
            _ => self.texture_page(color_texture_page),
        };

        self.device.set_texture_sampling_mode(paint_texture, sampling_flags);

        textures.push(paint_texture);
        uniforms.push((&alpha_tile_program.paint_texture_uniform, UniformData::TextureUnit(1)));

        match blend_mode_program {
            BlendModeProgram::Regular => {}
            BlendModeProgram::Overlay => {
                self.set_uniforms_for_overlay_blend_mode(&mut textures, &mut uniforms, blend_mode);
            }
            BlendModeProgram::DodgeBurn => {
                self.set_uniforms_for_dodge_burn_blend_mode(&mut textures,
                                                            &mut uniforms,
                                                            blend_mode);
            }
            BlendModeProgram::SoftLight => {
                self.set_uniforms_for_blend_mode(&mut textures,
                                                 &mut uniforms,
                                                 &self.alpha_tile_softlight_program);
            }
            BlendModeProgram::Difference => {
                self.set_uniforms_for_blend_mode(&mut textures,
                                                 &mut uniforms,
                                                 &self.alpha_tile_difference_program);
            }
            BlendModeProgram::Exclusion => {
                self.set_uniforms_for_blend_mode(&mut textures,
                                                 &mut uniforms,
                                                 &self.alpha_tile_exclusion_program);
            }
            BlendModeProgram::HSL => {
                self.set_uniforms_for_hsl_blend_mode(&mut textures, &mut uniforms, blend_mode);
            }
        }

        self.device.draw_elements(tile_count * 6, &RenderState {
            target: &self.draw_render_target(),
            program: &alpha_tile_program.program,
            vertex_array: &alpha_tile_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            textures: &textures,
            uniforms: &uniforms,
            viewport: draw_viewport,
            options: RenderOptions {
                blend: blend_mode.to_blend_state(),
                stencil: self.stencil_state(),
                clear_ops: ClearOps { color: clear_color, ..ClearOps::default() },
                ..RenderOptions::default()
            },
        });

        self.preserve_draw_framebuffer();
    }

    fn set_uniforms_for_blend_mode<'a>(
            &'a self,
            textures: &mut Vec<&'a D::Texture>,
            uniforms: &mut Vec<(&'a D::Uniform, UniformData)>,
            alpha_tile_blend_mode_program: &'a AlphaTileBlendModeProgram<D>) {
        textures.push(self.device.framebuffer_texture(&self.dest_blend_framebuffer));
        uniforms.push((&alpha_tile_blend_mode_program.dest_uniform,
                       UniformData::TextureUnit(textures.len() as u32 - 1)));
    }

    fn set_uniforms_for_overlay_blend_mode<'a>(&'a self,
                                               textures: &mut Vec<&'a D::Texture>,
                                               uniforms: &mut Vec<(&'a D::Uniform, UniformData)>,
                                               blend_mode: BlendMode) {
        let overlay_blend_mode = match blend_mode {
            BlendMode::Multiply  => OVERLAY_BLEND_MODE_MULTIPLY,
            BlendMode::Screen    => OVERLAY_BLEND_MODE_SCREEN,
            BlendMode::HardLight => OVERLAY_BLEND_MODE_HARD_LIGHT,
            BlendMode::Overlay   => OVERLAY_BLEND_MODE_OVERLAY,
            _                    => unreachable!(),
        };

        uniforms.push((&self.alpha_tile_overlay_program.blend_mode_uniform,
                       UniformData::Int(overlay_blend_mode)));

        self.set_uniforms_for_blend_mode(textures,
                                         uniforms,
                                         &self.alpha_tile_overlay_program
                                              .alpha_tile_blend_mode_program);
    }

    fn set_uniforms_for_dodge_burn_blend_mode<'a>(
            &'a self,
            textures: &mut Vec<&'a D::Texture>,
            uniforms: &mut Vec<(&'a D::Uniform, UniformData)>,
            blend_mode: BlendMode) {
        uniforms.push((&self.alpha_tile_dodgeburn_program.burn_uniform,
                       UniformData::Int(if blend_mode == BlendMode::ColorBurn { 1 } else { 0 })));

        self.set_uniforms_for_blend_mode(textures,
                                         uniforms,
                                         &self.alpha_tile_dodgeburn_program
                                              .alpha_tile_blend_mode_program);
    }

    fn set_uniforms_for_hsl_blend_mode<'a>(&'a self,
                                           textures: &mut Vec<&'a D::Texture>,
                                           uniforms: &mut Vec<(&'a D::Uniform, UniformData)>,
                                           blend_mode: BlendMode) {
        let hsl_terms = match blend_mode {
            BlendMode::Hue        => [BLEND_TERM_SRC,  BLEND_TERM_DEST, BLEND_TERM_DEST],
            BlendMode::Saturation => [BLEND_TERM_DEST, BLEND_TERM_SRC,  BLEND_TERM_DEST],
            BlendMode::Luminosity => [BLEND_TERM_DEST, BLEND_TERM_DEST, BLEND_TERM_SRC ],
            BlendMode::Color      => [BLEND_TERM_SRC,  BLEND_TERM_SRC,  BLEND_TERM_DEST],
            _                     => unreachable!(),
        };

        uniforms.push((&self.alpha_tile_hsl_program.blend_hsl_uniform,
                       UniformData::IVec3(hsl_terms)));

        self.set_uniforms_for_blend_mode(textures,
                                         uniforms,
                                         &self.alpha_tile_hsl_program
                                              .alpha_tile_blend_mode_program);
    }

    fn copy_alpha_tiles_to_dest_blend_texture(&mut self, tile_count: u32) {
        let draw_viewport = self.draw_viewport();

        let mut textures = vec![];
        let mut uniforms = vec![
            (&self.copy_tile_program.transform_uniform,
             UniformData::Mat4(self.tile_transform().to_columns())),
            (&self.copy_tile_program.tile_size_uniform,
             UniformData::Vec2(F32x2::new(TILE_WIDTH as f32, TILE_HEIGHT as f32))),
            (&self.copy_tile_program.framebuffer_size_uniform,
             UniformData::Vec2(draw_viewport.size().to_f32().0)),
        ];

        let draw_framebuffer = match self.draw_render_target() {
            RenderTarget::Framebuffer(framebuffer) => framebuffer,
            RenderTarget::Default => panic!("Can't copy alpha tiles from default framebuffer!"),
        };
        let draw_texture = self.device.framebuffer_texture(&draw_framebuffer);

        textures.push(draw_texture);
        uniforms.push((&self.copy_tile_program.src_uniform, UniformData::TextureUnit(0)));

        self.device.draw_elements(tile_count * 6, &RenderState {
            target: &RenderTarget::Framebuffer(&self.dest_blend_framebuffer),
            program: &self.copy_tile_program.program,
            vertex_array: &self.copy_tile_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            textures: &textures,
            uniforms: &uniforms,
            viewport: draw_viewport,
            options: RenderOptions {
                clear_ops: ClearOps {
                    color: Some(ColorF::transparent_black()),
                    ..ClearOps::default()
                },
                ..RenderOptions::default()
            },
        });
    }

    fn draw_solid_tiles(&mut self,
                        tile_count: u32,
                        color_texture_page: TexturePageId,
                        sampling_flags: TextureSamplingFlags,
                        effects: Effects) {
        let clear_color = self.clear_color_for_draw_operation();

        let (solid_tile_program, solid_tile_vertex_array) = match effects.filter {
            Filter::Composite(_) => {
                (&self.solid_tile_program, &self.solid_tile_vertex_array)
            }
            Filter::Text { .. } => {
                (&self.solid_tile_text_filter_program.solid_tile_program,
                 &self.solid_tile_text_filter_vertex_array)
            }
            Filter::Blur { .. } => {
                (&self.solid_tile_blur_filter_program.solid_tile_program,
                 &self.solid_tile_blur_filter_vertex_array)
            }
        };

        let mut textures = vec![];
        let mut uniforms = vec![
            (&solid_tile_program.transform_uniform,
             UniformData::Mat4(self.tile_transform().to_columns())),
            (&solid_tile_program.tile_size_uniform,
             UniformData::Vec2(F32x2::new(TILE_WIDTH as f32, TILE_HEIGHT as f32))),
        ];

        let texture_page = self.texture_page(color_texture_page);
        let texture_size = self.device.texture_size(texture_page);
        self.device.set_texture_sampling_mode(texture_page, sampling_flags);
        textures.push(texture_page);
        uniforms.push((&solid_tile_program.color_texture_uniform, UniformData::TextureUnit(0)));

        let blend_state = match effects.filter {
            Filter::Composite(composite_op) => composite_op.to_blend_state(),
            Filter::Blur { .. } | Filter::Text { .. } => CompositeOp::SrcOver.to_blend_state(),
        };

        match effects.filter {
            Filter::Composite(_) => {}
            Filter::Text { fg_color, bg_color, defringing_kernel, gamma_correction } => {
                self.set_uniforms_for_text_filter(&mut textures,
                                                  &mut uniforms,
                                                  fg_color,
                                                  bg_color,
                                                  defringing_kernel,
                                                  gamma_correction);
            }
            Filter::Blur { direction, sigma } => {
                self.set_uniforms_for_blur_filter(&mut uniforms, texture_size, direction, sigma);
            }
        }

        self.device.draw_elements(6 * tile_count, &RenderState {
            target: &self.draw_render_target(),
            program: &solid_tile_program.program,
            vertex_array: &solid_tile_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            textures: &textures,
            uniforms: &uniforms,
            viewport: self.draw_viewport(),
            options: RenderOptions {
                blend: blend_state,
                stencil: self.stencil_state(),
                clear_ops: ClearOps { color: clear_color, ..ClearOps::default() },
                ..RenderOptions::default()
            },
        });

        self.preserve_draw_framebuffer();
    }

    fn draw_stencil(&mut self, quad_positions: &[Vector4F]) {
        self.device.allocate_buffer(
            &self.stencil_vertex_array.vertex_buffer,
            BufferData::Memory(quad_positions),
            BufferTarget::Vertex,
            BufferUploadMode::Dynamic,
        );

        // Create indices for a triangle fan. (This is OK because the clipped quad should always be
        // convex.)
        let mut indices: Vec<u32> = vec![];
        for index in 1..(quad_positions.len() as u32 - 1) {
            indices.extend_from_slice(&[0, index as u32, index + 1]);
        }
        self.device.allocate_buffer(
            &self.stencil_vertex_array.index_buffer,
            BufferData::Memory(&indices),
            BufferTarget::Index,
            BufferUploadMode::Dynamic,
        );

        self.device.draw_elements(indices.len() as u32, &RenderState {
            target: &self.draw_render_target(),
            program: &self.stencil_program.program,
            vertex_array: &self.stencil_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            textures: &[],
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
    }

    pub fn reproject_texture(
        &mut self,
        texture: &D::Texture,
        old_transform: &Transform4F,
        new_transform: &Transform4F,
    ) {
        let clear_color = self.clear_color_for_draw_operation();

        self.device.draw_elements(6, &RenderState {
            target: &self.draw_render_target(),
            program: &self.reprojection_program.program,
            vertex_array: &self.reprojection_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            textures: &[texture],
            uniforms: &[
                (&self.reprojection_program.old_transform_uniform,
                 UniformData::from_transform_3d(old_transform)),
                (&self.reprojection_program.new_transform_uniform,
                 UniformData::from_transform_3d(new_transform)),
                (&self.reprojection_program.texture_uniform, UniformData::TextureUnit(0)),
            ],
            viewport: self.draw_viewport(),
            options: RenderOptions {
                blend: BlendMode::SrcOver.to_blend_state(),
                depth: Some(DepthState { func: DepthFunc::Less, write: false, }),
                clear_ops: ClearOps { color: clear_color, ..ClearOps::default() },
                ..RenderOptions::default()
            },
        });

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
                    RenderTarget::Framebuffer(&self.intermediate_dest_framebuffer)
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

    fn set_uniforms_for_text_filter<'a>(&'a self,
                                        textures: &mut Vec<&'a D::Texture>,
                                        uniforms: &mut Vec<(&'a D::Uniform, UniformData)>,
                                        fg_color: ColorF,
                                        bg_color: ColorF,
                                        defringing_kernel: Option<DefringingKernel>,
                                        gamma_correction: bool) {
        let gamma_lut_texture_unit = textures.len() as u32;
        textures.push(&self.gamma_lut_texture);

        uniforms.extend_from_slice(&[
            (&self.solid_tile_text_filter_program.gamma_lut_uniform,
             UniformData::TextureUnit(gamma_lut_texture_unit)),
            (&self.solid_tile_text_filter_program.fg_color_uniform, UniformData::Vec4(fg_color.0)),
            (&self.solid_tile_text_filter_program.bg_color_uniform, UniformData::Vec4(bg_color.0)),
            (&self.solid_tile_text_filter_program.gamma_correction_enabled_uniform,
             UniformData::Int(gamma_correction as i32)),
        ]);

        match defringing_kernel {
            Some(ref kernel) => {
                uniforms.push((&self.solid_tile_text_filter_program.kernel_uniform,
                               UniformData::Vec4(F32x4::from_slice(&kernel.0))));
            }
            None => {
                uniforms.push((&self.solid_tile_text_filter_program.kernel_uniform,
                               UniformData::Vec4(F32x4::default())));
            }
        }
    }

    fn set_uniforms_for_blur_filter<'a>(&'a self,
                                        uniforms: &mut Vec<(&'a D::Uniform, UniformData)>,
                                        src_texture_size: Vector2I,
                                        direction: BlurDirection,
                                        sigma: f32) {
        let sigma_inv = 1.0 / sigma;
        let gauss_coeff_x = SQRT_2_PI_INV * sigma_inv;
        let gauss_coeff_y = f32::exp(-0.5 * sigma_inv * sigma_inv);
        let gauss_coeff_z = gauss_coeff_y * gauss_coeff_y;

        let src_offset = match direction {
            BlurDirection::X => Vector2F::new(1.0, 0.0),
            BlurDirection::Y => Vector2F::new(0.0, 1.0),
        };
        let src_offset_scale = src_offset / src_texture_size.to_f32();

        uniforms.extend_from_slice(&[
            (&self.solid_tile_blur_filter_program.src_offset_scale_uniform,
             UniformData::Vec2(src_offset_scale.0)),
            (&self.solid_tile_blur_filter_program.initial_gauss_coeff_uniform,
             UniformData::Vec3([gauss_coeff_x, gauss_coeff_y, gauss_coeff_z])),
            (&self.solid_tile_blur_filter_program.support_uniform,
             UniformData::Int(f32::ceil(1.5 * sigma) as i32 * 2)),
        ]);
    }

    fn blit_intermediate_dest_framebuffer_if_necessary(&mut self) {
        if !self.flags.contains(RendererFlags::INTERMEDIATE_DEST_FRAMEBUFFER_NEEDED) {
            return;
        }

        let main_viewport = self.main_viewport();

        let uniforms = [(&self.blit_program.src_uniform, UniformData::TextureUnit(0))];
        let textures = [(self.device.framebuffer_texture(&self.intermediate_dest_framebuffer))];

        self.device.draw_elements(6, &RenderState {
            target: &RenderTarget::Default,
            program: &self.blit_program.program,
            vertex_array: &self.blit_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            textures: &textures[..],
            uniforms: &uniforms[..],
            viewport: main_viewport,
            options: RenderOptions::default(),
        });
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
                self.texture_pages[texture_page.0 as usize].must_preserve_contents
            }
            None => {
                self.framebuffer_flags
                    .contains(FramebufferFlags::MUST_PRESERVE_DEST_FRAMEBUFFER_CONTENTS)
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
                self.texture_pages[texture_page.0 as usize].must_preserve_contents = true;
            }
            None => {
                self.framebuffer_flags
                    .insert(FramebufferFlags::MUST_PRESERVE_DEST_FRAMEBUFFER_CONTENTS);
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
        RectI::new(Vector2I::default(),
                   Vector2I::new(MASK_FRAMEBUFFER_WIDTH, MASK_FRAMEBUFFER_HEIGHT))
    }

    fn render_target_location(&self, render_target_id: RenderTargetId) -> TextureLocation {
        self.render_targets[render_target_id.0 as usize].location
    }

    fn texture_page_framebuffer(&self, id: TexturePageId) -> &D::Framebuffer {
        &self.texture_pages[id.0 as usize].framebuffer
    }

    fn texture_page(&self, id: TexturePageId) -> &D::Texture {
        self.device.framebuffer_texture(&self.texture_page_framebuffer(id))
    }

    fn allocate_timer_query(&mut self) -> D::TimerQuery {
        match self.free_timer_queries.pop() {
            Some(query) => query,
            None => self.device.create_timer_query(),
        }
    }

    fn begin_composite_timer_query(&mut self) {
        let timer_query = self.allocate_timer_query();
        self.device.begin_timer_query(&timer_query);
        self.current_timers.stage_1 = Some(timer_query);
    }

    fn end_composite_timer_query(&mut self) {
        if let Some(ref query) = self.current_timers.stage_1 {
            self.device.end_timer_query(query);
        }
    }
}

// Render stats

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub path_count: usize,
    pub fill_count: usize,
    pub alpha_tile_count: usize,
    pub solid_tile_count: usize,
}

impl Add<RenderStats> for RenderStats {
    type Output = RenderStats;
    fn add(self, other: RenderStats) -> RenderStats {
        RenderStats {
            path_count: self.path_count + other.path_count,
            solid_tile_count: self.solid_tile_count + other.solid_tile_count,
            alpha_tile_count: self.alpha_tile_count + other.alpha_tile_count,
            fill_count: self.fill_count + other.fill_count,
        }
    }
}

impl Div<usize> for RenderStats {
    type Output = RenderStats;
    fn div(self, divisor: usize) -> RenderStats {
        RenderStats {
            path_count: self.path_count / divisor,
            solid_tile_count: self.solid_tile_count / divisor,
            alpha_tile_count: self.alpha_tile_count / divisor,
            fill_count: self.fill_count / divisor,
        }
    }
}

struct RenderTimers<D> where D: Device {
    stage_0: Vec<D::TimerQuery>,
    stage_1: Option<D::TimerQuery>,
}

impl<D> RenderTimers<D> where D: Device {
    fn new() -> RenderTimers<D> {
        RenderTimers { stage_0: vec![], stage_1: None }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RenderTime {
    pub stage_0: Duration,
    pub stage_1: Duration,
}

impl Default for RenderTime {
    #[inline]
    fn default() -> RenderTime {
        RenderTime { stage_0: Duration::new(0, 0), stage_1: Duration::new(0, 0) }
    }
}

impl Add<RenderTime> for RenderTime {
    type Output = RenderTime;

    #[inline]
    fn add(self, other: RenderTime) -> RenderTime {
        RenderTime {
            stage_0: self.stage_0 + other.stage_0,
            stage_1: self.stage_1 + other.stage_1,
        }
    }
}

bitflags! {
    struct FramebufferFlags: u8 {
        const MUST_PRESERVE_FILL_FRAMEBUFFER_CONTENTS = 0x01;
        const MUST_PRESERVE_MASK_FRAMEBUFFER_CONTENTS = 0x02;
        const MUST_PRESERVE_DEST_FRAMEBUFFER_CONTENTS = 0x04;
    }
}

struct TextureCache<D> where D: Device {
    textures: Vec<D::Texture>,
}

impl<D> TextureCache<D> where D: Device {
    fn new() -> TextureCache<D> {
        TextureCache { textures: vec![] }
    }

    fn create_texture(&mut self, device: &mut D, format: TextureFormat, size: Vector2I)
                      -> D::Texture {
        for index in 0..self.textures.len() {
            if device.texture_size(&self.textures[index]) != size ||
                    device.texture_format(&self.textures[index]) != format {
                continue;
            }
            return self.textures.remove(index);
        }

        device.create_texture(format, size)
    }

    fn release_texture(&mut self, texture: D::Texture) {
        if self.textures.len() == TEXTURE_CACHE_SIZE {
            self.textures.pop();
        }
        self.textures.insert(0, texture);
    }
}

struct TexturePage<D> where D: Device {
    framebuffer: D::Framebuffer,
    must_preserve_contents: bool,
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
                    dest_rgb_factor: BlendFactor::OneMinusSrcAlpha,
                    src_alpha_factor: BlendFactor::Zero,
                    dest_alpha_factor: BlendFactor::OneMinusSrcAlpha,
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
                    dest_rgb_factor: BlendFactor::DestAlpha,
                    src_alpha_factor: BlendFactor::OneMinusDestAlpha,
                    dest_alpha_factor: BlendFactor::One,
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
            BlendMode::Lighten => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::One,
                    dest_rgb_factor: BlendFactor::OneMinusSrcAlpha,
                    src_alpha_factor: BlendFactor::One,
                    dest_alpha_factor: BlendFactor::One,
                    op: BlendOp::Max,
                })
            }
            BlendMode::Darken => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::One,
                    dest_rgb_factor: BlendFactor::OneMinusSrcAlpha,
                    src_alpha_factor: BlendFactor::One,
                    dest_alpha_factor: BlendFactor::One,
                    op: BlendOp::Min,
                })
            }
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

impl ToBlendState for CompositeOp {
    fn to_blend_state(self) -> Option<BlendState> {
        match self {
            CompositeOp::Clear => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::Zero,
                    dest_rgb_factor: BlendFactor::OneMinusSrcAlpha,
                    src_alpha_factor: BlendFactor::Zero,
                    dest_alpha_factor: BlendFactor::OneMinusSrcAlpha,
                    ..BlendState::default()
                })
            }
            CompositeOp::Copy => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::One,
                    dest_rgb_factor: BlendFactor::Zero,
                    src_alpha_factor: BlendFactor::One,
                    dest_alpha_factor: BlendFactor::Zero,
                    ..BlendState::default()
                })
            }
            CompositeOp::SrcOver => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::One,
                    dest_rgb_factor: BlendFactor::OneMinusSrcAlpha,
                    src_alpha_factor: BlendFactor::One,
                    dest_alpha_factor: BlendFactor::OneMinusSrcAlpha,
                    ..BlendState::default()
                })
            }
            CompositeOp::SrcIn => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::DestAlpha,
                    dest_rgb_factor: BlendFactor::Zero,
                    src_alpha_factor: BlendFactor::DestAlpha,
                    dest_alpha_factor: BlendFactor::Zero,
                    ..BlendState::default()
                })
            }
            CompositeOp::DestIn => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::Zero,
                    dest_rgb_factor: BlendFactor::SrcAlpha,
                    src_alpha_factor: BlendFactor::Zero,
                    dest_alpha_factor: BlendFactor::SrcAlpha,
                    ..BlendState::default()
                })
            }
            CompositeOp::SrcOut => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::OneMinusDestAlpha,
                    dest_rgb_factor: BlendFactor::Zero,
                    src_alpha_factor: BlendFactor::OneMinusDestAlpha,
                    dest_alpha_factor: BlendFactor::Zero,
                    ..BlendState::default()
                })
            }
            CompositeOp::DestAtop => {
                Some(BlendState {
                    src_rgb_factor: BlendFactor::OneMinusDestAlpha,
                    dest_rgb_factor: BlendFactor::SrcAlpha,
                    src_alpha_factor: BlendFactor::OneMinusDestAlpha,
                    dest_alpha_factor: BlendFactor::SrcAlpha,
                    ..BlendState::default()
                })
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BlendModeProgram {
    Regular,
    Overlay,
    DodgeBurn,
    SoftLight,
    Difference,
    Exclusion,
    HSL,
}

impl BlendModeProgram {
    pub(crate) fn from_blend_mode(blend_mode: BlendMode) -> BlendModeProgram {
        match blend_mode {
            BlendMode::Clear |
            BlendMode::SrcOver |
            BlendMode::DestOver |
            BlendMode::DestOut |
            BlendMode::SrcAtop |
            BlendMode::Xor |
            BlendMode::Lighter |
            BlendMode::Lighten |
            BlendMode::Darken => BlendModeProgram::Regular,
            BlendMode::Multiply |
            BlendMode::Screen |
            BlendMode::HardLight |
            BlendMode::Overlay => BlendModeProgram::Overlay,
            BlendMode::ColorDodge |
            BlendMode::ColorBurn => BlendModeProgram::DodgeBurn,
            BlendMode::SoftLight => BlendModeProgram::SoftLight,
            BlendMode::Difference => BlendModeProgram::Difference,
            BlendMode::Exclusion => BlendModeProgram::Exclusion,
            BlendMode::Hue |
            BlendMode::Saturation |
            BlendMode::Color |
            BlendMode::Luminosity => BlendModeProgram::HSL,
        }
    }

    pub(crate) fn needs_readable_framebuffer(self) -> bool {
        match self {
            BlendModeProgram::Regular => false,
            BlendModeProgram::Overlay |
            BlendModeProgram::DodgeBurn |
            BlendModeProgram::SoftLight |
            BlendModeProgram::Difference |
            BlendModeProgram::Exclusion |
            BlendModeProgram::HSL => true,
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

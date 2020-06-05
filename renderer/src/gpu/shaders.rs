// pathfinder/renderer/src/gpu/shaders.rs
//
// Copyright © 2020 The Pathfinder Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::gpu::options::RendererLevel;
use crate::tiles::{TILE_HEIGHT, TILE_WIDTH};
use pathfinder_gpu::{BufferTarget, BufferUploadMode, ComputeDimensions, Device, VertexAttrClass};
use pathfinder_gpu::{VertexAttrDescriptor, VertexAttrType};
use pathfinder_resources::ResourceLoader;

// TODO(pcwalton): Replace with `mem::size_of` calls?
pub(crate) const TILE_INSTANCE_SIZE: usize = 16;
const FILL_INSTANCE_SIZE: usize = 12;
const CLIP_TILE_INSTANCE_SIZE: usize = 16;

pub const MAX_FILLS_PER_BATCH: usize = 0x10000;

pub const PROPAGATE_WORKGROUP_SIZE: u32 = 64;

pub struct BlitVertexArray<D> where D: Device {
    pub vertex_array: D::VertexArray,
}

impl<D> BlitVertexArray<D> where D: Device {
    pub fn new(device: &D,
               blit_program: &BlitProgram<D>,
               quad_vertex_positions_buffer: &D::Buffer,
               quad_vertex_indices_buffer: &D::Buffer)
               -> BlitVertexArray<D> {
        let vertex_array = device.create_vertex_array();
        let position_attr = device.get_vertex_attr(&blit_program.program, "Position").unwrap();

        device.bind_buffer(&vertex_array, quad_vertex_positions_buffer, BufferTarget::Vertex);
        device.configure_vertex_attr(&vertex_array, &position_attr, &VertexAttrDescriptor {
            size: 2,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I16,
            stride: 4,
            offset: 0,
            divisor: 0,
            buffer_index: 0,
        });
        device.bind_buffer(&vertex_array, quad_vertex_indices_buffer, BufferTarget::Index);

        BlitVertexArray { vertex_array }
    }
}

pub struct BlitBufferVertexArray<D> where D: Device {
    pub vertex_array: D::VertexArray,
}

impl<D> BlitBufferVertexArray<D> where D: Device {
    pub fn new(device: &D,
               blit_buffer_program: &BlitBufferProgram<D>,
               quad_vertex_positions_buffer: &D::Buffer,
               quad_vertex_indices_buffer: &D::Buffer)
               -> BlitBufferVertexArray<D> {
        let vertex_array = device.create_vertex_array();
        let position_attr = device.get_vertex_attr(&blit_buffer_program.program,
                                                   "Position").unwrap();

        device.bind_buffer(&vertex_array, quad_vertex_positions_buffer, BufferTarget::Vertex);
        device.configure_vertex_attr(&vertex_array, &position_attr, &VertexAttrDescriptor {
            size: 2,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I16,
            stride: 4,
            offset: 0,
            divisor: 0,
            buffer_index: 0,
        });
        device.bind_buffer(&vertex_array, quad_vertex_indices_buffer, BufferTarget::Index);

        BlitBufferVertexArray { vertex_array }
    }
}

pub struct ClearVertexArray<D> where D: Device {
    pub vertex_array: D::VertexArray,
}

impl<D> ClearVertexArray<D> where D: Device {
    pub fn new(device: &D,
               clear_program: &ClearProgram<D>,
               quad_vertex_positions_buffer: &D::Buffer,
               quad_vertex_indices_buffer: &D::Buffer)
               -> ClearVertexArray<D> {
        let vertex_array = device.create_vertex_array();
        let position_attr = device.get_vertex_attr(&clear_program.program, "Position").unwrap();

        device.bind_buffer(&vertex_array, quad_vertex_positions_buffer, BufferTarget::Vertex);
        device.configure_vertex_attr(&vertex_array, &position_attr, &VertexAttrDescriptor {
            size: 2,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I16,
            stride: 4,
            offset: 0,
            divisor: 0,
            buffer_index: 0,
        });
        device.bind_buffer(&vertex_array, quad_vertex_indices_buffer, BufferTarget::Index);

        ClearVertexArray { vertex_array }
    }
}

pub struct FillVertexArray<D> where D: Device {
    pub vertex_array: D::VertexArray,
}

impl<D> FillVertexArray<D>
where
    D: Device,
{
    pub fn new(
        device: &D,
        fill_program: &FillRasterProgram<D>,
        vertex_buffer: &D::Buffer,
        quad_vertex_positions_buffer: &D::Buffer,
        quad_vertex_indices_buffer: &D::Buffer,
    ) -> FillVertexArray<D> {
        let vertex_array = device.create_vertex_array();

        let tess_coord_attr = device.get_vertex_attr(&fill_program.program, "TessCoord").unwrap();
        let line_segment_attr = device.get_vertex_attr(&fill_program.program, "LineSegment")
                                      .unwrap();
        let tile_index_attr = device.get_vertex_attr(&fill_program.program, "TileIndex").unwrap();

        device.bind_buffer(&vertex_array, quad_vertex_positions_buffer, BufferTarget::Vertex);
        device.configure_vertex_attr(&vertex_array, &tess_coord_attr, &VertexAttrDescriptor {
            size: 2,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::U16,
            stride: 4,
            offset: 0,
            divisor: 0,
            buffer_index: 0,
        });
        device.bind_buffer(&vertex_array, &vertex_buffer, BufferTarget::Vertex);
        device.configure_vertex_attr(&vertex_array, &line_segment_attr, &VertexAttrDescriptor {
            size: 4,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::U16,
            stride: FILL_INSTANCE_SIZE,
            offset: 0,
            divisor: 1,
            buffer_index: 1,
        });
        device.configure_vertex_attr(&vertex_array, &tile_index_attr, &VertexAttrDescriptor {
            size: 1,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I32,
            stride: FILL_INSTANCE_SIZE,
            offset: 8,
            divisor: 1,
            buffer_index: 1,
        });
        device.bind_buffer(&vertex_array, quad_vertex_indices_buffer, BufferTarget::Index);

        FillVertexArray { vertex_array }
    }
}

pub struct TileVertexArray<D> where D: Device {
    pub vertex_array: D::VertexArray,
}

impl<D> TileVertexArray<D> where D: Device {
    pub fn new(device: &D,
               tile_program: &TileProgram<D>,
               tile_vertex_buffer: &D::Buffer,
               quad_vertex_positions_buffer: &D::Buffer,
               quad_vertex_indices_buffer: &D::Buffer)
               -> TileVertexArray<D> {
        let vertex_array = device.create_vertex_array();

        let tile_offset_attr =
            device.get_vertex_attr(&tile_program.program, "TileOffset").unwrap();
        let tile_origin_attr =
            device.get_vertex_attr(&tile_program.program, "TileOrigin").unwrap();
        let mask_0_tex_coord_attr =
            device.get_vertex_attr(&tile_program.program, "MaskTexCoord0").unwrap();
        let ctrl_backdrop_attr =
            device.get_vertex_attr(&tile_program.program, "CtrlBackdrop").unwrap();
        let color_attr = device.get_vertex_attr(&tile_program.program, "Color").unwrap();
        let path_index_attr = device.get_vertex_attr(&tile_program.program, "PathIndex").unwrap();

        device.bind_buffer(&vertex_array, quad_vertex_positions_buffer, BufferTarget::Vertex);
        device.configure_vertex_attr(&vertex_array, &tile_offset_attr, &VertexAttrDescriptor {
            size: 2,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I16,
            stride: 4,
            offset: 0,
            divisor: 0,
            buffer_index: 0,
        });
        device.bind_buffer(&vertex_array, tile_vertex_buffer, BufferTarget::Vertex);
        device.configure_vertex_attr(&vertex_array, &tile_origin_attr, &VertexAttrDescriptor {
            size: 2,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I16,
            stride: TILE_INSTANCE_SIZE,
            offset: 0,
            divisor: 1,
            buffer_index: 1,
        });
        device.configure_vertex_attr(&vertex_array, &mask_0_tex_coord_attr, &VertexAttrDescriptor {
            size: 4,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::U8,
            stride: TILE_INSTANCE_SIZE,
            offset: 4,
            divisor: 1,
            buffer_index: 1,
        });
        device.configure_vertex_attr(&vertex_array, &path_index_attr, &VertexAttrDescriptor {
            size: 1,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I32,
            stride: TILE_INSTANCE_SIZE,
            offset: 8,
            divisor: 1,
            buffer_index: 1,
        });
        device.configure_vertex_attr(&vertex_array, &color_attr, &VertexAttrDescriptor {
            size: 1,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I16,
            stride: TILE_INSTANCE_SIZE,
            offset: 12,
            divisor: 1,
            buffer_index: 1,
        });
        device.configure_vertex_attr(&vertex_array, &ctrl_backdrop_attr, &VertexAttrDescriptor {
            size: 2,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I8,
            stride: TILE_INSTANCE_SIZE,
            offset: 14,
            divisor: 1,
            buffer_index: 1,
        });
        device.bind_buffer(&vertex_array, quad_vertex_indices_buffer, BufferTarget::Index);

        TileVertexArray { vertex_array }
    }
}

pub struct CopyTileVertexArray<D> where D: Device {
    pub vertex_array: D::VertexArray,
}

impl<D> CopyTileVertexArray<D> where D: Device {
    pub fn new(
        device: &D,
        copy_tile_program: &CopyTileProgram<D>,
        copy_tile_vertex_buffer: &D::Buffer,
        quads_vertex_indices_buffer: &D::Buffer,
    ) -> CopyTileVertexArray<D> {
        let vertex_array = device.create_vertex_array();

        let tile_position_attr =
            device.get_vertex_attr(&copy_tile_program.program, "TilePosition").unwrap();

        device.bind_buffer(&vertex_array, copy_tile_vertex_buffer, BufferTarget::Vertex);
        device.configure_vertex_attr(&vertex_array, &tile_position_attr, &VertexAttrDescriptor {
            size: 2,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I16,
            stride: TILE_INSTANCE_SIZE,
            offset: 0,
            divisor: 0,
            buffer_index: 0,
        });
        device.bind_buffer(&vertex_array, quads_vertex_indices_buffer, BufferTarget::Index);

        CopyTileVertexArray { vertex_array }
    }
}

pub struct ClipTileCopyVertexArray<D> where D: Device {
    pub vertex_array: D::VertexArray,
}

impl<D> ClipTileCopyVertexArray<D> where D: Device {
    pub fn new(device: &D,
               clip_tile_copy_program: &ClipTileCopyProgram<D>,
               vertex_buffer: &D::Buffer,
               quad_vertex_positions_buffer: &D::Buffer,
               quad_vertex_indices_buffer: &D::Buffer)
               -> ClipTileCopyVertexArray<D> {
        let vertex_array = device.create_vertex_array();

        let tile_offset_attr =
            device.get_vertex_attr(&clip_tile_copy_program.program, "TileOffset").unwrap();
        let tile_index_attr =
            device.get_vertex_attr(&clip_tile_copy_program.program, "TileIndex").unwrap();

        device.bind_buffer(&vertex_array, quad_vertex_positions_buffer, BufferTarget::Vertex);
        device.configure_vertex_attr(&vertex_array, &tile_offset_attr, &VertexAttrDescriptor {
            size: 2,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I16,
            stride: 4,
            offset: 0,
            divisor: 0,
            buffer_index: 0,
        });
        device.bind_buffer(&vertex_array, &vertex_buffer, BufferTarget::Vertex);
        device.configure_vertex_attr(&vertex_array, &tile_index_attr, &VertexAttrDescriptor {
            size: 1,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I32,
            stride: CLIP_TILE_INSTANCE_SIZE / 2,
            offset: 0,
            divisor: 1,
            buffer_index: 1,
        });
        device.bind_buffer(&vertex_array, quad_vertex_indices_buffer, BufferTarget::Index);

        ClipTileCopyVertexArray { vertex_array }
    }
}

pub struct ClipTileCombineVertexArray<D> where D: Device {
    pub vertex_array: D::VertexArray,
}

impl<D> ClipTileCombineVertexArray<D> where D: Device {
    pub fn new(device: &D,
               clip_tile_combine_program: &ClipTileCombineProgram<D>,
               vertex_buffer: &D::Buffer,
               quad_vertex_positions_buffer: &D::Buffer,
               quad_vertex_indices_buffer: &D::Buffer)
               -> ClipTileCombineVertexArray<D> {
        let vertex_array = device.create_vertex_array();

        let tile_offset_attr =
            device.get_vertex_attr(&clip_tile_combine_program.program, "TileOffset").unwrap();
        let dest_tile_index_attr =
            device.get_vertex_attr(&clip_tile_combine_program.program, "DestTileIndex").unwrap();
        let dest_backdrop_attr =
            device.get_vertex_attr(&clip_tile_combine_program.program, "DestBackdrop").unwrap();
        let src_tile_index_attr =
            device.get_vertex_attr(&clip_tile_combine_program.program, "SrcTileIndex").unwrap();
        let src_backdrop_attr =
            device.get_vertex_attr(&clip_tile_combine_program.program, "SrcBackdrop").unwrap();

        device.bind_buffer(&vertex_array, quad_vertex_positions_buffer, BufferTarget::Vertex);
        device.configure_vertex_attr(&vertex_array, &tile_offset_attr, &VertexAttrDescriptor {
            size: 2,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I16,
            stride: 4,
            offset: 0,
            divisor: 0,
            buffer_index: 0,
        });
        device.bind_buffer(&vertex_array, &vertex_buffer, BufferTarget::Vertex);
        device.configure_vertex_attr(&vertex_array, &dest_tile_index_attr, &VertexAttrDescriptor {
            size: 1,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I32,
            stride: CLIP_TILE_INSTANCE_SIZE,
            offset: 0,
            divisor: 1,
            buffer_index: 1,
        });
        device.configure_vertex_attr(&vertex_array, &dest_backdrop_attr, &VertexAttrDescriptor {
            size: 1,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I32,
            stride: CLIP_TILE_INSTANCE_SIZE,
            offset: 4,
            divisor: 1,
            buffer_index: 1,
        });
        device.configure_vertex_attr(&vertex_array, &src_tile_index_attr, &VertexAttrDescriptor {
            size: 1,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I32,
            stride: CLIP_TILE_INSTANCE_SIZE,
            offset: 8,
            divisor: 1,
            buffer_index: 1,
        });
        device.configure_vertex_attr(&vertex_array, &src_backdrop_attr, &VertexAttrDescriptor {
            size: 1,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I32,
            stride: CLIP_TILE_INSTANCE_SIZE,
            offset: 12,
            divisor: 1,
            buffer_index: 1,
        });
        device.bind_buffer(&vertex_array, quad_vertex_indices_buffer, BufferTarget::Index);

        ClipTileCombineVertexArray { vertex_array }
    }
}

pub struct BlitProgram<D> where D: Device {
    pub program: D::Program,
    pub dest_rect_uniform: D::Uniform,
    pub framebuffer_size_uniform: D::Uniform,
    pub src_texture: D::TextureParameter,
}

impl<D> BlitProgram<D> where D: Device {
    pub fn new(device: &D, resources: &dyn ResourceLoader) -> BlitProgram<D> {
        let program = device.create_raster_program(resources, "blit");
        let dest_rect_uniform = device.get_uniform(&program, "DestRect");
        let framebuffer_size_uniform = device.get_uniform(&program, "FramebufferSize");
        let src_texture = device.get_texture_parameter(&program, "Src");
        BlitProgram { program, dest_rect_uniform, framebuffer_size_uniform, src_texture }
    }
}

pub struct BlitBufferProgram<D> where D: Device {
    pub program: D::Program,
    pub buffer_storage_buffer: D::StorageBuffer,
    pub buffer_size_uniform: D::Uniform,
}

impl<D> BlitBufferProgram<D> where D: Device {
    pub fn new(device: &D, resources: &dyn ResourceLoader) -> BlitBufferProgram<D> {
        let program = device.create_raster_program(resources, "blit_buffer");
        let buffer_storage_buffer = device.get_storage_buffer(&program, "Buffer", 0);
        let buffer_size_uniform = device.get_uniform(&program, "BufferSize");
        BlitBufferProgram { program, buffer_storage_buffer, buffer_size_uniform }
    }
}

pub struct ClearProgram<D> where D: Device {
    pub program: D::Program,
    pub rect_uniform: D::Uniform,
    pub framebuffer_size_uniform: D::Uniform,
    pub color_uniform: D::Uniform,
}

impl<D> ClearProgram<D> where D: Device {
    pub fn new(device: &D, resources: &dyn ResourceLoader) -> ClearProgram<D> {
        let program = device.create_raster_program(resources, "clear");
        let rect_uniform = device.get_uniform(&program, "Rect");
        let framebuffer_size_uniform = device.get_uniform(&program, "FramebufferSize");
        let color_uniform = device.get_uniform(&program, "Color");
        ClearProgram { program, rect_uniform, framebuffer_size_uniform, color_uniform }
    }
}

pub enum FillProgram<D> where D: Device {
    Raster(FillRasterProgram<D>),
    Compute(FillComputeProgram<D>),
}

impl<D> FillProgram<D> where D: Device {
    pub fn new(device: &D, resources: &dyn ResourceLoader, renderer_level: RendererLevel)
               -> FillProgram<D> {
        match renderer_level {
            RendererLevel::D3D11 => {
                FillProgram::Compute(FillComputeProgram::new(device, resources))
            }
            RendererLevel::D3D9 => FillProgram::Raster(FillRasterProgram::new(device, resources)),
        }
    }
}

pub struct FillRasterProgram<D> where D: Device {
    pub program: D::Program,
    pub framebuffer_size_uniform: D::Uniform,
    pub tile_size_uniform: D::Uniform,
    pub area_lut_texture: D::TextureParameter,
}

impl<D> FillRasterProgram<D> where D: Device {
    fn new(device: &D, resources: &dyn ResourceLoader) -> FillRasterProgram<D> {
        let program = device.create_raster_program(resources, "fill");
        let framebuffer_size_uniform = device.get_uniform(&program, "FramebufferSize");
        let tile_size_uniform = device.get_uniform(&program, "TileSize");
        let area_lut_texture = device.get_texture_parameter(&program, "AreaLUT");
        FillRasterProgram {
            program,
            framebuffer_size_uniform,
            tile_size_uniform,
            area_lut_texture,
        }
    }
}

pub struct FillComputeProgram<D> where D: Device {
    pub program: D::Program,
    pub dest_image: D::ImageParameter,
    pub area_lut_texture: D::TextureParameter,
    pub tile_range_uniform: D::Uniform,
    pub fills_storage_buffer: D::StorageBuffer,
    pub fill_tile_map_storage_buffer: D::StorageBuffer,
    pub tiles_storage_buffer: D::StorageBuffer,
}

impl<D> FillComputeProgram<D> where D: Device {
    pub fn new(device: &D, resources: &dyn ResourceLoader) -> FillComputeProgram<D> {
        let mut program = device.create_compute_program(resources, "fill");
        let local_size = ComputeDimensions { x: TILE_WIDTH, y: TILE_HEIGHT / 4, z: 1 };
        device.set_compute_program_local_size(&mut program, local_size);

        let dest_image = device.get_image_parameter(&program, "Dest");
        let area_lut_texture = device.get_texture_parameter(&program, "AreaLUT");
        let tile_range_uniform = device.get_uniform(&program, "TileRange");
        let fills_storage_buffer = device.get_storage_buffer(&program, "Fills", 0);
        let fill_tile_map_storage_buffer = device.get_storage_buffer(&program, "FillTileMap", 1);
        let tiles_storage_buffer = device.get_storage_buffer(&program, "Tiles", 2);

        FillComputeProgram {
            program,
            dest_image,
            area_lut_texture,
            tile_range_uniform,
            fills_storage_buffer,
            fill_tile_map_storage_buffer,
            tiles_storage_buffer,
        }
    }
}

pub struct TileProgram<D> where D: Device {
    pub program: D::Program,
    pub transform_uniform: D::Uniform,
    pub tile_size_uniform: D::Uniform,
    pub texture_metadata_texture: D::TextureParameter,
    pub texture_metadata_size_uniform: D::Uniform,
    pub z_buffer_texture: D::TextureParameter,
    pub z_buffer_texture_size_uniform: D::Uniform,
    pub dest_texture: D::TextureParameter,
    pub color_texture_0: D::TextureParameter,
    pub color_texture_size_0_uniform: D::Uniform,
    pub color_texture_1: D::TextureParameter,
    pub mask_texture_0: D::TextureParameter,
    pub mask_texture_size_0_uniform: D::Uniform,
    pub gamma_lut_texture: D::TextureParameter,
    pub filter_params_0_uniform: D::Uniform,
    pub filter_params_1_uniform: D::Uniform,
    pub filter_params_2_uniform: D::Uniform,
    pub framebuffer_size_uniform: D::Uniform,
    pub ctrl_uniform: D::Uniform,
}

impl<D> TileProgram<D> where D: Device {
    pub fn new(device: &D, resources: &dyn ResourceLoader) -> TileProgram<D> {
        let program = device.create_raster_program(resources, "tile");
        let transform_uniform = device.get_uniform(&program, "Transform");
        let tile_size_uniform = device.get_uniform(&program, "TileSize");
        let texture_metadata_texture = device.get_texture_parameter(&program, "TextureMetadata");
        let texture_metadata_size_uniform = device.get_uniform(&program, "TextureMetadataSize");
        let z_buffer_texture = device.get_texture_parameter(&program, "ZBuffer");
        let z_buffer_texture_size_uniform = device.get_uniform(&program, "ZBufferSize");
        let dest_texture = device.get_texture_parameter(&program, "DestTexture");
        let color_texture_0 = device.get_texture_parameter(&program, "ColorTexture0");
        let color_texture_size_0_uniform = device.get_uniform(&program, "ColorTextureSize0");
        let color_texture_1 = device.get_texture_parameter(&program, "ColorTexture1");
        let mask_texture_0 = device.get_texture_parameter(&program, "MaskTexture0");
        let mask_texture_size_0_uniform = device.get_uniform(&program, "MaskTextureSize0");
        let gamma_lut_texture = device.get_texture_parameter(&program, "GammaLUT");
        let filter_params_0_uniform = device.get_uniform(&program, "FilterParams0");
        let filter_params_1_uniform = device.get_uniform(&program, "FilterParams1");
        let filter_params_2_uniform = device.get_uniform(&program, "FilterParams2");
        let framebuffer_size_uniform = device.get_uniform(&program, "FramebufferSize");
        let ctrl_uniform = device.get_uniform(&program, "Ctrl");

        TileProgram {
            program,
            transform_uniform,
            tile_size_uniform,
            texture_metadata_texture,
            texture_metadata_size_uniform,
            z_buffer_texture,
            z_buffer_texture_size_uniform,
            dest_texture,
            color_texture_0,
            color_texture_size_0_uniform,
            color_texture_1,
            mask_texture_0,
            mask_texture_size_0_uniform,
            gamma_lut_texture,
            filter_params_0_uniform,
            filter_params_1_uniform,
            filter_params_2_uniform,
            framebuffer_size_uniform,
            ctrl_uniform,
        }
    }
}

pub struct CopyTileProgram<D> where D: Device {
    pub program: D::Program,
    pub transform_uniform: D::Uniform,
    pub tile_size_uniform: D::Uniform,
    pub framebuffer_size_uniform: D::Uniform,
    pub src_texture: D::TextureParameter,
}

impl<D> CopyTileProgram<D> where D: Device {
    pub fn new(device: &D, resources: &dyn ResourceLoader) -> CopyTileProgram<D> {
        let program = device.create_raster_program(resources, "tile_copy");
        let transform_uniform = device.get_uniform(&program, "Transform");
        let tile_size_uniform = device.get_uniform(&program, "TileSize");
        let framebuffer_size_uniform = device.get_uniform(&program, "FramebufferSize");
        let src_texture = device.get_texture_parameter(&program, "Src");
        CopyTileProgram {
            program,
            transform_uniform,
            tile_size_uniform,
            framebuffer_size_uniform,
            src_texture,
        }
    }
}

pub struct ClipTileCombineProgram<D> where D: Device {
    pub program: D::Program,
    pub src_texture: D::TextureParameter,
    pub framebuffer_size_uniform: D::Uniform,
}

impl<D> ClipTileCombineProgram<D> where D: Device {
    pub fn new(device: &D, resources: &dyn ResourceLoader) -> ClipTileCombineProgram<D> {
        let program = device.create_raster_program(resources, "tile_clip_combine");
        let src_texture = device.get_texture_parameter(&program, "Src");
        let framebuffer_size_uniform = device.get_uniform(&program, "FramebufferSize");
        ClipTileCombineProgram { program, src_texture, framebuffer_size_uniform }
    }
}

pub struct ClipTileCopyProgram<D> where D: Device {
    pub program: D::Program,
    pub src_texture: D::TextureParameter,
    pub framebuffer_size_uniform: D::Uniform,
}

impl<D> ClipTileCopyProgram<D> where D: Device {
    pub fn new(device: &D, resources: &dyn ResourceLoader) -> ClipTileCopyProgram<D> {
        let program = device.create_raster_program(resources, "tile_clip_copy");
        let src_texture = device.get_texture_parameter(&program, "Src");
        let framebuffer_size_uniform = device.get_uniform(&program, "FramebufferSize");
        ClipTileCopyProgram { program, src_texture, framebuffer_size_uniform }
    }
}

pub struct D3D11Programs<D> where D: Device {
    pub init_program: InitProgram<D>,
    pub bin_compute_program: BinComputeProgram<D>,
    pub dice_compute_program: DiceComputeProgram<D>,
    pub blit_buffer_program: BlitBufferProgram<D>,
    pub propagate_program: PropagateProgram<D>,
}

impl<D> D3D11Programs<D> where D: Device {
    pub fn new(device: &D, resources: &dyn ResourceLoader) -> D3D11Programs<D> {
        D3D11Programs {
            init_program: InitProgram::new(device, resources),
            bin_compute_program: BinComputeProgram::new(device, resources),
            dice_compute_program: DiceComputeProgram::new(device, resources),
            blit_buffer_program: BlitBufferProgram::new(device, resources),
            propagate_program: PropagateProgram::new(device, resources),
        }
    }
}

pub struct PropagateProgram<D> where D: Device {
    pub program: D::Program,
    pub framebuffer_tile_size_uniform: D::Uniform,
    pub column_count_uniform: D::Uniform,
    pub draw_metadata_storage_buffer: D::StorageBuffer,
    pub clip_metadata_storage_buffer: D::StorageBuffer,
    pub backdrops_storage_buffer: D::StorageBuffer,
    pub draw_tiles_storage_buffer: D::StorageBuffer,
    pub clip_tiles_storage_buffer: D::StorageBuffer,
    pub clip_vertex_storage_buffer: D::StorageBuffer,
    pub z_buffer_storage_buffer: D::StorageBuffer,
}

impl<D> PropagateProgram<D> where D: Device {
    pub fn new(device: &D, resources: &dyn ResourceLoader) -> PropagateProgram<D> {
        let mut program = device.create_compute_program(resources, "propagate");
        let local_size = ComputeDimensions { x: PROPAGATE_WORKGROUP_SIZE, y: 1, z: 1 };
        device.set_compute_program_local_size(&mut program, local_size);

        let framebuffer_tile_size_uniform = device.get_uniform(&program, "FramebufferTileSize");
        let column_count_uniform = device.get_uniform(&program, "ColumnCount");
        let draw_metadata_storage_buffer = device.get_storage_buffer(&program, "DrawMetadata", 0);
        let clip_metadata_storage_buffer = device.get_storage_buffer(&program, "ClipMetadata", 1);
        let backdrops_storage_buffer = device.get_storage_buffer(&program, "Backdrops", 2);
        let draw_tiles_storage_buffer = device.get_storage_buffer(&program, "DrawTiles", 3);
        let clip_tiles_storage_buffer = device.get_storage_buffer(&program, "ClipTiles", 4);
        let clip_vertex_storage_buffer =
            device.get_storage_buffer(&program, "ClipVertexBuffer", 5);
        let z_buffer_storage_buffer = device.get_storage_buffer(&program, "ZBuffer", 6);
        PropagateProgram {
            program,
            framebuffer_tile_size_uniform,
            column_count_uniform,
            draw_metadata_storage_buffer,
            clip_metadata_storage_buffer,
            backdrops_storage_buffer,
            draw_tiles_storage_buffer,
            clip_tiles_storage_buffer,
            clip_vertex_storage_buffer,
            z_buffer_storage_buffer,
        }
    }
}

pub struct StencilProgram<D>
where
    D: Device,
{
    pub program: D::Program,
}

impl<D> StencilProgram<D>
where
    D: Device,
{
    pub fn new(device: &D, resources: &dyn ResourceLoader) -> StencilProgram<D> {
        let program = device.create_raster_program(resources, "stencil");
        StencilProgram { program }
    }
}

pub struct StencilVertexArray<D>
where
    D: Device,
{
    pub vertex_array: D::VertexArray,
    pub vertex_buffer: D::Buffer,
    pub index_buffer: D::Buffer,
}

impl<D> StencilVertexArray<D>
where
    D: Device,
{
    pub fn new(device: &D, stencil_program: &StencilProgram<D>) -> StencilVertexArray<D> {
        let vertex_array = device.create_vertex_array();
        let vertex_buffer = device.create_buffer(BufferUploadMode::Static);
        let index_buffer = device.create_buffer(BufferUploadMode::Static);

        let position_attr = device.get_vertex_attr(&stencil_program.program, "Position").unwrap();

        device.bind_buffer(&vertex_array, &vertex_buffer, BufferTarget::Vertex);
        device.configure_vertex_attr(&vertex_array, &position_attr, &VertexAttrDescriptor {
            size: 3,
            class: VertexAttrClass::Float,
            attr_type: VertexAttrType::F32,
            stride: 4 * 4,
            offset: 0,
            divisor: 0,
            buffer_index: 0,
        });
        device.bind_buffer(&vertex_array, &index_buffer, BufferTarget::Index);

        StencilVertexArray { vertex_array, vertex_buffer, index_buffer }
    }
}

pub struct ReprojectionProgram<D> where D: Device {
    pub program: D::Program,
    pub old_transform_uniform: D::Uniform,
    pub new_transform_uniform: D::Uniform,
    pub texture: D::TextureParameter,
}

impl<D> ReprojectionProgram<D> where D: Device {
    pub fn new(device: &D, resources: &dyn ResourceLoader) -> ReprojectionProgram<D> {
        let program = device.create_raster_program(resources, "reproject");
        let old_transform_uniform = device.get_uniform(&program, "OldTransform");
        let new_transform_uniform = device.get_uniform(&program, "NewTransform");
        let texture = device.get_texture_parameter(&program, "Texture");
        ReprojectionProgram { program, old_transform_uniform, new_transform_uniform, texture }
    }
}

pub struct ReprojectionVertexArray<D>
where
    D: Device,
{
    pub vertex_array: D::VertexArray,
}

impl<D> ReprojectionVertexArray<D>
where
    D: Device,
{
    pub fn new(
        device: &D,
        reprojection_program: &ReprojectionProgram<D>,
        quad_vertex_positions_buffer: &D::Buffer,
        quad_vertex_indices_buffer: &D::Buffer,
    ) -> ReprojectionVertexArray<D> {
        let vertex_array = device.create_vertex_array();
        let position_attr = device.get_vertex_attr(&reprojection_program.program, "Position")
                                  .unwrap();

        device.bind_buffer(&vertex_array, quad_vertex_positions_buffer, BufferTarget::Vertex);
        device.configure_vertex_attr(&vertex_array, &position_attr, &VertexAttrDescriptor {
            size: 2,
            class: VertexAttrClass::Int,
            attr_type: VertexAttrType::I16,
            stride: 4,
            offset: 0,
            divisor: 0,
            buffer_index: 0,
        });
        device.bind_buffer(&vertex_array, quad_vertex_indices_buffer, BufferTarget::Index);

        ReprojectionVertexArray { vertex_array }
    }
}

pub struct BinComputeProgram<D> where D: Device {
    pub program: D::Program,
    pub microline_count_uniform: D::Uniform,
    pub max_fill_count_uniform: D::Uniform,
    pub metadata_storage_buffer: D::StorageBuffer,
    pub indirect_draw_params_storage_buffer: D::StorageBuffer,
    pub fills_storage_buffer: D::StorageBuffer,
    pub tiles_storage_buffer: D::StorageBuffer,
    pub microlines_storage_buffer: D::StorageBuffer,
    pub fill_tile_map_storage_buffer: D::StorageBuffer,
    pub backdrops_storage_buffer: D::StorageBuffer,
}

impl<D> BinComputeProgram<D> where D: Device {
    pub fn new(device: &D, resources: &dyn ResourceLoader) -> BinComputeProgram<D> {
        let mut program = device.create_compute_program(resources, "bin");
        let dimensions = ComputeDimensions { x: 64, y: 1, z: 1 };
        device.set_compute_program_local_size(&mut program, dimensions);

        let microline_count_uniform = device.get_uniform(&program, "MicrolineCount");
        let max_fill_count_uniform = device.get_uniform(&program, "MaxFillCount");

        let microlines_storage_buffer = device.get_storage_buffer(&program, "Microlines", 0);
        let metadata_storage_buffer = device.get_storage_buffer(&program, "Metadata", 1);
        let indirect_draw_params_storage_buffer =
            device.get_storage_buffer(&program, "IndirectDrawParams", 2);
        let fills_storage_buffer = device.get_storage_buffer(&program, "Fills", 3);
        let tiles_storage_buffer = device.get_storage_buffer(&program, "Tiles", 4);
        let fill_tile_map_storage_buffer = device.get_storage_buffer(&program, "FillTileMap", 5);
        let backdrops_storage_buffer = device.get_storage_buffer(&program, "Backdrops", 6);

        BinComputeProgram {
            program,
            microline_count_uniform,
            max_fill_count_uniform,
            metadata_storage_buffer,
            indirect_draw_params_storage_buffer,
            fills_storage_buffer,
            tiles_storage_buffer,
            microlines_storage_buffer,
            fill_tile_map_storage_buffer,
            backdrops_storage_buffer,
        }
    }
}

pub struct DiceComputeProgram<D> where D: Device {
    pub program: D::Program,
    pub transform_uniform: D::Uniform,
    pub translation_uniform: D::Uniform,
    pub path_count_uniform: D::Uniform,
    pub last_batch_segment_index_uniform: D::Uniform,
    pub max_microline_count_uniform: D::Uniform,
    pub compute_indirect_params_storage_buffer: D::StorageBuffer,
    pub dice_metadata_storage_buffer: D::StorageBuffer,
    pub points_storage_buffer: D::StorageBuffer,
    pub input_indices_storage_buffer: D::StorageBuffer,
    pub microlines_storage_buffer: D::StorageBuffer,
}

impl<D> DiceComputeProgram<D> where D: Device {
    pub fn new(device: &D, resources: &dyn ResourceLoader) -> DiceComputeProgram<D> {
        let mut program = device.create_compute_program(resources, "dice");
        let dimensions = ComputeDimensions { x: 64, y: 1, z: 1 };
        device.set_compute_program_local_size(&mut program, dimensions);

        let transform_uniform = device.get_uniform(&program, "Transform");
        let translation_uniform = device.get_uniform(&program, "Translation");
        let path_count_uniform = device.get_uniform(&program, "PathCount");
        let last_batch_segment_index_uniform = device.get_uniform(&program,
                                                                  "LastBatchSegmentIndex");
        let max_microline_count_uniform = device.get_uniform(&program, "MaxMicrolineCount");

        let compute_indirect_params_storage_buffer =
            device.get_storage_buffer(&program, "ComputeIndirectParams", 0);
        let dice_metadata_storage_buffer = device.get_storage_buffer(&program, "DiceMetadata", 1);
        let points_storage_buffer = device.get_storage_buffer(&program, "Points", 2);
        let input_indices_storage_buffer = device.get_storage_buffer(&program, "InputIndices", 3);
        let microlines_storage_buffer = device.get_storage_buffer(&program, "Microlines", 4);

        DiceComputeProgram {
            program,
            transform_uniform,
            translation_uniform,
            path_count_uniform,
            last_batch_segment_index_uniform,
            max_microline_count_uniform,
            compute_indirect_params_storage_buffer,
            dice_metadata_storage_buffer,
            points_storage_buffer,
            input_indices_storage_buffer,
            microlines_storage_buffer,
        }
    }
}

pub struct InitProgram<D> where D: Device {
    pub program: D::Program,
    pub path_count_uniform: D::Uniform,
    pub tile_count_uniform: D::Uniform,
    pub tile_path_info_storage_buffer: D::StorageBuffer,
    pub tiles_storage_buffer: D::StorageBuffer,
    pub fill_tile_map_storage_buffer: D::StorageBuffer,
}

impl<D> InitProgram<D> where D: Device {
    pub fn new(device: &D, resources: &dyn ResourceLoader) -> InitProgram<D> {
        let mut program = device.create_compute_program(resources, "init");
        let dimensions = ComputeDimensions { x: 64, y: 1, z: 1 };
        device.set_compute_program_local_size(&mut program, dimensions);

        let path_count_uniform = device.get_uniform(&program, "PathCount");
        let tile_count_uniform = device.get_uniform(&program, "TileCount");

        let tile_path_info_storage_buffer = device.get_storage_buffer(&program, "TilePathInfo", 0);
        let tiles_storage_buffer = device.get_storage_buffer(&program, "Tiles", 1);
        let fill_tile_map_storage_buffer = device.get_storage_buffer(&program, "FillTileMap", 2);

        InitProgram {
            program,
            path_count_uniform,
            tile_count_uniform,
            tile_path_info_storage_buffer,
            tiles_storage_buffer,
            fill_tile_map_storage_buffer,
        }
    }
}

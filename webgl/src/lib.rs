// pathfinder/gl/src/lib.rs
//
// Copyright © 2019 The Pathfinder Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! An OpenGL implementation of the device abstraction.

#[macro_use]
extern crate log;

use web_sys::{
    HtmlCanvasElement,
    WebGl2RenderingContext,
};
use web_sys::WebGl2RenderingContext as WebGl;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use pathfinder_geometry::rect::RectI;
use pathfinder_geometry::vector::Vector2I;
use pathfinder_gpu::resources::ResourceLoader;
use pathfinder_gpu::{RenderTarget, BlendState, BufferData, BufferTarget, BufferUploadMode};
use pathfinder_gpu::{ClearOps, DepthFunc, Device, Primitive, RenderOptions, RenderState};
use pathfinder_gpu::{ShaderKind, StencilFunc, TextureData, TextureFormat, UniformData};
use pathfinder_gpu::{VertexAttrClass, VertexAttrDescriptor, VertexAttrType};
use pathfinder_simd::default::F32x4;
use std::ffi::CString;
use std::mem;
use std::ptr;
use std::str;
use std::time::Duration;

pub struct WebGLDevice {
    context: web_sys::WebGl2RenderingContext
}
impl WebGLDevice {
    // Error checking
    
    #[cfg(debug_assertions)]
    fn ck(&self) {
        let mut num_errors = 0;
        loop {
            let err = self.context.get_error();
            println!("GL error: 0x{:x} ({})", err, match err {
                WebGl::NO_ERROR => break,
                WebGl::INVALID_ENUM => "INVALID_ENUM",
                WebGl::INVALID_VALUE => "INVALID_VALUE",
                WebGl::INVALID_OPERATION => "INVALID_OPERATION",
                WebGl::INVALID_FRAMEBUFFER_OPERATION => "INVALID_FRAMEBUFFER_OPERATION",
                WebGl::OUT_OF_MEMORY => "OUT_OF_MEMORY",
                WebGl::STACK_UNDERFLOW => "STACK_UNDERFLOW",
                WebGl::STACK_OVERFLOW => "STACK_OVERFLOW",
                _ => "Unknown"
            });
        }
        if num_errors > 0 {
            panic!("aborting due to {} errors", num_errors);
        }
    }
    
    #[cfg(not(debug_assertions))]
    fn ck(&self) {}
    
    fn set_texture_parameters(&self, texture: &WebGlTexture) {
        self.bind_texture(texture, 0);
        unsafe {
            self.context.tex_parameter_i32(
                WebGl::TEXTURE_2D,
                WebGl::TEXTURE_MIN_FILTER,
                WebGl::LINEAR as i32,
            );
            self.context.tex_parameter_i32(
                WebGl::TEXTURE_2D,
                WebGl::TEXTURE_MAG_FILTER,
                WebGl::LINEAR as i32,
            );
            self.context.tex_parameter_i32(
                WebGl::TEXTURE_2D,
                WebGl::TEXTURE_WRAP_S,
                WebGl::CLAMP_TO_EDGE as i32,
            );
            self.context.tex_parameter_i32(
                WebGl::TEXTURE_2D,
                WebGl::TEXTURE_WRAP_T,
                WebGl::CLAMP_TO_EDGE as i32,
            );
        }
    }
    fn bind_texture(&self, texture: &WebGlTexture, unit: u32) {
        self.context.active_texture(WebGl::TEXTURE0 + unit);
        self.context.bind_texture(WebGl::TEXTURE_2D, Some(texture.gl_texture));
    }
    fn unbind_texture(&self, unit: u32) {
        self.context.active_texture(WebGl::TEXTURE0 + unit);
        self.context.bind_texture(WebGl::TEXTURE_2D, None);
    }

    fn bind_render_target(&self, attachment: &RenderTarget<WebGlDevice>) {
        let framebuffer = match *attachment {
            RenderTarget::Default => None,
            RenderTarget::Framebuffer(framebuffer) => Some(framebuffer),
        }
        self.context.bind_framebuffer(WebGl::FRAMEBUFFER, framebuffer);
    }

    fn set_render_state(&self, render_state: &RenderState<WebGlDevice>) {
        self.bind_render_target(render_state.target);

        let (origin, size) = (render_state.viewport.origin(), render_state.viewport.size());
        self.context.viewport(origin.x(), origin.y(), size.x(), size.y());

        if render_state.options.clear_ops.has_ops() {
            self.clear(&render_state.options.clear_ops);
        }

        self.use_program(render_state.program);
        self.bind_vertex_array(render_state.vertex_array);
        for (texture_unit, texture) in render_state.textures.iter().enumerate() {
            self.bind_texture(texture, texture_unit as u32);
        }

        for (uniform, data) in render_state.uniforms {
            self.set_uniform(uniform, data);
        }
        self.set_render_options(&render_state.options);
    }

    fn clear(&self, ops: &ClearOps) {
        let mut flags = 0;
        if let Some(color) = ops.color {
            self.context.color_mask(true, true, true, true);
            self.context.clear_color(color.r(), color.g(), color.b(), color.a());
            flags |= WebGl::COLOR_BUFFER_BIT;
        }
        if let Some(depth) = ops.depth {
            self.context.depth_mask(true);
            self.context.clear_depth_f32(depth as _);
            flags |= WebGl::DEPTH_BUFFER_BIT;
        }
        if let Some(stencil) = ops.stencil {
            self.context.stencil_mask(!0);
            self.context.clear_stencil(stencil as i32);
            flags |= WebGl::STENCIL_BUFFER_BIT;
        }
        if flags != 0 {
            self.context.clear(flags);
        }
    }
}

fn slice_to_u8<T>(slice: &[T]) -> &[u8] {
    std::slice::from_raw_parts(slice.as_ptr() as *const u8, slice.len() * mem::size_of<T>())
}

impl Device for WebGLDevice {
    type Buffer = WebGlBuffer;
    type Framebuffer = WebGlFramebuffer;
    type Program = WebGlProgram;
    type Shader = WebGlShader;
    type Texture = WebGlTexture;
    type TimerQuery = WebGlTimerQuery;
    type Uniform = WebGlUniform;
    type VertexArray = WebGlVertexArray;
    type VertexAttr = WebGlVertexAttr;

    fn create_texture(&self, format: TextureFormat, size: Vector2I) -> WebGlTexture {
        let texture = self.context.create_texture();
        self.context.bind_texture(0, Some(&texture));
        self.context.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
            WebGLDevice::TEXTURE_2D,
            0,
            format.gl_internal_format(),
            size.x(),
            size.y(),
            0,
            format,
            format.gl_type(),
            None,
        );

        self.set_texture_parameters(&texture);
        texture
    }

    fn create_texture_from_data(&self, size: Vector2I, data: &[u8]) -> WebGlTexture {
        assert!(data.len() >= size.x() as usize * size.y() as usize);

        let texture = self.context.create_texture();
        self.context.bind_texture(0, Some(&texture));
        self.context.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
            WebGl::TEXTURE_2D,
            0,
            WebGl::R8,
            size.x(),
            size.y(),
            0,
            WebGl::REED,
            WebGl::UNSIGNED_BYTE,
            Some(data),
        );

        self.set_texture_parameters(&texture);
        texture
    }

    fn create_shader_from_source(&self, name: &str, source: &[u8], kind: ShaderKind) -> GLShader {
        let glsl_version_spec = "300 es";

        let mut output = vec![];
        self.preprocess(&mut output, source, glsl_version_spec);
        let source = output;

        let gl_shader_kind = match kind {
            ShaderKind::Vertex => WebGl::VERTEX_SHADER,
            ShaderKind::Fragment => WebGl::FRAGMENT_SHADER,
        };

        let gl_shader = self.context.create_shader(gl_shader_kind).expect("could not create shader");
        self.context.compile_shader(&gl_shader);
        let compile_status = self.context.get_shader_parameter(raw_shader, WebGl::COMPILE_STATUS)
        if !compile_status.as_bool().unwrap_or(false) {
            let info_log = self.context.get_shader_info_log(gl_shader).unwrap_or_default();
            println!("Shader info log:\n{}", String::from_utf8_lossy(&info_log));
            panic!("{:?} shader '{}' compilation failed", kind, name);
        }

        GLShader {
            context: self.context.clone(),
            gl_shader
        }
    }

    fn create_program_from_shaders(&self,
                                   _resources: &dyn ResourceLoader,
                                   name: &str,
                                   vertex_shader: GLShader,
                                   fragment_shader: GLShader)
                                   -> GLProgram {
        let gl_program = self.context.create_program().expect("unable to create program object");
        self.context.attach_shader(gl_program, vertex_shader.gl_shader);
        self.context.attach_shader(gl_program, fragment_shader.gl_shader);
        self.context.link_program();
        if !self.context.get_program_parameter(raw_program, WebGl::LINK_STATUS).as_bool().unwrap_or(false) {
            let info_log = self.context.get_shader_info_log(gl_program).unwrap_or_default();
            println!("Program info log:\n{}", String::from_utf8_lossy(&info_log));
            panic!("Program {:?} linking failed", name);
        }

        GLProgram { gl_program, vertex_shader, fragment_shader }
    }

    #[inline]
    fn create_vertex_array(&self) -> WebGLVertexArray {
        WebGLVertexArray {
            context: self.context.clone(),
            vertex_array: self.context.create_vertex_array.unwrap()
        }
    }

    fn get_vertex_attr(&self, program: &GLProgram, name: &str) -> Option<WebGLVertexAttr> {
        let name = format!("a{}", name);
        let attr = self.context.get_attrib_location(program.gl_program, &name);
        if attr < 0 {
            return None;
        }
        Some(WebGlVertexAttr { attr: attr as u32 })
    }

    fn get_uniform(&self, program: &GLProgram, name: &str) -> WebGLUniform {
        let name = format!("u{}", name);
        let location = self.context.get_uniform_location(program.gl_program, &name).unwrap();
        WebGLUniform { location }
    }

    fn configure_vertex_attr(&self,
                             vertex_array: &WebGLVertexArray,
                             attr: &WebGLVertexAttr,
                             descriptor: &VertexAttrDescriptor) {
        debug_assert_ne!(descriptor.stride, 0);

        self.context.bind_vertex_array(vertex_array.gl_vertex_array);

        let attr_type = descriptor.attr_type.to_gl_type();
        match descriptor.class {
            VertexAttrClass::Float | VertexAttrClass::FloatNorm => {
                let normalized = descriptor.class == VertexAttrClass::FloatNorm;
                self.context.vertex_attrib_pointer_with_i32(
                    attr.attr,
                    descriptor.size as i32,
                    attr_type,
                    normalized,
                    descriptor.stride as i32,
                    descriptor.offset as i32
                );
            }
            VertexAttrClass::Int => {
                self.context.vertex_attrib_pointer_with_i32(
                    attr.attr,
                    descriptor.size as i32,
                    attr_type,
                    descriptor.stride as i32,
                    descriptor.offset as i32
                );
            }

            self.contextvertex_attrib_divisor(attr.attr, descriptor.divisor); ck();
            self.context.enable_vertex_attrib_array(attr.attr);
        }

        self.context.bind_vertex_array(None);
    }

    fn create_framebuffer(&self, texture: GLTexture) -> WebGLFramebuffer {
        let gl_framebuffer = self.context.create_framebuffer().unwrap();
        self.context.bind_framebuffer(WebGl::FRAMEBUFFER, Some(gl_framebuffer));
        self.bind_texture(texture, 0);
        self.context.framebuffer_texture_2d(
            WebGl::FRAMEBUFFER,
            WebGl::COLOR_ATTACHMENT0,
            WebGl::TEXTURE_2D,
            Some(texture.gl_texture),
            0
        );
        assert_eq!(
            self.context.check_framebuffer_status(WebGl::FRAMEBUFFER),
            WebGl::FRAMEBUFFER_COMPLETE)
        );

        GLFramebuffer { context: self.context.clone(), gl_framebuffer, texture }
    }

    fn create_buffer(&self) -> GLBuffer {
        let gl_buffer = self.context.create_buffer().unwrap();
        GLBuffer { gl_buffer }
    }

    fn allocate_buffer<T>(&self,
                          buffer: &GLBuffer,
                          data: BufferData<T>,
                          target: BufferTarget,
                          mode: BufferUploadMode) {
        let target = match target {
            BufferTarget::Vertex => WebGl::ARRAY_BUFFER,
            BufferTarget::Index => WebGl::ELEMENT_ARRAY_BUFFER,
        };
        self.context.bind_buffer(buffer.gl_buffer);
        let usage = mode.to_gl_usage();
        let (ptr, len) = match data {
            BufferData::Uninitialized(len) =>
                self.context.buffer_data_with_i32(target, len * mem::size_of<T>(), usage),
            BufferData::Memory(buffer) =>
                self.context.buffer_data_u8_array(target, slice_to_u8(buffer), usage),
        }
    }

    #[inline]
    fn framebuffer_texture<'f>(&self, framebuffer: &'f Self::Framebuffer) -> &'f Self::Texture {
        &framebuffer.texture
    }

    #[inline]
    fn texture_size(&self, texture: &Self::Texture) -> Vector2I {
        texture.size
    }

    fn upload_to_texture(&self, texture: &Self::Texture, size: Vector2I, data: &[u8]) {
        assert!(data.len() >= size.x() as usize * size.y() as usize * 4);
        
        self.bind_texture(texture, 0);
        self.context.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
            WebGl::TEXTURE_2D,
            0,
            WebGl::RGBA as i32,
            size.x() as i32,
            size.y() as i32,
            0,
            WebGl::RGBA,
            WebGl::UNSIGNED_BYTE,
            Some(data)
        );

        self.set_texture_parameters(texture);
    }

    fn read_pixels(&self, render_target: &RenderTarget<GLDevice>, viewport: RectI) -> TextureData {
        panic!("read_pixels is not supported");
    }

    fn begin_commands(&self) {
        // TODO(pcwalton): Add some checks in debug mode to make sure render commands are bracketed
        // by these?
    }

    fn end_commands(&self) {
        self.context.flush();
    }

    fn draw_arrays(&self, index_count: u32, render_state: &RenderState<Self>) {
        self.set_render_state(render_state);
        self.context.draw_arrays(
            render_state.primitive.to_gl_primitive(),
            0,
            index_count as i32
        );
        self.reset_render_state(render_state);
    }

    fn draw_elements(&self, index_count: u32, render_state: &RenderState<Self>) {
        self.set_render_state(render_state);
        self.context.draw_elements(
            render_state.primitive.to_gl_primitive(),
            index_count as i32,
            WebGl::UNSIGNED_INT,
            0
        );
        self.reset_render_state(render_state);
    }

    fn draw_elements_instanced(&self,
                               index_count: u32,
                               instance_count: u32,
                               render_state: &RenderState<Self>) {
        self.set_render_state(render_state);
        self.context.draw_elements_instanced(
            render_state.primitive.to_gl_primitive(),
            index_count as GLsizei,
            WebGl::UNSIGNED_INT,
            0,
            instance_count as i32
        );
        self.reset_render_state(render_state);
    }

    #[inline]
    fn create_timer_query(&self) -> GLTimerQuery {
        // FIXME use performance timers
        GLTimerQuery {}
    }

    #[inline]
    fn begin_timer_query(&self, query: &Self::TimerQuery) {
        // FIXME use performance timers
    }

    #[inline]
    fn end_timer_query(&self, _: &Self::TimerQuery) {
        // FIXME use performance timers
    }

    #[inline]
    fn get_timer_query(&self, query: &Self::TimerQuery) -> Option<Duration> {
        // FIXME use performance timers
        None
    }

    #[inline]
    fn bind_buffer(&self, vertex_array: &GLVertexArray, buffer: &GLBuffer, target: BufferTarget) {
        self.bind_vertex_array(vertex_array);
        self.context.bind_buffer(target.to_gl_target(), buffer.gl_buffer);
        self.unbind_vertex_array();
    }

    #[inline]
    fn create_shader(
        &self,
        resources: &dyn ResourceLoader,
        name: &str,
        kind: ShaderKind,
    ) -> Self::Shader {
        let suffix = match kind {
            ShaderKind::Vertex => 'v',
            ShaderKind::Fragment => 'f',
        };
        let path = format!("shaders/gl3/{}.{}s.glsl", name, suffix);
        self.create_shader_from_source(name, &resources.slurp(&path).unwrap(), kind)
    }
}

pub struct WebGlVertexArray {
    context: web_sys::WebGl2RenderingContext,
    pub vertex_array: web_sys::WebGlVertexArrayObject,
}

impl Drop for WebGlVertexArray {
    #[inline]
    fn drop(&mut self) {
        self.context.delete_vertex_array(Some(&self.vertex_array));
    }
}

pub struct WebGLVertexAttr {
    attr: u32,
}

pub struct WebGlFrameBuffer {
    context: web_sys::WebGl2RenderingContext,
    pub framebuffer: web_sys::WebGlFrameBuffer,
    pub texture: web_sys::WebGlTexture,
}

impl Drop for WebGlFrameBuffer {
    fn drop(&mut self) {
        self.context.delete_framebuffer(Some(&self.framebuffer));
    }
}

pub struct WebGlBuffer {
    context: web_sys::WebGl2RenderingContext,
    pub buffer: web_sys::WebGlBuffer,
}

impl Drop for WebGlBuffer {
    fn drop(&mut self) {
        self.context.delete_buffer(Some(&self.buffer));
    }
}

#[derive(Debug)]
pub struct WebGlUniform {
    location: u32,
}

pub struct WebGlProgram {
    context: web_sys::WebGl2RenderingContext,
    pub program: web_sys::WebGlProgram,
    vertex_shader: web_sys::WebGlShader,
    fragment_shader: web_sys::WebGlShader,
}

impl Drop for WebGlProgram {
    fn drop(&mut self) {
        self.context.delete_program(Some(&self.program));
    }
}

pub struct WebGlShader {
    context: web_sys::WebGl2RenderingContext,
    shader: web_sys::WebGlShader,
}

impl Drop for GLShader {
    fn drop(&mut self) {
        self.context.delete_shader(Some(&self.shader));
    }
}

pub struct WebGlTexture {
    context: web_sys::WebGl2RenderingContext,
    texture: web_sys::WebGlTexture,
    pub size: Vector2I,
    pub format: TextureFormat,
}
impl Drop for WebGlTexture {
    fn drop(&mut self) {
        self.context.delete_texture(Some(&self.texture));
    }
}

pub struct WebGlTimerQuery {
}


trait BufferTargetExt {
    fn to_gl_target(self) -> u32;
}

impl BufferTargetExt for BufferTarget {
    fn to_gl_target(self) -> u32 {
        match self {
            BufferTarget::Vertex => WebGl::ARRAY_BUFFER,
            BufferTarget::Index => WebGl::ELEMENT_ARRAY_BUFFER,
        }
    }
}

trait BufferUploadModeExt {
    fn to_gl_usage(self) -> u32;
}

impl BufferUploadModeExt for BufferUploadMode {
    fn to_gl_usage(self) -> u32 {
        match self {
            BufferUploadMode::Static => WebGl::STATIC_DRAW,
            BufferUploadMode::Dynamic => WebGl::DYNAMIC_DRAW,
        }
    }
}

trait DepthFuncExt {
    fn to_gl_depth_func(self) -> u32;
}

impl DepthFuncExt for DepthFunc {
    fn to_gl_depth_func(self) -> u32 {
        match self {
            DepthFunc::Less => WebGl::LESS,
            DepthFunc::Always => WebGl::ALWAYS,
        }
    }
}

trait PrimitiveExt {
    fn to_gl_primitive(self) -> u32;
}

impl PrimitiveExt for Primitive {
    fn to_gl_primitive(self) -> u32 {
        match self {
            Primitive::Triangles => WebGl::TRIANGLES,
            Primitive::Lines => WebGl::LINES,
        }
    }
}

trait StencilFuncExt {
    fn to_gl_stencil_func(self) -> u32;
}

impl StencilFuncExt for StencilFunc {
    fn to_gl_stencil_func(self) -> u32 {
        match self {
            StencilFunc::Always => WebGl::ALWAYS,
            StencilFunc::Equal => WebGl::EQUAL,
        }
    }
}

trait TextureFormatExt {
    fn gl_internal_format(self) -> u32;
    fn gl_format(self) -> u32;
    fn gl_type(self) -> u32;
}

impl TextureFormatExt for TextureFormat {
    fn gl_internal_format(self) -> u32 {
        match self {
            TextureFormat::R8 => WebGl::R8,
            TextureFormat::R16F => WebGl::R16F,
            TextureFormat::RGBA8 => WebGl::RGBA,
        }
    }

    fn gl_format(self) -> u32 {
        match self {
            TextureFormat::R8 | TextureFormat::R16F => WebGl::RED,
            TextureFormat::RGBA8 => WebGl::RGBA,
        }
    }

    fn gl_type(self) -> u32 {
        match self {
            TextureFormat::R8 | TextureFormat::RGBA8 => WebGl::UNSIGNED_BYTE,
            TextureFormat::R16F => WebGl::HALF_FLOAT,
        }
    }
}

trait VertexAttrTypeExt {
    fn to_gl_type(self) -> u32;
}

impl VertexAttrTypeExt for VertexAttrType {
    fn to_gl_type(self) -> u32 {
        match self {
            VertexAttrType::F32 => WebGl::FLOAT,
            VertexAttrType::I16 => WebGl::SHORT,
            VertexAttrType::I8  => WebGl::BYTE,
            VertexAttrType::U16 => WebGl::UNSIGNED_SHORT,
            VertexAttrType::U8  => WebGl::UNSIGNED_BYTE,
        }
    }
}

/// The version/dialect of OpenGL we should render with.
#[derive(Clone, Copy)]
#[repr(u32)]
pub enum GLVersion {
    /// OpenGL 3.0+, core profile.
    GL3 = 0,
    /// OpenGL ES 3.0+.
    GLES3 = 1,
}

impl GLVersion {
    fn to_glsl_version_spec(&self) -> &'static str {
        match *self {
            GLVersion::GL3 => "330",
            GLVersion::GLES3 => "300 es",
        }
    }
}

// Utilities

// Flips a buffer of image data upside-down.
fn flip_y<T>(pixels: &mut [T], size: Vector2I, channels: usize) {
    let stride = size.x() as usize * channels;
    for y in 0..(size.y() as usize / 2) {
        let (index_a, index_b) = (y * stride, (size.y() as usize - y - 1) * stride);
        for offset in 0..stride {
            pixels.swap(index_a + offset, index_b + offset);
        }
    }
}

use surfman::{Connection, ContextAttributeFlags, ContextAttributes, GLApi, GLVersion};
use surfman::{SurfaceAccess, SurfaceType};
use pathfinder_gl::{GLDevice};
use pathfinder_renderer::{
    gpu::renderer::{Renderer},
    scene::Scene,
    gpu::options::{RendererMode, RendererLevel, RendererOptions},
};
use pathfinder_gpu::{Device, RenderTarget, TextureData};
use pathfinder_resources::embedded::EmbeddedResourceLoader;
use pathfinder_geometry::{
    vector::Vector2I,
    rect::RectI,
};
use image::{RgbaImage, DynamicImage, ImageOutputFormat};
use euclid::Size2D;
use gl;
use std::io;

pub enum Mode {
    Software,
    Hardware,
}

pub fn export_png<W: io::Write>(scene: &Scene, writer: &mut W) -> io::Result<()> {
    let image = export_raster(scene, 1.0, None);
    DynamicImage::ImageRgba8(image).write_to(writer, ImageOutputFormat::Png).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

pub fn export_raster(scene: &Scene, scale: f32, mode: Option<Mode>) -> RgbaImage {
    let image_size = (scene.view_box().size() * scale).ceil().to_i32();
    let width = image_size.x() as u32;
    let height = image_size.y() as u32;
    
    let connection = Connection::new().unwrap();

    let adapter = match mode {
        Some(Mode::Software) => connection.create_software_adapter().unwrap(),
        Some(Mode::Hardware) => connection.create_hardware_adapter().unwrap(),
        None => connection.create_adapter().unwrap()
    };

    let mut device = connection.create_device(&adapter).unwrap();

    let context_attributes = ContextAttributes {
        version: GLVersion::new(3, 3),
        flags: ContextAttributeFlags::empty(),
    };
    let context_descriptor = device.create_context_descriptor(&context_attributes).unwrap();
    let mut context = device.create_context(&context_descriptor).unwrap();
    let surface = device.create_surface(&context, SurfaceAccess::GPUOnly, SurfaceType::Generic {
        size: Size2D::new(width as i32, height as i32),
    }).unwrap();
    device.bind_surface_to_context(&mut context, surface).unwrap();

    device.make_context_current(&context).unwrap();
    gl::load_with(|symbol_name| device.get_proc_address(&context, symbol_name));
    let surface_info = device.context_surface_info(&context).unwrap().unwrap();
    let gl_device = GLDevice::new(pathfinder_gl::GLVersion::GL3, surface_info.framebuffer_object);

    let render_mode = RendererMode::default_for_device(&gl_device);
    let renderer = Renderer::new(gl_device, &EmbeddedResourceLoader, render_mode, RendererOptions::default());

    let viewport = RectI::new(Vector2I::default(), image_size);
    let texture_data_receiver =
        renderer.device().read_pixels(&RenderTarget::Default, viewport);
    let pixels = match renderer.device().recv_texture_data(&texture_data_receiver) {
        TextureData::U8(pixels) => pixels,
        _ => panic!("Unexpected pixel format for default framebuffer!"),
    };
    let image = RgbaImage::from_raw(width, height, pixels).unwrap();

    device.destroy_context(&mut context).unwrap();

    image
}

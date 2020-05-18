// pathfinder/renderer/src/gpu/options.rs
//
// Copyright © 2019 The Pathfinder Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use pathfinder_color::ColorF;
use pathfinder_geometry::rect::RectI;
use pathfinder_geometry::vector::Vector2I;
use pathfinder_gpu::{Device, FeatureLevel};

/// Options that influence rendering.
pub struct RendererOptions {
    /// The level of hardware features that the renderer will attempt to use.
    pub level: RendererLevel,
    /// The background color. If not present, transparent is assumed.
    pub background_color: Option<ColorF>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RendererLevel {
    /// Direct3D 9/OpenGL 3.0/WebGL 2.0 compatibility. Bin on CPU, fill and composite on GPU.
    D3D9,
    /// Direct3D 11/OpenGL 4.3/Metal/Vulkan/WebGPU compatibility. Bin, fill, and composite on GPU.
    D3D11,
}

impl RendererOptions {
    pub fn default_for_device<D>(device: &D) -> RendererOptions where D: Device {
        RendererOptions {
            level: RendererLevel::default_for_device(device),
            background_color: None,
        }
    }
}

impl RendererLevel {
    pub fn default_for_device<D>(device: &D) -> RendererLevel where D: Device {
        match device.feature_level() {
            FeatureLevel::D3D10 => RendererLevel::D3D9,
            FeatureLevel::D3D11 => RendererLevel::D3D11,
        }
    }
}

#[derive(Clone)]
pub enum DestFramebuffer<D> where D: Device {
    Default {
        viewport: RectI,
        window_size: Vector2I,
    },
    Other(D::Framebuffer),
}

impl<D> Default for DestFramebuffer<D> where D: Device {
    #[inline]
    fn default() -> DestFramebuffer<D> {
        DestFramebuffer::Default { viewport: RectI::default(), window_size: Vector2I::default() }
    }
}

impl<D> DestFramebuffer<D>
where
    D: Device,
{
    #[inline]
    pub fn full_window(window_size: Vector2I) -> DestFramebuffer<D> {
        let viewport = RectI::new(Vector2I::default(), window_size);
        DestFramebuffer::Default { viewport, window_size }
    }

    #[inline]
    pub fn window_size(&self, device: &D) -> Vector2I {
        match *self {
            DestFramebuffer::Default { window_size, .. } => window_size,
            DestFramebuffer::Other(ref framebuffer) => {
                device.texture_size(device.framebuffer_texture(framebuffer))
            }
        }
    }
}

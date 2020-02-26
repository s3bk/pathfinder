#version 330

// pathfinder/shaders/tile_solid.fs.glsl
//
// Copyright © 2019 The Pathfinder Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

precision highp float;

uniform sampler2D uPaintTexture;

in vec2 vColorTexCoord;

out vec4 oFragColor;

void main() {
    vec4 color = texture(uPaintTexture, vColorTexCoord);
    oFragColor = vec4(color.rgb * color.a, color.a);
}

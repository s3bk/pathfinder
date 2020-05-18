#version 430

// pathfinder/shaders/blit_buffer.fs.glsl
//
// Copyright © 2020 The Pathfinder Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

precision highp float;

#ifdef GL_ES
precision highp sampler2D;
#endif

uniform ivec2 uBufferSize;

layout(std430, binding = 0) buffer bBuffer {
    restrict int iBuffer[];
};

in vec2 vTexCoord;

out vec4 oFragColor;

void main() {
    ivec2 texCoord = ivec2(floor(vTexCoord));
    int value = iBuffer[texCoord.y * uBufferSize.x + texCoord.x];
    oFragColor = vec4(value & 0xff,
                      (value >> 8) & 0xff,
                      (value >> 16) & 0xff,
                      (value >> 24) & 0xff) / 255.0;
}

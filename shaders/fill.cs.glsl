#version 430

// pathfinder/shaders/fill.cs.glsl
//
// Copyright © 2020 The Pathfinder Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#extension GL_GOOGLE_include_directive : enable

precision highp float;

#ifdef GL_ES
precision highp sampler2D;
#endif

#include "fill_area.inc.glsl"

layout(local_size_x = 16, local_size_y = 4) in;

#define TILE_FIELD_NEXT_TILE_ID             0
#define TILE_FIELD_FIRST_FILL_ID            1
#define TILE_FIELD_BACKDROP_ALPHA_TILE_ID   2
#define TILE_FIELD_CONTROL                  3

uniform writeonly image2D uDest;
uniform sampler2D uAreaLUT;
uniform int uAlphaTileCount;

layout(std430, binding = 0) buffer bFills {
    restrict readonly uint iFills[];
};

layout(std430, binding = 1) buffer bTiles {
    // [0]: path ID
    // [1]: next tile ID
    // [2]: first fill ID
    // [3]: backdrop delta upper 8 bits, alpha tile ID lower 24 bits
    // [4]: color/ctrl/backdrop word
    restrict uint iTiles[];
};

layout(std430, binding = 2) buffer bAlphaTileIndices {
    // List of alpha tile indices.
    restrict readonly uint iAlphaTileIndices[];
};

#include "fill_compute.inc.glsl"

void main() {
    ivec2 tileSubCoord = ivec2(gl_LocalInvocationID.xy) * ivec2(1, 4);

    // This is a workaround for the 64K workgroup dispatch limit in OpenGL.
    uint alphaTileIndex = (gl_WorkGroupID.x | (gl_WorkGroupID.y << 15));
    if (alphaTileIndex >= uAlphaTileCount)
        return;

    uint tileIndex = iAlphaTileIndices[alphaTileIndex];
    int fillIndex = int(iTiles[tileIndex * 4 + TILE_FIELD_FIRST_FILL_ID]);
    vec4 coverages = accumulateCoverageForFillList(fillIndex, tileSubCoord);

    ivec2 tileOrigin = ivec2(16, 4) *
        ivec2(alphaTileIndex & 0xff,
              (alphaTileIndex >> 8u) & 0xff + (((alphaTileIndex >> 16u) & 0xff) << 8u));
    ivec2 destCoord = tileOrigin + ivec2(gl_LocalInvocationID.xy);
    imageStore(uDest, destCoord, coverages);
}

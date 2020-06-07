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

uniform writeonly image2D uDest;
uniform sampler2D uAreaLUT;
uniform ivec2 uTileRange;

layout(std430, binding = 0) buffer bFills {
    restrict readonly uint iFills[];
};

layout(std430, binding = 1) buffer bTileLinkMap {
    // [0]: index of first fill in this tile
    // [1]: index of next tile
    restrict readonly int iTileLinkMap[];
};

layout(std430, binding = 2) buffer bTiles {
    restrict readonly int iTiles[];
};

#include "fill_compute.inc.glsl"

void main() {
    ivec2 tileSubCoord = ivec2(gl_LocalInvocationID.xy) * ivec2(1, 4);

    // This is a workaround for the 64K workgroup dispatch limit in OpenGL.
    uint tileIndexOffset = gl_WorkGroupID.x | (gl_WorkGroupID.y << 15);
    uint tileIndex = tileIndexOffset + uint(uTileRange.x);
    if (tileIndex >= uTileRange.y)
        return;

    int fillIndex = iTileLinkMap[tileIndex * 2 + 0];
    if (fillIndex < 0)
        return;

    vec4 coverages = accumulateCoverageForFillList(fillIndex, tileSubCoord);

    // The `tileIndex` value refers to a *global* tile index, and we have to convert that to an
    // alpha tile index.
    uint alphaTileIndex = iTiles[tileIndex * 4 + 1];

    ivec2 tileOrigin = ivec2(16, 4) *
        ivec2(alphaTileIndex & 0xff,
              (alphaTileIndex >> 8u) & 0xff + (((alphaTileIndex >> 16u) & 0xff) << 8u));
    ivec2 destCoord = tileOrigin + ivec2(gl_LocalInvocationID.xy);
    imageStore(uDest, destCoord, coverages);
}

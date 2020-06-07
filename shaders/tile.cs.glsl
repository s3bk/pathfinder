#version 430

// pathfinder/shaders/tile.cs.glsl
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

layout(local_size_x = 16, local_size_y = 4) in;

#include "tile_fragment.inc.glsl"
#include "tile_vertex.inc.glsl"

#define LOAD_ACTION_CLEAR   0
#define LOAD_ACTION_LOAD    1

uniform int uLoadAction;
uniform vec4 uClearColor;
uniform vec2 uTileSize;
uniform sampler2D uTextureMetadata;
uniform ivec2 uTextureMetadataSize;
uniform sampler2D uZBuffer;
uniform ivec2 uZBufferSize;
uniform sampler2D uColorTexture0;
uniform sampler2D uMaskTexture0;
uniform sampler2D uDestTexture;
uniform sampler2D uGammaLUT;
uniform vec2 uColorTextureSize0;
uniform vec2 uMaskTextureSize0;
uniform vec4 uFilterParams0;
uniform vec4 uFilterParams1;
uniform vec4 uFilterParams2;
uniform vec2 uFramebufferSize;
uniform int uCtrl;
uniform ivec2 uFramebufferTileSize;
layout(rgba8) uniform image2D uDestImage;

layout(std430, binding = 0) buffer bTiles {
    restrict uint iTiles[];
};

layout(std430, binding = 1) buffer bTileLinkMap {
    // [0]: index of first fill in this tile
    // [1]: index of next tile
    restrict int iTileLinkMap[];
};

layout(std430, binding = 2) buffer bFirstTileMap {
    restrict int iFirstTileMap[];
};

uint calculateTileIndex(uint bufferOffset, uvec4 tileRect, uvec2 tileCoord) {
    return bufferOffset + tileCoord.y * (tileRect.z - tileRect.x) + tileCoord.x;
}

ivec2 toImageCoords(ivec2 coords) {
    return ivec2(coords.x, uFramebufferSize.y - coords.y);
}

void main() {
    ivec2 tileCoord = ivec2(gl_WorkGroupID.xy);
    ivec2 firstTileSubCoord = ivec2(gl_LocalInvocationID.xy) * ivec2(1, 4);
    ivec2 firstFragCoord = tileCoord * ivec2(uTileSize) + firstTileSubCoord;

    mat4 destColors;
    for (int subY = 0; subY < 4; subY++) {
        if (uLoadAction == LOAD_ACTION_CLEAR) {
            destColors[subY] = uClearColor;
        } else {
            ivec2 imageCoords = toImageCoords(firstFragCoord + ivec2(0, subY));
            destColors[subY] = imageLoad(uDestImage, imageCoords);
        }
    }

    int tileIndex = iFirstTileMap[tileCoord.x + uFramebufferTileSize.x * tileCoord.y];
    while (tileIndex >= 0) {
        for (int subY = 0; subY < 4; subY++) {
            ivec2 tileSubCoord = firstTileSubCoord + ivec2(0, subY);
            vec2 fragCoord = vec2(firstFragCoord + ivec2(0, subY)) + vec2(0.5);

            uint alphaTileIndex = int(iTiles[tileIndex * 4 + 1]);
            uint tileControlWord = iTiles[tileIndex * 4 + 3];
            uint colorEntry = tileControlWord & 0xffff;
            int tileCtrl = int((tileControlWord >> 16) & 0xff);
            int backdrop = int(tileControlWord) >> 24;

            uvec2 maskTileCoord = uvec2(alphaTileIndex & 0xff, alphaTileIndex >> 8) *
                uvec2(uTileSize);
            vec3 maskTexCoord0 = vec3(vec2(ivec2(maskTileCoord) + tileSubCoord), backdrop);

            vec2 colorTexCoord0;
            vec4 baseColor;
            computeTileVaryings(fragCoord,
                                int(colorEntry),
                                uTextureMetadata,
                                uTextureMetadataSize,
                                colorTexCoord0,
                                baseColor);

            vec4 srcColor = calculateColor(fragCoord,
                                           uColorTexture0,
                                           uMaskTexture0,
                                           uDestTexture,
                                           uGammaLUT,
                                           uColorTextureSize0,
                                           uMaskTextureSize0,
                                           uFilterParams0,
                                           uFilterParams1,
                                           uFilterParams2,
                                           uFramebufferSize,
                                           uCtrl,
                                           maskTexCoord0,
                                           colorTexCoord0,
                                           baseColor,
                                           tileCtrl);

            destColors[subY] = destColors[subY] * (1.0 - srcColor.a) + srcColor;
        }

        tileIndex = iTileLinkMap[tileIndex * 2 + 1];
    }

    for (int subY = 0; subY < 4; subY++)
        imageStore(uDestImage, toImageCoords(firstFragCoord + ivec2(0, subY)), destColors[subY]);
}

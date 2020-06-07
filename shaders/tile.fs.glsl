#version 330

// pathfinder/shaders/tile.fs.glsl
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

#include "tile_fragment.inc.glsl"

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

in vec3 vMaskTexCoord0;
in vec2 vColorTexCoord0;
in vec4 vBaseColor;
in float vTileCtrl;

out vec4 oFragColor;

// Entry point
//
// TODO(pcwalton): Generate this dynamically.

void main() {
    oFragColor = calculateColor(gl_FragCoord.xy,
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
                                vMaskTexCoord0,
                                vColorTexCoord0,
                                vBaseColor,
                                int(vTileCtrl));
}

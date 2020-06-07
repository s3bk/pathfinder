// pathfinder/shaders/tile_vertex.inc.glsl
//
// Copyright © 2020 The Pathfinder Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

void computeTileVaryings(vec2 position,
                         int colorEntry,
                         sampler2D textureMetadata,
                         ivec2 textureMetadataSize,
                         out vec2 outColorTexCoord0,
                         out vec4 outBaseColor) {
    vec2 textureMetadataScale = vec2(1.0) / vec2(textureMetadataSize);
    vec2 metadataEntryCoord = vec2(colorEntry % 128 * 4, colorEntry / 128);
    vec2 colorTexMatrix0Coord = (metadataEntryCoord + vec2(0.5, 0.5)) * textureMetadataScale;
    vec2 colorTexOffsetsCoord = (metadataEntryCoord + vec2(1.5, 0.5)) * textureMetadataScale;
    vec2 baseColorCoord = (metadataEntryCoord + vec2(2.5, 0.5)) * textureMetadataScale;
    vec4 colorTexMatrix0 = texture(textureMetadata, colorTexMatrix0Coord);
    vec4 colorTexOffsets = texture(textureMetadata, colorTexOffsetsCoord);
    vec4 baseColor = texture(textureMetadata, baseColorCoord);

    outColorTexCoord0 = mat2(colorTexMatrix0) * position + colorTexOffsets.xy;
    outBaseColor = baseColor;
}

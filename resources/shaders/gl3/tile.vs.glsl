#version {{version}}
// Automatically generated from files in pathfinder/shaders/. Do not edit!












#extension GL_GOOGLE_include_directive : enable

precision highp float;
















void computeTileVaryings(vec2 position,
                         int colorEntry,
                         sampler2D textureMetadata,
                         ivec2 textureMetadataSize,
                         out vec2 outColorTexCoord0,
                         out vec4 outBaseColor){
    vec2 textureMetadataScale = vec2(1.0)/ vec2(textureMetadataSize);
    vec2 metadataEntryCoord = vec2(colorEntry % 128 * 4, colorEntry / 128);
    vec2 colorTexMatrix0Coord =(metadataEntryCoord + vec2(0.5, 0.5))* textureMetadataScale;
    vec2 colorTexOffsetsCoord =(metadataEntryCoord + vec2(1.5, 0.5))* textureMetadataScale;
    vec2 baseColorCoord =(metadataEntryCoord + vec2(2.5, 0.5))* textureMetadataScale;
    vec4 colorTexMatrix0 = texture(textureMetadata, colorTexMatrix0Coord);
    vec4 colorTexOffsets = texture(textureMetadata, colorTexOffsetsCoord);
    vec4 baseColor = texture(textureMetadata, baseColorCoord);

    outColorTexCoord0 = mat2(colorTexMatrix0)* position + colorTexOffsets . xy;
    outBaseColor = baseColor;
}


uniform mat4 uTransform;
uniform vec2 uTileSize;
uniform sampler2D uTextureMetadata;
uniform ivec2 uTextureMetadataSize;
uniform sampler2D uZBuffer;
uniform ivec2 uZBufferSize;

in ivec2 aTileOffset;
in ivec2 aTileOrigin;
in uvec4 aMaskTexCoord0;
in ivec2 aCtrlBackdrop;
in int aPathIndex;
in int aColor;

out vec3 vMaskTexCoord0;
out vec2 vColorTexCoord0;
out vec4 vBaseColor;
out float vTileCtrl;

void main(){
    vec2 tileOrigin = vec2(aTileOrigin), tileOffset = vec2(aTileOffset);
    vec2 position =(tileOrigin + tileOffset)* uTileSize;

    ivec4 zValue = ivec4(texture(uZBuffer,(tileOrigin + vec2(0.5))/ vec2(uZBufferSize))* 255.0);
    if(aPathIndex <(zValue . x |(zValue . y << 8)|(zValue . z << 16)|(zValue . w << 24))){
        gl_Position = vec4(0.0);
        return;
    }

    uvec2 maskTileCoord = uvec2(aMaskTexCoord0 . x, aMaskTexCoord0 . y + 256u * aMaskTexCoord0 . z);
    vec2 maskTexCoord0 =(vec2(maskTileCoord)+ tileOffset)* uTileSize;
    if(aCtrlBackdrop . y == 0 && aMaskTexCoord0 . w != 0u){
        gl_Position = vec4(0.0);
        return;
    }

    computeTileVaryings(position,
                        aColor,
                        uTextureMetadata,
                        uTextureMetadataSize,
                        vColorTexCoord0,
                        vBaseColor);

    vTileCtrl = float(aCtrlBackdrop . x);
    vMaskTexCoord0 = vec3(maskTexCoord0, float(aCtrlBackdrop . y));
    gl_Position = uTransform * vec4(position, 0.0, 1.0);
}


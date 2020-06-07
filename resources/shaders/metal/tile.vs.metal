// Automatically generated from files in pathfinder/shaders/. Do not edit!
#pragma clang diagnostic ignored "-Wmissing-prototypes"

#include <metal_stdlib>
#include <simd/simd.h>

using namespace metal;

struct main0_out
{
    float3 vMaskTexCoord0 [[user(locn0)]];
    float2 vColorTexCoord0 [[user(locn1)]];
    float4 vBaseColor [[user(locn2)]];
    float vTileCtrl [[user(locn3)]];
    float4 gl_Position [[position]];
};

struct main0_in
{
    int2 aTileOffset [[attribute(0)]];
    int2 aTileOrigin [[attribute(1)]];
    uint4 aMaskTexCoord0 [[attribute(2)]];
    int2 aCtrlBackdrop [[attribute(3)]];
    int aPathIndex [[attribute(4)]];
    int aColor [[attribute(5)]];
};

static inline __attribute__((always_inline))
void computeTileVaryings(thread const float2& position, thread const int& colorEntry, thread const texture2d<float> textureMetadata, thread const sampler textureMetadataSmplr, thread const int2& textureMetadataSize, thread float2& outColorTexCoord0, thread float4& outBaseColor)
{
    float2 textureMetadataScale = float2(1.0) / float2(textureMetadataSize);
    float2 metadataEntryCoord = float2(float((colorEntry % 128) * 4), float(colorEntry / 128));
    float2 colorTexMatrix0Coord = (metadataEntryCoord + float2(0.5)) * textureMetadataScale;
    float2 colorTexOffsetsCoord = (metadataEntryCoord + float2(1.5, 0.5)) * textureMetadataScale;
    float2 baseColorCoord = (metadataEntryCoord + float2(2.5, 0.5)) * textureMetadataScale;
    float4 colorTexMatrix0 = textureMetadata.sample(textureMetadataSmplr, colorTexMatrix0Coord, level(0.0));
    float4 colorTexOffsets = textureMetadata.sample(textureMetadataSmplr, colorTexOffsetsCoord, level(0.0));
    float4 baseColor = textureMetadata.sample(textureMetadataSmplr, baseColorCoord, level(0.0));
    outColorTexCoord0 = (float2x2(float2(colorTexMatrix0.xy), float2(colorTexMatrix0.zw)) * position) + colorTexOffsets.xy;
    outBaseColor = baseColor;
}

vertex main0_out main0(main0_in in [[stage_in]], constant int2& uZBufferSize [[buffer(1)]], constant int2& uTextureMetadataSize [[buffer(2)]], constant float2& uTileSize [[buffer(0)]], constant float4x4& uTransform [[buffer(3)]], texture2d<float> uZBuffer [[texture(0)]], texture2d<float> uTextureMetadata [[texture(1)]], sampler uZBufferSmplr [[sampler(0)]], sampler uTextureMetadataSmplr [[sampler(1)]])
{
    main0_out out = {};
    float2 tileOrigin = float2(in.aTileOrigin);
    float2 tileOffset = float2(in.aTileOffset);
    float2 position = (tileOrigin + tileOffset) * uTileSize;
    int4 zValue = int4(uZBuffer.sample(uZBufferSmplr, ((tileOrigin + float2(0.5)) / float2(uZBufferSize)), level(0.0)) * 255.0);
    if (in.aPathIndex < (((zValue.x | (zValue.y << 8)) | (zValue.z << 16)) | (zValue.w << 24)))
    {
        out.gl_Position = float4(0.0);
        return out;
    }
    uint2 maskTileCoord = uint2(in.aMaskTexCoord0.x, in.aMaskTexCoord0.y + (256u * in.aMaskTexCoord0.z));
    float2 maskTexCoord0 = (float2(maskTileCoord) + tileOffset) * uTileSize;
    bool _191 = in.aCtrlBackdrop.y == 0;
    bool _197;
    if (_191)
    {
        _197 = in.aMaskTexCoord0.w != 0u;
    }
    else
    {
        _197 = _191;
    }
    if (_197)
    {
        out.gl_Position = float4(0.0);
        return out;
    }
    float2 param = position;
    int param_1 = in.aColor;
    int2 param_2 = uTextureMetadataSize;
    float2 param_3;
    float4 param_4;
    computeTileVaryings(param, param_1, uTextureMetadata, uTextureMetadataSmplr, param_2, param_3, param_4);
    out.vColorTexCoord0 = param_3;
    out.vBaseColor = param_4;
    out.vTileCtrl = float(in.aCtrlBackdrop.x);
    out.vMaskTexCoord0 = float3(maskTexCoord0, float(in.aCtrlBackdrop.y));
    out.gl_Position = uTransform * float4(position, 0.0, 1.0);
    return out;
}


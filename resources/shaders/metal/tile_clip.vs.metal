// Automatically generated from files in pathfinder/shaders/. Do not edit!
#include <metal_stdlib>
#include <simd/simd.h>

using namespace metal;

struct main0_out
{
    float2 vTexCoord [[user(locn0)]];
    float vBackdrop [[user(locn1)]];
    float4 gl_Position [[position]];
};

struct main0_in
{
    int2 aTileOffset [[attribute(0)]];
    int aDestTileIndex [[attribute(1)]];
    int aSrcTileIndex [[attribute(2)]];
    int aSrcBackdrop [[attribute(3)]];
};

vertex main0_out main0(main0_in in [[stage_in]], constant float2& uFramebufferSize [[buffer(0)]])
{
    main0_out out = {};
    float2 destPosition = float2(int2(in.aDestTileIndex % 256, in.aDestTileIndex / 256) + in.aTileOffset);
    float2 srcPosition = float2(int2(in.aSrcTileIndex % 256, in.aSrcTileIndex / 256) + in.aTileOffset);
    destPosition = (destPosition * float2(16.0, 4.0)) / uFramebufferSize;
    srcPosition = (srcPosition * float2(16.0, 4.0)) / uFramebufferSize;
    if (in.aDestTileIndex < 0)
    {
        destPosition = float2(0.0);
    }
    out.vTexCoord = srcPosition;
    out.vBackdrop = float(in.aSrcBackdrop);
    destPosition.y = 1.0 - destPosition.y;
    out.gl_Position = float4(mix(float2(-1.0), float2(1.0), destPosition), 0.0, 1.0);
    return out;
}


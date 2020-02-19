// Automatically generated from files in pathfinder/shaders/. Do not edit!
#include <metal_stdlib>
#include <simd/simd.h>

using namespace metal;

struct spvDescriptorSetBuffer0
{
    texture2d<float> uStencilTexture [[id(0)]];
    sampler uStencilTextureSmplr [[id(1)]];
    texture2d<float> uPaintTexture [[id(2)]];
    sampler uPaintTextureSmplr [[id(3)]];
};

struct main0_out
{
    float4 oFragColor [[color(0)]];
};

struct main0_in
{
    float2 vColorTexCoord [[user(locn0)]];
    float2 vMaskTexCoord [[user(locn1)]];
};

fragment main0_out main0(main0_in in [[stage_in]], constant spvDescriptorSetBuffer0& spvDescriptorSet0 [[buffer(0)]])
{
    main0_out out = {};
    float coverage = spvDescriptorSet0.uStencilTexture.sample(spvDescriptorSet0.uStencilTextureSmplr, in.vMaskTexCoord).x;
    float4 color = spvDescriptorSet0.uPaintTexture.sample(spvDescriptorSet0.uPaintTextureSmplr, in.vColorTexCoord);
    out.oFragColor = float4(color.xyz, color.w * coverage);
    return out;
}


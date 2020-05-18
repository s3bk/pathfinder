// Automatically generated from files in pathfinder/shaders/. Do not edit!
#pragma clang diagnostic ignored "-Wmissing-prototypes"
#pragma clang diagnostic ignored "-Wmissing-braces"
#pragma clang diagnostic ignored "-Wunused-variable"

#include <metal_stdlib>
#include <simd/simd.h>
#include <metal_atomic>

using namespace metal;

template<typename T, size_t Num>
struct spvUnsafeArray
{
    T elements[Num ? Num : 1];
    
    thread T& operator [] (size_t pos) thread
    {
        return elements[pos];
    }
    constexpr const thread T& operator [] (size_t pos) const thread
    {
        return elements[pos];
    }
    
    device T& operator [] (size_t pos) device
    {
        return elements[pos];
    }
    constexpr const device T& operator [] (size_t pos) const device
    {
        return elements[pos];
    }
    
    constexpr const constant T& operator [] (size_t pos) const constant
    {
        return elements[pos];
    }
    
    threadgroup T& operator [] (size_t pos) threadgroup
    {
        return elements[pos];
    }
    constexpr const threadgroup T& operator [] (size_t pos) const threadgroup
    {
        return elements[pos];
    }
};

struct bComputeIndirectParams
{
    uint iComputeIndirectParams[1];
};

struct bMicrolines
{
    uint4 iMicrolines[1];
};

struct bPoints
{
    float2 iPoints[1];
};

struct bDiceMetadata
{
    uint4 iDiceMetadata[1];
};

struct bInputIndices
{
    uint2 iInputIndices[1];
};

constant uint3 gl_WorkGroupSize [[maybe_unused]] = uint3(64u, 1u, 1u);

static inline __attribute__((always_inline))
float2 getPoint(thread const uint& pointIndex, thread float2x2 uTransform, const device bPoints& v_261, thread float2 uTranslation)
{
    return (uTransform * v_261.iPoints[pointIndex]) + uTranslation;
}

static inline __attribute__((always_inline))
void emitMicroline(thread const float4& microline, thread const uint& pathIndex, device bComputeIndirectParams& v_42, thread int uMaxMicrolineCount, device bMicrolines& v_99)
{
    uint _50 = atomic_fetch_add_explicit((device atomic_uint*)&v_42.iComputeIndirectParams[3], 1u, memory_order_relaxed);
    uint outputMicrolineIndex = _50;
    if ((outputMicrolineIndex % 64u) == 0u)
    {
        uint _58 = atomic_fetch_add_explicit((device atomic_uint*)&v_42.iComputeIndirectParams[0], 1u, memory_order_relaxed);
    }
    if (outputMicrolineIndex > uint(uMaxMicrolineCount))
    {
        return;
    }
    int4 microlineSubpixels = int4(round(fast::clamp(microline, float4(-32768.0), float4(32767.0)) * 256.0));
    int4 microlinePixels = int4(floor(float4(microlineSubpixels) / float4(256.0)));
    int4 microlineFractPixels = microlineSubpixels - (microlinePixels * int4(256));
    v_99.iMicrolines[outputMicrolineIndex] = uint4((uint(microlinePixels.x) & 65535u) | (uint(microlinePixels.y) << uint(16)), (uint(microlinePixels.z) & 65535u) | (uint(microlinePixels.w) << uint(16)), ((uint(microlineFractPixels.x) | (uint(microlineFractPixels.y) << uint(8))) | (uint(microlineFractPixels.z) << uint(16))) | (uint(microlineFractPixels.w) << uint(24)), pathIndex);
}

static inline __attribute__((always_inline))
bool curveIsFlat(thread const float4& baseline, thread const float4& ctrl)
{
    float4 uv = ((float4(3.0) * ctrl) - (float4(2.0) * baseline)) - baseline.zwxy;
    uv *= uv;
    uv = fast::max(uv, uv.zwxy);
    return (uv.x + uv.y) <= 1.0;
}

static inline __attribute__((always_inline))
void subdivideCurve(thread const float4& baseline, thread const float4& ctrl, thread const float& t, thread float4& prevBaseline, thread float4& prevCtrl, thread float4& nextBaseline, thread float4& nextCtrl)
{
    float2 p0 = baseline.xy;
    float2 p1 = ctrl.xy;
    float2 p2 = ctrl.zw;
    float2 p3 = baseline.zw;
    float2 p0p1 = mix(p0, p1, float2(t));
    float2 p1p2 = mix(p1, p2, float2(t));
    float2 p2p3 = mix(p2, p3, float2(t));
    float2 p0p1p2 = mix(p0p1, p1p2, float2(t));
    float2 p1p2p3 = mix(p1p2, p2p3, float2(t));
    float2 p0p1p2p3 = mix(p0p1p2, p1p2p3, float2(t));
    prevBaseline = float4(p0, p0p1p2p3);
    prevCtrl = float4(p0p1, p0p1p2);
    nextBaseline = float4(p0p1p2p3, p3);
    nextCtrl = float4(p1p2p3, p2p3);
}

kernel void main0(constant int& uMaxMicrolineCount [[buffer(1)]], constant int& uLastBatchSegmentIndex [[buffer(6)]], constant int& uPathCount [[buffer(7)]], constant float2x2& uTransform [[buffer(3)]], constant float2& uTranslation [[buffer(5)]], device bComputeIndirectParams& v_42 [[buffer(0)]], device bMicrolines& v_99 [[buffer(2)]], const device bPoints& v_261 [[buffer(4)]], const device bDiceMetadata& _320 [[buffer(8)]], const device bInputIndices& _366 [[buffer(9)]], uint3 gl_GlobalInvocationID [[thread_position_in_grid]])
{
    uint batchSegmentIndex = gl_GlobalInvocationID.x;
    if (batchSegmentIndex >= uint(uLastBatchSegmentIndex))
    {
        return;
    }
    uint lowPathIndex = 0u;
    uint highPathIndex = uint(uPathCount);
    int iteration = 0;
    for (;;)
    {
        bool _301 = iteration < 1024;
        bool _308;
        if (_301)
        {
            _308 = (lowPathIndex + 1u) < highPathIndex;
        }
        else
        {
            _308 = _301;
        }
        if (_308)
        {
            uint midPathIndex = lowPathIndex + ((highPathIndex - lowPathIndex) / 2u);
            uint midBatchSegmentIndex = _320.iDiceMetadata[midPathIndex].z;
            if (batchSegmentIndex < midBatchSegmentIndex)
            {
                highPathIndex = midPathIndex;
            }
            else
            {
                lowPathIndex = midPathIndex;
                if (batchSegmentIndex == midBatchSegmentIndex)
                {
                    break;
                }
            }
            iteration++;
            continue;
        }
        else
        {
            break;
        }
    }
    uint batchPathIndex = lowPathIndex;
    uint4 diceMetadata = _320.iDiceMetadata[batchPathIndex];
    uint firstGlobalSegmentIndexInPath = diceMetadata.y;
    uint firstBatchSegmentIndexInPath = diceMetadata.z;
    uint globalSegmentIndex = (batchSegmentIndex - firstBatchSegmentIndexInPath) + firstGlobalSegmentIndexInPath;
    uint2 inputIndices = _366.iInputIndices[globalSegmentIndex];
    uint fromPointIndex = inputIndices.x;
    uint flagsPathIndex = inputIndices.y;
    uint toPointIndex = fromPointIndex;
    if ((flagsPathIndex & 1073741824u) != 0u)
    {
        toPointIndex += 3u;
    }
    else
    {
        if ((flagsPathIndex & 2147483648u) != 0u)
        {
            toPointIndex += 2u;
        }
        else
        {
            toPointIndex++;
        }
    }
    uint param = fromPointIndex;
    uint param_1 = toPointIndex;
    float4 baseline = float4(getPoint(param, uTransform, v_261, uTranslation), getPoint(param_1, uTransform, v_261, uTranslation));
    if ((flagsPathIndex & 3221225472u) == 0u)
    {
        float4 param_2 = baseline;
        uint param_3 = batchPathIndex;
        emitMicroline(param_2, param_3, v_42, uMaxMicrolineCount, v_99);
        return;
    }
    uint param_4 = fromPointIndex + 1u;
    float2 ctrl0 = getPoint(param_4, uTransform, v_261, uTranslation);
    float4 ctrl;
    if ((flagsPathIndex & 2147483648u) != 0u)
    {
        float2 ctrl0_2 = ctrl0 * float2(2.0);
        ctrl = (baseline + (ctrl0 * float2(2.0)).xyxy) * float4(0.3333333432674407958984375);
    }
    else
    {
        uint param_5 = fromPointIndex + 2u;
        ctrl = float4(ctrl0, getPoint(param_5, uTransform, v_261, uTranslation));
    }
    int curveStackSize = 1;
    spvUnsafeArray<float4, 32> baselines;
    baselines[0] = baseline;
    spvUnsafeArray<float4, 32> ctrls;
    ctrls[0] = ctrl;
    float4 param_13;
    float4 param_14;
    float4 param_15;
    float4 param_16;
    while (curveStackSize > 0)
    {
        curveStackSize--;
        baseline = baselines[curveStackSize];
        ctrl = ctrls[curveStackSize];
        float4 param_6 = baseline;
        float4 param_7 = ctrl;
        bool _486 = curveIsFlat(param_6, param_7);
        bool _495;
        if (!_486)
        {
            _495 = (curveStackSize + 2) >= 32;
        }
        else
        {
            _495 = _486;
        }
        if (_495)
        {
            float4 param_8 = baseline;
            uint param_9 = batchPathIndex;
            emitMicroline(param_8, param_9, v_42, uMaxMicrolineCount, v_99);
        }
        else
        {
            float4 param_10 = baseline;
            float4 param_11 = ctrl;
            float param_12 = 0.5;
            subdivideCurve(param_10, param_11, param_12, param_13, param_14, param_15, param_16);
            baselines[curveStackSize + 1] = param_13;
            ctrls[curveStackSize + 1] = param_14;
            baselines[curveStackSize + 0] = param_15;
            ctrls[curveStackSize + 0] = param_16;
            curveStackSize += 2;
        }
    }
}


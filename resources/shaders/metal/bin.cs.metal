// Automatically generated from files in pathfinder/shaders/. Do not edit!
#pragma clang diagnostic ignored "-Wmissing-prototypes"
#pragma clang diagnostic ignored "-Wunused-variable"

#include <metal_stdlib>
#include <simd/simd.h>
#include <metal_atomic>

using namespace metal;

struct bTiles
{
    uint iTiles[1];
};

struct bIndirectDrawParams
{
    uint iIndirectDrawParams[1];
};

struct bFillTileMap
{
    uint iFillTileMap[1];
};

struct bFills
{
    uint iFills[1];
};

struct bBackdrops
{
    uint iBackdrops[1];
};

struct bMicrolines
{
    uint4 iMicrolines[1];
};

struct bMetadata
{
    int4 iMetadata[1];
};

constant uint3 gl_WorkGroupSize [[maybe_unused]] = uint3(64u, 1u, 1u);

static inline __attribute__((always_inline))
float4 unpackMicroline(thread const uint4& packedMicroline, thread uint& outPathIndex)
{
    outPathIndex = packedMicroline.w;
    int4 signedMicroline = int4(packedMicroline);
    return float4(float((signedMicroline.x << 16) >> 16), float(signedMicroline.x >> 16), float((signedMicroline.y << 16) >> 16), float(signedMicroline.y >> 16)) + (float4(float(signedMicroline.z & 255), float((signedMicroline.z >> 8) & 255), float((signedMicroline.z >> 16) & 255), float((signedMicroline.z >> 24) & 255)) / float4(256.0));
}

static inline __attribute__((always_inline))
uint computeTileIndexNoCheck(thread const int2& tileCoords, thread const int4& pathTileRect, thread const uint& pathTileOffset)
{
    int2 offsetCoords = tileCoords - pathTileRect.xy;
    return (pathTileOffset + uint(offsetCoords.x)) + uint(offsetCoords.y * (pathTileRect.z - pathTileRect.x));
}

static inline __attribute__((always_inline))
bool4 computeTileOutcodes(thread const int2& tileCoords, thread const int4& pathTileRect)
{
    return bool4(tileCoords < pathTileRect.xy, tileCoords >= pathTileRect.zw);
}

static inline __attribute__((always_inline))
bool computeTileIndex(thread const int2& tileCoords, thread const int4& pathTileRect, thread const uint& pathTileOffset, thread uint& outTileIndex)
{
    int2 param = tileCoords;
    int4 param_1 = pathTileRect;
    uint param_2 = pathTileOffset;
    outTileIndex = computeTileIndexNoCheck(param, param_1, param_2);
    int2 param_3 = tileCoords;
    int4 param_4 = pathTileRect;
    return !any(computeTileOutcodes(param_3, param_4));
}

static inline __attribute__((always_inline))
void addFill(thread const float4& lineSegment, thread const int2& tileCoords, thread const int4& pathTileRect, thread const uint& pathTileOffset, device bTiles& v_154, device bIndirectDrawParams& v_174, device bFillTileMap& v_187, thread int uMaxFillCount, device bFills& v_203)
{
    int2 param = tileCoords;
    int4 param_1 = pathTileRect;
    uint param_2 = pathTileOffset;
    uint param_3;
    bool _124 = computeTileIndex(param, param_1, param_2, param_3);
    uint tileIndex = param_3;
    if (!_124)
    {
        return;
    }
    uint4 scaledLocalLine = uint4((lineSegment - float4(tileCoords.xyxy * int4(16))) * float4(256.0));
    if (scaledLocalLine.x == scaledLocalLine.z)
    {
        return;
    }
    uint _163;
    do
    {
        _163 = 4294967295u;
    } while (!atomic_compare_exchange_weak_explicit((device atomic_uint*)&v_154.iTiles[(tileIndex * 4u) + 1u], &_163, 0u, memory_order_relaxed, memory_order_relaxed) && _163 == 4294967295u);
    if (_163 == 4294967295u)
    {
        uint _177 = atomic_fetch_add_explicit((device atomic_uint*)&v_174.iIndirectDrawParams[4], 1u, memory_order_relaxed);
        uint _178 = atomic_exchange_explicit((device atomic_uint*)&v_154.iTiles[(tileIndex * 4u) + 1u], _177, memory_order_relaxed);
    }
    uint _182 = atomic_fetch_add_explicit((device atomic_uint*)&v_174.iIndirectDrawParams[1], 1u, memory_order_relaxed);
    uint fillIndex = _182;
    uint _191 = atomic_exchange_explicit((device atomic_uint*)&v_187.iFillTileMap[tileIndex], fillIndex, memory_order_relaxed);
    uint fillLink = _191;
    if (fillIndex < uint(uMaxFillCount))
    {
        v_203.iFills[(fillIndex * 3u) + 0u] = scaledLocalLine.x | (scaledLocalLine.y << uint(16));
        v_203.iFills[(fillIndex * 3u) + 1u] = scaledLocalLine.z | (scaledLocalLine.w << uint(16));
        v_203.iFills[(fillIndex * 3u) + 2u] = fillLink;
    }
}

static inline __attribute__((always_inline))
void adjustBackdrop(thread const int& backdropDelta, thread const int2& tileCoords, thread const int4& pathTileRect, thread const uint& pathTileOffset, thread const uint& pathBackdropOffset, device bTiles& v_154, device bBackdrops& v_268)
{
    int2 param = tileCoords;
    int4 param_1 = pathTileRect;
    bool4 outcodes = computeTileOutcodes(param, param_1);
    if (any(outcodes))
    {
        bool _247 = (!outcodes.x) && outcodes.y;
        bool _253;
        if (_247)
        {
            _253 = !outcodes.z;
        }
        else
        {
            _253 = _247;
        }
        if (_253)
        {
            uint backdropIndex = pathBackdropOffset + uint(tileCoords.x - pathTileRect.x);
            uint _274 = atomic_fetch_add_explicit((device atomic_uint*)&v_268.iBackdrops[backdropIndex * 3u], uint(backdropDelta), memory_order_relaxed);
        }
    }
    else
    {
        int2 param_2 = tileCoords;
        int4 param_3 = pathTileRect;
        uint param_4 = pathTileOffset;
        uint tileIndex = computeTileIndexNoCheck(param_2, param_3, param_4);
        uint _292 = atomic_fetch_add_explicit((device atomic_uint*)&v_154.iTiles[(tileIndex * 4u) + 3u], uint(backdropDelta << 24), memory_order_relaxed);
    }
}

kernel void main0(constant int& uMaxFillCount [[buffer(3)]], constant int& uMicrolineCount [[buffer(6)]], device bTiles& v_154 [[buffer(0)]], device bIndirectDrawParams& v_174 [[buffer(1)]], device bFillTileMap& v_187 [[buffer(2)]], device bFills& v_203 [[buffer(4)]], device bBackdrops& v_268 [[buffer(5)]], const device bMicrolines& _363 [[buffer(7)]], const device bMetadata& _377 [[buffer(8)]], uint3 gl_GlobalInvocationID [[thread_position_in_grid]])
{
    uint segmentIndex = gl_GlobalInvocationID.x;
    if (segmentIndex >= uint(uMicrolineCount))
    {
        return;
    }
    uint4 param = _363.iMicrolines[segmentIndex];
    uint param_1;
    float4 _371 = unpackMicroline(param, param_1);
    uint pathIndex = param_1;
    float4 lineSegment = _371;
    int4 pathTileRect = _377.iMetadata[(pathIndex * 3u) + 0u];
    uint pathTileOffset = uint(_377.iMetadata[(pathIndex * 3u) + 1u].x);
    uint pathBackdropOffset = uint(_377.iMetadata[(pathIndex * 3u) + 2u].x);
    int2 tileSize = int2(16);
    int4 tileLineSegment = int4(floor(lineSegment / float4(tileSize.xyxy)));
    int2 fromTileCoords = tileLineSegment.xy;
    int2 toTileCoords = tileLineSegment.zw;
    float2 vector = lineSegment.zw - lineSegment.xy;
    float2 vectorIsNegative = float2((vector.x < 0.0) ? (-1.0) : 0.0, (vector.y < 0.0) ? (-1.0) : 0.0);
    int2 tileStep = int2((vector.x < 0.0) ? (-1) : 1, (vector.y < 0.0) ? (-1) : 1);
    float2 firstTileCrossing = float2((fromTileCoords + int2(int(vector.x >= 0.0), int(vector.y >= 0.0))) * tileSize);
    float2 tMax = (firstTileCrossing - lineSegment.xy) / vector;
    float2 tDelta = abs(float2(tileSize) / vector);
    float2 currentPosition = lineSegment.xy;
    int2 tileCoords = fromTileCoords;
    int lastStepDirection = 0;
    uint iteration = 0u;
    int nextStepDirection;
    float _518;
    float4 auxiliarySegment;
    while (iteration < 1024u)
    {
        if (tMax.x < tMax.y)
        {
            nextStepDirection = 1;
        }
        else
        {
            if (tMax.x > tMax.y)
            {
                nextStepDirection = 2;
            }
            else
            {
                if (float(tileStep.x) > 0.0)
                {
                    nextStepDirection = 1;
                }
                else
                {
                    nextStepDirection = 2;
                }
            }
        }
        if (nextStepDirection == 1)
        {
            _518 = tMax.x;
        }
        else
        {
            _518 = tMax.y;
        }
        float nextT = fast::min(_518, 1.0);
        if (all(tileCoords == toTileCoords))
        {
            nextStepDirection = 0;
        }
        float2 nextPosition = mix(lineSegment.xy, lineSegment.zw, float2(nextT));
        float4 clippedLineSegment = float4(currentPosition, nextPosition);
        float4 param_2 = clippedLineSegment;
        int2 param_3 = tileCoords;
        int4 param_4 = pathTileRect;
        uint param_5 = pathTileOffset;
        addFill(param_2, param_3, param_4, param_5, v_154, v_174, v_187, uMaxFillCount, v_203);
        bool haveAuxiliarySegment = false;
        if ((tileStep.y < 0) && (nextStepDirection == 2))
        {
            auxiliarySegment = float4(clippedLineSegment.zw, float2(tileCoords * tileSize));
            haveAuxiliarySegment = true;
        }
        else
        {
            if ((tileStep.y > 0) && (lastStepDirection == 2))
            {
                auxiliarySegment = float4(float2(tileCoords * tileSize), clippedLineSegment.xy);
                haveAuxiliarySegment = true;
            }
        }
        if (haveAuxiliarySegment)
        {
            float4 param_6 = auxiliarySegment;
            int2 param_7 = tileCoords;
            int4 param_8 = pathTileRect;
            uint param_9 = pathTileOffset;
            addFill(param_6, param_7, param_8, param_9, v_154, v_174, v_187, uMaxFillCount, v_203);
        }
        if ((tileStep.x < 0) && (lastStepDirection == 1))
        {
            int param_10 = 1;
            int2 param_11 = tileCoords;
            int4 param_12 = pathTileRect;
            uint param_13 = pathTileOffset;
            uint param_14 = pathBackdropOffset;
            adjustBackdrop(param_10, param_11, param_12, param_13, param_14, v_154, v_268);
        }
        else
        {
            if ((tileStep.x > 0) && (nextStepDirection == 1))
            {
                int param_15 = -1;
                int2 param_16 = tileCoords;
                int4 param_17 = pathTileRect;
                uint param_18 = pathTileOffset;
                uint param_19 = pathBackdropOffset;
                adjustBackdrop(param_15, param_16, param_17, param_18, param_19, v_154, v_268);
            }
        }
        if (nextStepDirection == 1)
        {
            tMax.x += tDelta.x;
            tileCoords.x += tileStep.x;
        }
        else
        {
            if (nextStepDirection == 2)
            {
                tMax.y += tDelta.y;
                tileCoords.y += tileStep.y;
            }
            else
            {
                if (nextStepDirection == 0)
                {
                    break;
                }
            }
        }
        currentPosition = nextPosition;
        lastStepDirection = nextStepDirection;
        iteration++;
    }
}


// Automatically generated from files in pathfinder/shaders/. Do not edit!
#pragma clang diagnostic ignored "-Wmissing-prototypes"

#include <metal_stdlib>
#include <simd/simd.h>

using namespace metal;

struct bFirstTileMap
{
    int iFirstTileMap[1];
};

struct bTileLinkMap
{
    int iTileLinkMap[1];
};

constant uint3 gl_WorkGroupSize [[maybe_unused]] = uint3(64u, 1u, 1u);

static inline __attribute__((always_inline))
int getFirst(thread const uint& globalTileIndex, device bFirstTileMap& v_26)
{
    return v_26.iFirstTileMap[globalTileIndex];
}

static inline __attribute__((always_inline))
int getNext(thread const int& tileIndex, device bTileLinkMap& v_37)
{
    return v_37.iTileLinkMap[(tileIndex * 2) + 1];
}

static inline __attribute__((always_inline))
void setNext(thread const int& tileIndex, thread const int& newNextTileIndex, device bTileLinkMap& v_37)
{
    v_37.iTileLinkMap[(tileIndex * 2) + 1] = newNextTileIndex;
}

kernel void main0(constant int& uTileCount [[buffer(2)]], device bFirstTileMap& v_26 [[buffer(0)]], device bTileLinkMap& v_37 [[buffer(1)]], uint3 gl_GlobalInvocationID [[thread_position_in_grid]])
{
    uint globalTileIndex = gl_GlobalInvocationID.x;
    if (globalTileIndex >= uint(uTileCount))
    {
        return;
    }
    uint param = globalTileIndex;
    int unsortedFirstTileIndex = getFirst(param, v_26);
    int sortedFirstTileIndex = -1;
    while (unsortedFirstTileIndex >= 0)
    {
        int currentTileIndex = unsortedFirstTileIndex;
        int param_1 = currentTileIndex;
        unsortedFirstTileIndex = getNext(param_1, v_37);
        int prevTrialTileIndex = -1;
        int trialTileIndex = sortedFirstTileIndex;
        while (true)
        {
            if ((trialTileIndex < 0) || (currentTileIndex < trialTileIndex))
            {
                if (prevTrialTileIndex < 0)
                {
                    int param_2 = currentTileIndex;
                    int param_3 = sortedFirstTileIndex;
                    setNext(param_2, param_3, v_37);
                    sortedFirstTileIndex = currentTileIndex;
                }
                else
                {
                    int param_4 = currentTileIndex;
                    int param_5 = trialTileIndex;
                    setNext(param_4, param_5, v_37);
                    int param_6 = prevTrialTileIndex;
                    int param_7 = currentTileIndex;
                    setNext(param_6, param_7, v_37);
                }
                break;
            }
            prevTrialTileIndex = trialTileIndex;
            int param_8 = trialTileIndex;
            trialTileIndex = getNext(param_8, v_37);
        }
    }
    v_26.iFirstTileMap[globalTileIndex] = sortedFirstTileIndex;
}


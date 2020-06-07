#version {{version}}
// Automatically generated from files in pathfinder/shaders/. Do not edit!














#extension GL_GOOGLE_include_directive : enable

precision highp float;





layout(local_size_x = 64)in;

uniform int uPathCount;
uniform int uTileCount;

layout(std430, binding = 0)buffer bTilePathInfo {




    restrict readonly uvec4 iTilePathInfo[];
};

layout(std430, binding = 1)buffer bTiles {




    restrict uvec4 iTiles[];
};

layout(std430, binding = 2)buffer bTileLinkMap {


    restrict int iTileLinkMap[];
};

void main(){
    uint tileIndex = gl_GlobalInvocationID . x;
    if(tileIndex >= uint(uTileCount))
        return;

    uint lowPathIndex = 0, highPathIndex = uint(uPathCount);
    int iteration = 0;
    while(iteration < 1024 && lowPathIndex + 1 < highPathIndex){
        uint midPathIndex = lowPathIndex +(highPathIndex - lowPathIndex)/ 2;
        uint midTileIndex = iTilePathInfo[midPathIndex]. z;
        if(tileIndex < midTileIndex){
            highPathIndex = midPathIndex;
        } else {
            lowPathIndex = midPathIndex;
            if(tileIndex == midTileIndex)
                break;
        }
        iteration ++;
    }

    uint pathIndex = lowPathIndex;
    uvec4 pathInfo = iTilePathInfo[pathIndex];

    ivec2 packedTileRect = ivec2(pathInfo . xy);
    ivec4 tileRect = ivec4((packedTileRect . x << 16)>> 16, packedTileRect . x >> 16,
                           (packedTileRect . y << 16)>> 16, packedTileRect . y >> 16);

    uint tileOffset = tileIndex - pathInfo . z;
    uint tileWidth = uint(tileRect . z - tileRect . x);
    ivec2 tileCoords = tileRect . xy + ivec2(tileOffset % tileWidth, tileOffset / tileWidth);

    iTiles[tileIndex]= uvec4((uint(tileCoords . x)& 0xffffu)|(uint(tileCoords . y)<< 16),
                              ~ 0u,
                              pathIndex,
                              pathInfo . w);

    iTileLinkMap[tileIndex * 2 + 0]= - 1;
    iTileLinkMap[tileIndex * 2 + 1]= - 1;
}


#version {{version}}
// Automatically generated from files in pathfinder/shaders/. Do not edit!














#extension GL_GOOGLE_include_directive : enable

precision highp float;





layout(local_size_x = 64)in;






uniform ivec2 uFramebufferTileSize;
uniform int uColumnCount;

layout(std430, binding = 0)buffer bDrawMetadata {






    restrict readonly uvec4 iDrawMetadata[];
};

layout(std430, binding = 1)buffer bClipMetadata {





    restrict readonly uvec4 iClipMetadata[];
};

layout(std430, binding = 2)buffer bBackdrops {



    restrict readonly int iBackdrops[];
};

layout(std430, binding = 3)buffer bDrawTiles {




    restrict uint iDrawTiles[];
};

layout(std430, binding = 4)buffer bClipTiles {




    restrict uint iClipTiles[];
};

layout(std430, binding = 5)buffer bClipVertexBuffer {
    restrict ivec4 iClipVertexBuffer[];
};

layout(std430, binding = 6)buffer bZBuffer {
    restrict int iZBuffer[];
};

layout(std430, binding = 7)buffer bFirstTileMap {
    restrict int iFirstTileMap[];
};

layout(std430, binding = 8)buffer bIndirectDrawParams {





    restrict uint iIndirectDrawParams[];
};

layout(std430, binding = 9)buffer bAlphaTileIndices {

    restrict uint iAlphaTileIndices[];
};

uint calculateTileIndex(uint bufferOffset, uvec4 tileRect, uvec2 tileCoord){
    return bufferOffset + tileCoord . y *(tileRect . z - tileRect . x)+ tileCoord . x;
}

void main(){
    uint columnIndex = gl_GlobalInvocationID . x;
    if(int(columnIndex)>= uColumnCount)
        return;

    int currentBackdrop = iBackdrops[columnIndex * 3 + 0];
    int tileX = iBackdrops[columnIndex * 3 + 1];
    uint drawPathIndex = uint(iBackdrops[columnIndex * 3 + 2]);

    uvec4 drawTileRect = iDrawMetadata[drawPathIndex * 3 + 0];
    uvec4 drawOffsets = iDrawMetadata[drawPathIndex * 3 + 1];
    uvec2 drawTileSize = drawTileRect . zw - drawTileRect . xy;
    uint drawTileBufferOffset = drawOffsets . x;
    bool zWrite = drawOffsets . z != 0;

    int clipPathIndex = int(drawOffsets . w);
    uvec4 clipTileRect = uvec4(0u), clipOffsets = uvec4(0u);
    if(clipPathIndex >= 0){
        clipTileRect = iClipMetadata[clipPathIndex * 2 + 0];
        clipOffsets = iClipMetadata[clipPathIndex * 2 + 1];
    }
    uint clipTileBufferOffset = clipOffsets . x, clipBackdropOffset = clipOffsets . y;

    for(uint tileY = 0;tileY < drawTileSize . y;tileY ++){
        uvec2 drawTileCoord = uvec2(tileX, tileY);
        uint drawTileIndex = calculateTileIndex(drawTileBufferOffset, drawTileRect, drawTileCoord);

        int drawAlphaTileIndex = - 1;
        int drawFirstFillIndex = int(iDrawTiles[drawTileIndex * 4 + 1]);
        int drawBackdropDelta =
            int(iDrawTiles[drawTileIndex * 4 + 2])>> 24;
        uint drawTileWord = iDrawTiles[drawTileIndex * 4 + 3];

        int drawTileBackdrop = currentBackdrop;



        if(drawFirstFillIndex >= 0){
            drawAlphaTileIndex = int(atomicAdd(iIndirectDrawParams[4], 1));
            iAlphaTileIndices[drawAlphaTileIndex]= drawTileIndex;
        }


        if(clipPathIndex >= 0){
            uvec2 tileCoord = drawTileCoord + drawTileRect . xy;
            ivec4 clipTileData = ivec4(- 1, 0, - 1, 0);
            if(all(bvec4(greaterThanEqual(tileCoord, clipTileRect . xy),
                          lessThan(tileCoord, clipTileRect . zw)))){
                uvec2 clipTileCoord = tileCoord - clipTileRect . xy;
                uint clipTileIndex = calculateTileIndex(clipTileBufferOffset,
                                                        clipTileRect,
                                                        clipTileCoord);

                int clipAlphaTileIndex = int(iClipTiles[clipTileIndex * 4 + 1]);
                uint clipTileWord = iClipTiles[clipTileIndex * 4 + 3];
                int clipTileBackdrop = int(clipTileWord)>> 24;

                if(clipAlphaTileIndex >= 0 && drawAlphaTileIndex >= 0){




                    clipTileData = ivec4(drawAlphaTileIndex,
                                         drawTileBackdrop,
                                         clipAlphaTileIndex,
                                         clipTileBackdrop);
                    drawTileBackdrop = 0;
                } else if(clipAlphaTileIndex >= 0 &&
                           drawAlphaTileIndex < 0 &&
                           drawTileBackdrop != 0){


                    drawAlphaTileIndex = clipAlphaTileIndex;
                    drawTileBackdrop = clipTileBackdrop;
                } else if(clipAlphaTileIndex < 0 && clipTileBackdrop == 0){

                    drawAlphaTileIndex = - 1;
                    drawTileBackdrop = 0;
                }
            } else {

                drawAlphaTileIndex = - 1;
                drawTileBackdrop = 0;
            }

            iClipVertexBuffer[drawTileIndex]= clipTileData;
        }

        iDrawTiles[drawTileIndex * 4 + 2]=
            (uint(drawAlphaTileIndex)& 0x00ffffffu)|(uint(drawBackdropDelta)<< 24);
        iDrawTiles[drawTileIndex * 4 + 3]=
            (drawTileWord & 0x00ffffff)|(uint(drawTileBackdrop)<< 24);


        ivec2 tileCoord = ivec2(tileX, tileY)+ ivec2(drawTileRect . xy);
        int tileMapIndex = tileCoord . y * uFramebufferTileSize . x + tileCoord . x;
        if(zWrite && drawTileBackdrop != 0 && drawAlphaTileIndex < 0)
            atomicMax(iZBuffer[tileMapIndex], int(drawPathIndex));


        if(drawTileBackdrop != 0 || drawAlphaTileIndex >= 0){
            int nextTileIndex = atomicExchange(iFirstTileMap[tileMapIndex], int(drawTileIndex));
            iDrawTiles[drawTileIndex * 4 + 0]= nextTileIndex;
        }

        currentBackdrop += drawBackdropDelta;
    }
}


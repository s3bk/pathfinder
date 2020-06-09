#version {{version}}
// Automatically generated from files in pathfinder/shaders/. Do not edit!












#extension GL_GOOGLE_include_directive : enable

precision highp float;










uniform int uTileCount;

layout(std430, binding = 0)buffer bTiles {




    restrict uint iTiles[];
};

layout(std430, binding = 1)buffer bFirstTileMap {
    restrict int iFirstTileMap[];
};

layout(local_size_x = 64)in;

int getFirst(uint globalTileIndex){
    return iFirstTileMap[globalTileIndex];
}

int getNextTile(int tileIndex){
    return int(iTiles[tileIndex * 4 + 0]);
}

void setNextTile(int tileIndex, int newNextTileIndex){
    iTiles[tileIndex * 4 + 0]= uint(newNextTileIndex);
}

void main(){
    uint globalTileIndex = gl_GlobalInvocationID . x;
    if(globalTileIndex >= uint(uTileCount))
        return;

    int unsortedFirstTileIndex = getFirst(globalTileIndex);
    int sortedFirstTileIndex = - 1;

    while(unsortedFirstTileIndex >= 0){
        int currentTileIndex = unsortedFirstTileIndex;
        unsortedFirstTileIndex = getNextTile(currentTileIndex);

        int prevTrialTileIndex = - 1;
        int trialTileIndex = sortedFirstTileIndex;
        while(true){
            if(trialTileIndex < 0 || currentTileIndex < trialTileIndex){
                if(prevTrialTileIndex < 0){
                    setNextTile(currentTileIndex, sortedFirstTileIndex);
                    sortedFirstTileIndex = currentTileIndex;
                } else {
                    setNextTile(currentTileIndex, trialTileIndex);
                    setNextTile(prevTrialTileIndex, currentTileIndex);
                }
                break;
            }
            prevTrialTileIndex = trialTileIndex;
            trialTileIndex = getNextTile(trialTileIndex);
        }
    }

    iFirstTileMap[globalTileIndex]= sortedFirstTileIndex;
}


#version {{version}}
// Automatically generated from files in pathfinder/shaders/. Do not edit!












#extension GL_GOOGLE_include_directive : enable

precision highp float;





uniform int uTileCount;

layout(std430, binding = 0)buffer bTileLinkMap {


    restrict int iTileLinkMap[];
};

layout(std430, binding = 1)buffer bFirstTileMap {
    restrict int iFirstTileMap[];
};

layout(local_size_x = 64)in;

int getFirst(uint globalTileIndex){
    return iFirstTileMap[globalTileIndex];
}

int getNext(int tileIndex){
    return iTileLinkMap[tileIndex * 2 + 1];
}

void setNext(int tileIndex, int newNextTileIndex){
    iTileLinkMap[tileIndex * 2 + 1]= newNextTileIndex;
}

void main(){
    uint globalTileIndex = gl_GlobalInvocationID . x;
    if(globalTileIndex >= uint(uTileCount))
        return;

    int unsortedFirstTileIndex = getFirst(globalTileIndex);
    int sortedFirstTileIndex = - 1;

    while(unsortedFirstTileIndex >= 0){
        int currentTileIndex = unsortedFirstTileIndex;
        unsortedFirstTileIndex = getNext(currentTileIndex);

        int prevTrialTileIndex = - 1;
        int trialTileIndex = sortedFirstTileIndex;
        while(true){
            if(trialTileIndex < 0 || currentTileIndex < trialTileIndex){
                if(prevTrialTileIndex < 0){
                    setNext(currentTileIndex, sortedFirstTileIndex);
                    sortedFirstTileIndex = currentTileIndex;
                } else {
                    setNext(currentTileIndex, trialTileIndex);
                    setNext(prevTrialTileIndex, currentTileIndex);
                }
                break;
            }
            prevTrialTileIndex = trialTileIndex;
            trialTileIndex = getNext(trialTileIndex);
        }
    }

    iFirstTileMap[globalTileIndex]= sortedFirstTileIndex;
}


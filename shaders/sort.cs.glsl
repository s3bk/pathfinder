#version 430

// pathfinder/shaders/sort.cs.glsl
//
// Copyright © 2020 The Pathfinder Project Developers.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#extension GL_GOOGLE_include_directive : enable

precision highp float;

#ifdef GL_ES
precision highp sampler2D;
#endif

uniform int uTileCount;

layout(std430, binding = 0) buffer bTileLinkMap {
    // [0]: index of first fill in this tile
    // [1]: index of next tile
    restrict int iTileLinkMap[];
};

layout(std430, binding = 1) buffer bFirstTileMap {
    restrict int iFirstTileMap[];
};

layout(local_size_x = 64) in;

int getFirst(uint globalTileIndex) {
    return iFirstTileMap[globalTileIndex];
}

int getNext(int tileIndex) {
    return iTileLinkMap[tileIndex * 2 + 1];
}

void setNext(int tileIndex, int newNextTileIndex) {
    iTileLinkMap[tileIndex * 2 + 1] = newNextTileIndex;
}

void main() {
    uint globalTileIndex = gl_GlobalInvocationID.x;
    if (globalTileIndex >= uint(uTileCount))
        return;

    int unsortedFirstTileIndex = getFirst(globalTileIndex);
    int sortedFirstTileIndex = -1;

    while (unsortedFirstTileIndex >= 0) {
        int currentTileIndex = unsortedFirstTileIndex;
        unsortedFirstTileIndex = getNext(currentTileIndex);

        int prevTrialTileIndex = -1;
        int trialTileIndex = sortedFirstTileIndex;
        while (true) {
            if (trialTileIndex < 0 || currentTileIndex < trialTileIndex) {
                if (prevTrialTileIndex < 0) {
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

    iFirstTileMap[globalTileIndex] = sortedFirstTileIndex;
}

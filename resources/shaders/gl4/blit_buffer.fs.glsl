#version {{version}}
// Automatically generated from files in pathfinder/shaders/. Do not edit!












precision highp float;





uniform ivec2 uBufferSize;

layout(std430, binding = 0)buffer bBuffer {
    restrict int iBuffer[];
};

in vec2 vTexCoord;

out vec4 oFragColor;

void main(){
    ivec2 texCoord = ivec2(floor(vTexCoord));
    int value = iBuffer[texCoord . y * uBufferSize . x + texCoord . x];
    oFragColor = vec4(value & 0xff,
                      (value >> 8)& 0xff,
                      (value >> 16)& 0xff,
                      (value >> 24)& 0xff)/ 255.0;
}


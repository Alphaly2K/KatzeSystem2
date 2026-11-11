#include <metal_stdlib>
using namespace metal;

struct FrameVertex {
    float4 position;
    float4 color;
    float2 uv;
    float2 padding;
};

struct RasterVertex {
    float4 position [[position]];
    float4 color;
    float2 uv;
};

vertex RasterVertex frame_vertex(const device FrameVertex* vertices [[buffer(0)]],
                                uint index [[vertex_id]]) {
    FrameVertex input = vertices[index];
    return {input.position, input.color, input.uv};
}

fragment float4 frame_fragment(RasterVertex input [[stage_in]],
                               texture2d<float> image [[texture(0)]],
                               sampler sampling [[sampler(0)]]) {
    return image.sample(sampling, input.uv) * input.color;
}

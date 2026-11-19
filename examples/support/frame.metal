#include <metal_stdlib>
using namespace metal;

struct FrameVertex {
    float4 position;
    float4 color;
    float2 uv;
    float2 padding;
    float4 uv_plane;
    float4 uv_gradient;
};

struct RasterVertex {
    float4 position [[position]];
    float4 color;
    float2 uv;
    float4 uv_plane [[flat]];
    float4 uv_gradient [[flat]];
};

vertex RasterVertex frame_vertex(const device FrameVertex* vertices [[buffer(0)]],
                                uint index [[vertex_id]]) {
    FrameVertex input = vertices[index];
    return {input.position, input.color, input.uv, input.uv_plane, input.uv_gradient};
}

fragment float4 frame_fragment(RasterVertex input [[stage_in]],
                               texture2d<float> image [[texture(0)]],
                               sampler sampling [[sampler(0)]]) {
    float2 uv = input.uv;
    if (input.uv_plane.z != 0.0) {
        float2 point = input.position.xy - 0.5;
        uv = input.uv_plane.xy + input.uv_gradient.xy * point.x
            + input.uv_gradient.zw * point.y;
    }
    float4 color = image.sample(sampling, uv) * input.color;
    if (color.a <= 0.0) {
        discard_fragment();
    }
    return color;
}

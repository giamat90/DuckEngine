// Sprite shader in HLSL. Same interface as the engine's sprite pipeline (render::sprite):
// vertex slot 0 = position float2, uv float2, colour unorm8x4; uniform slot 0 = view-projection;
// texture slot 0 + sampler slot 0.
cbuffer Camera : register(b0, space0) { float4x4 view_proj; };
Texture2D tex : register(t0, space0);
SamplerState samp : register(s0, space0);

struct VsIn { float2 pos : POSITION; float2 uv : TEXCOORD0; float4 color : COLOR0; };
struct VsOut { float4 clip : SV_Position; float2 uv : TEXCOORD0; float4 color : COLOR0; };

VsOut vs_main(VsIn i) {
    VsOut o;
    o.clip = mul(view_proj, float4(i.pos, 0.0, 1.0));
    o.uv = i.uv;
    o.color = i.color;
    return o;
}

float4 ps_main(VsOut i) : SV_Target {
    return tex.Sample(samp, i.uv) * i.color;
}

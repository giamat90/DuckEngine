// Sprite shader in Slang.
struct Camera { float4x4 view_proj; };
[[vk::binding(0, 0)]] ConstantBuffer<Camera> camera;
[[vk::binding(1, 0)]] Texture2D tex;
[[vk::binding(2, 0)]] SamplerState samp;

struct VsIn { float2 pos : POSITION; float2 uv : TEXCOORD0; float4 color : COLOR0; };
struct VsOut { float4 clip : SV_Position; float2 uv : TEXCOORD0; float4 color : COLOR0; };

[shader("vertex")]
VsOut vs_main(VsIn i) {
    VsOut o;
    o.clip = mul(camera.view_proj, float4(i.pos, 0.0, 1.0));
    o.uv = i.uv;
    o.color = i.color;
    return o;
}

[shader("fragment")]
float4 ps_main(VsOut i) : SV_Target {
    return tex.Sample(samp, i.uv) * i.color;
}

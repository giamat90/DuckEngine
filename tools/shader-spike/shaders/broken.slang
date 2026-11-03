[shader("fragment")]
float4 ps_main(float2 uv : TEXCOORD0) : SV_Target {
    float3 c = undefined_name * 2.0;
    return float4(c, 1.0) + uv;
}

#version 450
// Lit mesh fragment shader in GLSL (Vulkan-style bindings). One uniform block, the light block of
// `render::lit` (std140), and the texture: ambient plus Lambert diffuse of one directional light,
// times the texel and the colour of the object.
layout(set = 0, binding = 1) uniform Light {
    vec4 to_light;
    vec4 light_colour;
    vec4 ambient;
};
layout(set = 0, binding = 0) uniform sampler2D tex;
layout(location = 0) in vec3 v_normal;
layout(location = 1) in vec2 v_uv;
layout(location = 2) in vec4 v_colour;
layout(location = 0) out vec4 out_color;
void main() {
    vec3 n = normalize(v_normal);
    float diffuse = max(dot(n, to_light.xyz), 0.0);
    vec3 lit = ambient.rgb + light_colour.rgb * diffuse;
    vec4 base = texture(tex, v_uv) * v_colour;
    out_color = vec4(base.rgb * lit, base.a);
}

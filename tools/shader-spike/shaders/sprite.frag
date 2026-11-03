#version 450
// Sprite fragment shader in GLSL.
layout(set = 0, binding = 0) uniform texture2D tex;
layout(set = 0, binding = 1) uniform sampler samp;
layout(location = 0) in vec2 v_uv;
layout(location = 1) in vec4 v_color;
layout(location = 0) out vec4 out_color;
void main() {
    out_color = texture(sampler2D(tex, samp), v_uv) * v_color;
}

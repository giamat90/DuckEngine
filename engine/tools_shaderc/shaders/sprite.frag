#version 450
// Sprite fragment shader. A combined sampler2D, not a separate texture and sampler: SPIRV-Cross
// drops the bindings of separate textures and samplers when it writes OpenGL GLSL (card 3-11).
layout(set = 0, binding = 0) uniform sampler2D tex;
layout(location = 0) in vec2 v_uv;
layout(location = 1) in vec4 v_color;
layout(location = 0) out vec4 out_color;
void main() {
    out_color = texture(tex, v_uv) * v_color;
}

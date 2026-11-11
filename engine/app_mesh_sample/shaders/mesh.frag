#version 450
// Mesh fragment shader in GLSL (Vulkan-style bindings): the texture, as it is.
layout(set = 0, binding = 0) uniform sampler2D tex;
layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 out_color;
void main() {
    out_color = texture(tex, v_uv);
}

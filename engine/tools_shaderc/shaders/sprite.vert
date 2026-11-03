#version 450
// Sprite vertex shader in GLSL (Vulkan-style bindings).
layout(set = 0, binding = 0) uniform Camera { mat4 view_proj; };
layout(location = 0) in vec2 in_pos;
layout(location = 1) in vec2 in_uv;
layout(location = 2) in vec4 in_color;
layout(location = 0) out vec2 v_uv;
layout(location = 1) out vec4 v_color;
void main() {
    gl_Position = view_proj * vec4(in_pos, 0.0, 1.0);
    v_uv = in_uv;
    v_color = in_color;
}

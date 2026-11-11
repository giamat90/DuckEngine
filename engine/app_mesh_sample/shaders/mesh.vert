#version 450
// Mesh vertex shader in GLSL (Vulkan-style bindings): position, normal (unused), uv.
layout(set = 0, binding = 0) uniform Transform { mat4 mvp; };
layout(location = 0) in vec3 in_pos;
layout(location = 1) in vec3 in_normal;
layout(location = 2) in vec2 in_uv;
layout(location = 0) out vec2 v_uv;
void main() {
    gl_Position = mvp * vec4(in_pos, 1.0);
    v_uv = in_uv;
}

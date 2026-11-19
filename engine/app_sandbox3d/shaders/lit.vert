#version 450
// Lit mesh vertex shader in GLSL (Vulkan-style bindings). One uniform block, the object block of
// `render::lit` (std140): mvp, model, the normal matrix (columns padded to four floats) and the
// colour, which is handed on to the fragment stage.
layout(set = 0, binding = 0) uniform Object {
    mat4 mvp;
    mat4 model;
    mat3 normal_matrix;
    vec4 colour;
};
layout(location = 0) in vec3 in_pos;
layout(location = 1) in vec3 in_normal;
layout(location = 2) in vec2 in_uv;
layout(location = 0) out vec3 v_normal;
layout(location = 1) out vec2 v_uv;
layout(location = 2) out vec4 v_colour;
void main() {
    gl_Position = mvp * vec4(in_pos, 1.0);
    v_normal = normal_matrix * in_normal;
    v_uv = in_uv;
    v_colour = colour;
}

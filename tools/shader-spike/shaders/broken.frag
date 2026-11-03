#version 450
layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 o;
void main() {
    vec3 c = undefined_name * 2.0;
    o = vec4(c, 1.0) + uv;
}

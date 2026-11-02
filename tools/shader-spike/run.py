#!/usr/bin/env python3
"""Shader toolchain spike (card 3-6): compile the same sprite shader with each candidate toolchain.

Dev-time only, not part of the engine, not run by CI. The candidate tools live in the git-ignored
folder target/shader-spike (see wiki/spikes/shader-toolchain-spike.md for how they were obtained
and verified). Run from the repository root:

    python tools/shader-spike/run.py

It prints one markdown table of results: whether each step succeeded, the median wall time of
RUNS runs, and the size of the output. Nothing here writes outside target/shader-spike/out.
"""

import os
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SPIKE = ROOT / "target" / "shader-spike"
OUT = SPIKE / "out"
SHADERS = Path(__file__).resolve().parent / "shaders"
RUNS = 15

DXC = SPIKE / "tools/dxc/bin/x64/dxc.exe"
GLSLANG = SPIKE / "tools/glslang/bin/glslang.exe"
SLANGC = SPIKE / "tools/slang/bin/slangc.exe"
CROSS = SPIKE / "build-spirv-cross/Release/spirv-cross.exe"


def run_once(cmd):
    start = time.perf_counter()
    # Slang loads the DXC library for DXIL output from the PATH.
    env = {**os.environ, "PATH": f"{DXC.parent}{os.pathsep}{os.environ['PATH']}"}
    done = subprocess.run([str(c) for c in cmd], capture_output=True, text=True, env=env)
    return done, (time.perf_counter() - start) * 1000.0


def measure(label, cmd, output=None):
    """Runs `cmd` once to check it, then RUNS times to time it. Returns a table row."""
    first, _ = run_once(cmd)
    if first.returncode != 0:
        message = (first.stderr or first.stdout).strip().splitlines()[:2]
        return f"| {label} | FAIL | - | - | {' / '.join(message)[:90]} |"
    times = [run_once(cmd)[1] for _ in range(RUNS)]
    size = "-"
    if output is not None and Path(output).exists():
        size = f"{Path(output).stat().st_size}"
    return f"| {label} | ok | {statistics.median(times):.0f} | {size} | |"


def main():
    for tool in (DXC, GLSLANG, SLANGC, CROSS):
        if not tool.exists():
            sys.exit(f"missing tool: {tool} (see the spike page for how to obtain it)")
    OUT.mkdir(parents=True, exist_ok=True)
    o = lambda name: OUT / name  # noqa: E731
    hlsl, vert, frag, slang = (SHADERS / n for n in ("sprite.hlsl", "sprite.vert", "sprite.frag", "sprite.slang"))
    rows = []
    rows.append(("HLSL, DXC", [
        ("hlsl -> SPIR-V (vertex)", [DXC, "-T", "vs_6_0", "-E", "vs_main", "-spirv", "-fspv-target-env=vulkan1.1", hlsl, "-Fo", o("h.vert.spv")], o("h.vert.spv")),
        ("hlsl -> SPIR-V (pixel)", [DXC, "-T", "ps_6_0", "-E", "ps_main", "-spirv", "-fspv-target-env=vulkan1.1", hlsl, "-Fo", o("h.frag.spv")], o("h.frag.spv")),
        ("hlsl -> DXIL (vertex)", [DXC, "-T", "vs_6_0", "-E", "vs_main", hlsl, "-Fo", o("h.vert.dxil")], o("h.vert.dxil")),
        ("hlsl -> DXIL (pixel)", [DXC, "-T", "ps_6_0", "-E", "ps_main", hlsl, "-Fo", o("h.frag.dxil")], o("h.frag.dxil")),
        ("SPIR-V -> GLSL 450 (vertex), SPIRV-Cross", [CROSS, o("h.vert.spv"), "--version", "450", "--no-es", "--output", o("h.vert.glsl")], o("h.vert.glsl")),
        ("SPIR-V -> GLSL 450 (pixel), SPIRV-Cross", [CROSS, o("h.frag.spv"), "--version", "450", "--no-es", "--output", o("h.frag.glsl")], o("h.frag.glsl")),
        ("SPIR-V reflection (vertex), SPIRV-Cross", [CROSS, o("h.vert.spv"), "--reflect", "--output", o("h.vert.json")], o("h.vert.json")),
    ]))
    rows.append(("GLSL, glslang + SPIRV-Cross", [
        ("glsl -> SPIR-V (vertex), glslang -V", [GLSLANG, "-V", vert, "-o", o("g.vert.spv")], o("g.vert.spv")),
        ("glsl -> SPIR-V (fragment), glslang -V", [GLSLANG, "-V", frag, "-o", o("g.frag.spv")], o("g.frag.spv")),
        ("glsl -> SPIR-V for OpenGL (vertex), glslang -G", [GLSLANG, "-G", vert, "-o", o("g.vert.gl.spv")], o("g.vert.gl.spv")),
        ("SPIR-V -> GLSL 450 (vertex), SPIRV-Cross", [CROSS, o("g.vert.spv"), "--version", "450", "--no-es", "--output", o("g.vert.out.glsl")], o("g.vert.out.glsl")),
        ("SPIR-V -> HLSL SM 6.0 (vertex), SPIRV-Cross", [CROSS, o("g.vert.spv"), "--hlsl", "--shader-model", "60", "--output", o("g.vert.out.hlsl")], o("g.vert.out.hlsl")),
        ("SPIR-V -> MSL (vertex), SPIRV-Cross", [CROSS, o("g.vert.spv"), "--msl", "--output", o("g.vert.out.msl")], o("g.vert.out.msl")),
        ("generated HLSL -> DXIL (vertex), DXC", [DXC, "-T", "vs_6_0", "-E", "main", o("g.vert.out.hlsl"), "-Fo", o("g.vert.out.dxil")], o("g.vert.out.dxil")),
        ("generated GLSL re-compiled, glslang", [GLSLANG, "-S", "vert", "-V", "--target-env", "vulkan1.1", o("g.vert.out.glsl"), "-o", o("g.vert.re.spv")], o("g.vert.re.spv")),
        ("SPIR-V reflection (vertex), SPIRV-Cross", [CROSS, o("g.vert.spv"), "--reflect", "--output", o("g.vert.json")], o("g.vert.json")),
    ]))
    rows.append(("Slang", [
        ("slang -> SPIR-V (vertex)", [SLANGC, slang, "-target", "spirv", "-entry", "vs_main", "-stage", "vertex", "-o", o("s.vert.spv")], o("s.vert.spv")),
        ("slang -> SPIR-V (fragment)", [SLANGC, slang, "-target", "spirv", "-entry", "ps_main", "-stage", "fragment", "-o", o("s.frag.spv")], o("s.frag.spv")),
        ("slang -> GLSL 450 (vertex)", [SLANGC, slang, "-target", "glsl", "-entry", "vs_main", "-stage", "vertex", "-o", o("s.vert.glsl")], o("s.vert.glsl")),
        ("slang -> HLSL (vertex)", [SLANGC, slang, "-target", "hlsl", "-entry", "vs_main", "-stage", "vertex", "-o", o("s.vert.hlsl")], o("s.vert.hlsl")),
        ("slang -> DXIL (vertex)", [SLANGC, slang, "-target", "dxil", "-entry", "vs_main", "-profile", "vs_6_0", "-o", o("s.vert.dxil")], o("s.vert.dxil")),
        ("slang -> Metal (vertex)", [SLANGC, slang, "-target", "metal", "-entry", "vs_main", "-stage", "vertex", "-o", o("s.vert.metal")], o("s.vert.metal")),
        ("slang reflection JSON", [SLANGC, slang, "-target", "spirv", "-entry", "vs_main", "-stage", "vertex", "-reflection-json", o("s.vert.json"), "-o", o("s.vert2.spv")], o("s.vert.json")),
    ]))
    print(f"median of {RUNS} runs after one checking run; times in ms, sizes in bytes\n")
    for title, steps in rows:
        print(f"### {title}\n\n| step | result | ms | bytes | note |\n|---|---|---|---|---|")
        for label, cmd, output in steps:
            print(measure(label, cmd, output))
        print()


if __name__ == "__main__":
    main()

# Audio Architecture (from scratch) `[P]`

> The phase 5 shape of the ports, the thread model, the mixer, the adapters and the tests (signatures, cards, choices for the owner) is proposed in [audio-port-v0](audio-port-v0.md); the research pass of card 5-1 is its section 1.

> Research caveat: the audio literature search returned only marketing pages. This design rests on general real-time-audio practice **[K]** and needs a second research pass (miniaudio, SoLoud, FMOD/Wwise docs) before refinement.

## 1. Boundaries

- Domain `audio` owns: voices, buses, mixer graph, DSP effects, resampler, spatialisation, streaming/decoding orchestration.
- Ports: `IAudioDevice` (output device with a callback), `IAudioDecoder` (WAV/Ogg/FLAC... decoders as pluggable codecs).
- Adapters: WASAPI (Windows first), ALSA/PipeWire (Linux), CoreAudio (macOS), **Null** (discards), **Offline** (renders to a buffer/file - essential for tests).
- Stopgap allowed: miniaudio as a T1-stopgap device adapter to get sound early; replaced by native adapters later (exit plan in `THIRD_PARTY.md`).

## 2. Pull model and the real-time rule

The device adapter calls the engine: `Render(float* out, frames, format)` on a hard real-time thread. Inside that call: **no allocation, no locks, no I/O, no syscalls that can block, no unbounded loops.**

- Game thread -> audio thread: **lock-free SPSC command queue** (play, stop, set param, attach to bus).
- Audio thread -> game thread: lock-free event queue (voice finished, markers).
- Decoding/streaming happens on I/O workers into ring buffers the mixer only reads.
- The audio thread is **not** a job-system worker.

## 3. Mixer design

```
sources(voices) -> per-voice DSP (pitch/resample, filter, pan/spatial) -> buses -> bus DSP (reverb, EQ, compressor) -> master -> device
```

- Fixed block size processing (e.g. 128-1024 frames), float32 internally, SoA buffers, SIMD where measured.
- The audio thread owns the mixer and the game thread sends it commands; sound data is shared as immutable `Arc` values and handed back through a retire queue so that nothing is freed on the audio thread. This replaces the earlier idea of publishing an immutable graph snapshot (decided in the audio design note, [audio-port-v0](audio-port-v0.md) section 3, choice O1).
- Spatialisation: distance attenuation + panning first; HRTF later (needs data and a convolution engine).
- Format negotiation through the port (sample rate, channels); resampler between engine rate and device rate.

## 4. TDD plan (audio is very testable offline)

| Test | Technique |
|---|---|
| Mixer correctness | Offline adapter renders N blocks; compare against analytic expectations (sine sum, gain, pan law) |
| DSP | impulse/step response, frequency response via FFT within tolerance, resampler aliasing bounds |
| Real-time safety | counting allocator + "forbidden call" detection inside the render callback in tests; stress test with TSan |
| Determinism | same input -> bit-identical output on the same binary |
| Port contract | device adapter: callback cadence, frame counts, format negotiation, underrun reporting |
| Perf | CPU time per N voices per block; budget e.g. "1,000 voices < X% of one core" (X to be set from first benchmark) |

## 5. Scope for v1

Mono/stereo/5.1 output, one-shot and looping voices, streaming music, buses with volume + a few effects, 3D attenuation/pan, WAV decoding from scratch (Ogg Vorbis/Opus decoders are large; treat as `IAudioDecoder` plugins and decide whether to write or adopt later).

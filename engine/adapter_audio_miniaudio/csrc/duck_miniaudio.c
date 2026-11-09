/* The thin C side of the miniaudio adapter: the only file that includes miniaudio.h.
 *
 * Rust sees six opaque functions and never miniaudio's large, version-dependent structs. Without
 * the header (DUCK_HAVE_MINIAUDIO not defined, see build.rs) the same functions exist and report
 * "no device", so the adapter always builds and links.
 *
 * Real-time rule: the data callback calls Rust and nothing else (no allocation, no lock). The
 * callback of a stopped device never runs again once duck_ma_stop has returned. */

#include <stdint.h>
#include <stdlib.h>

#define DUCK_MA_OK 0
#define DUCK_MA_NO_DEVICE 1
#define DUCK_MA_REFUSED 2
#define DUCK_MA_FAILED 3

typedef void (*duck_ma_render)(void *user, float *out, uint32_t frames);

#ifdef DUCK_HAVE_MINIAUDIO

#define MA_NO_DECODING
#define MA_NO_ENCODING
#define MA_NO_GENERATION
#define MA_NO_RESOURCE_MANAGER
#define MA_NO_NODE_GRAPH
#define MA_NO_ENGINE
/* A machine without a sound device must say so, not play into the null backend. */
#define MA_NO_NULL
#define MINIAUDIO_IMPLEMENTATION
#include "miniaudio.h"

typedef struct duck_ma {
    ma_device device;
    duck_ma_render render;
    void *user;
    ma_atomic_bool32 stopping;
    ma_atomic_bool32 lost;
} duck_ma;

static void duck_ma_data(ma_device *device, void *output, const void *input, ma_uint32 frames) {
    duck_ma *self = (duck_ma *)device->pUserData;
    (void)input;
    self->render(self->user, (float *)output, (uint32_t)frames);
}

/* A stop that nobody asked for (unplugged device, service restart) is a lost device. */
static void duck_ma_note(const ma_device_notification *note) {
    duck_ma *self = (duck_ma *)note->pDevice->pUserData;
    if (note->type == ma_device_notification_type_stopped && !ma_atomic_bool32_get(&self->stopping)) {
        ma_atomic_bool32_set(&self->lost, MA_TRUE);
    }
}

static int duck_ma_error_of(ma_result result) {
    switch (result) {
    case MA_FORMAT_NOT_SUPPORTED:
    case MA_INVALID_ARGS:
        return DUCK_MA_REFUSED;
    case MA_NO_DEVICE:
    case MA_NO_BACKEND:
    case MA_FAILED_TO_INIT_BACKEND:
    case MA_DEVICE_TYPE_NOT_SUPPORTED:
        return DUCK_MA_NO_DEVICE;
    default:
        return DUCK_MA_FAILED;
    }
}

int duck_ma_available(void) { return 1; }

void *duck_ma_open(uint32_t sample_rate, uint32_t channels, uint32_t block_frames,
                   duck_ma_render render, void *user, uint32_t *actual_rate, int *error) {
    duck_ma *self = (duck_ma *)calloc(1, sizeof(duck_ma));
    ma_device_config config;
    ma_result result;
    if (self == NULL) {
        *error = DUCK_MA_FAILED;
        return NULL;
    }
    self->render = render;
    self->user = user;
    config = ma_device_config_init(ma_device_type_playback);
    config.playback.format = ma_format_f32;
    config.playback.channels = channels;
    config.sampleRate = sample_rate;
    config.periodSizeInFrames = block_frames;
    config.dataCallback = duck_ma_data;
    config.notificationCallback = duck_ma_note;
    config.pUserData = self;
    result = ma_device_init(NULL, &config, &self->device);
    if (result != MA_SUCCESS) {
        *error = duck_ma_error_of(result);
        free(self);
        return NULL;
    }
    *actual_rate = (uint32_t)self->device.sampleRate;
    *error = DUCK_MA_OK;
    return self;
}

int duck_ma_start(void *handle) {
    duck_ma *self = (duck_ma *)handle;
    ma_atomic_bool32_set(&self->stopping, MA_FALSE);
    return ma_device_start(&self->device) == MA_SUCCESS ? DUCK_MA_OK : DUCK_MA_FAILED;
}

/* When this returns the data callback is not running and will not run again. */
void duck_ma_stop(void *handle) {
    duck_ma *self = (duck_ma *)handle;
    ma_atomic_bool32_set(&self->stopping, MA_TRUE);
    (void)ma_device_stop(&self->device);
}

int duck_ma_lost(void *handle) {
    return ma_atomic_bool32_get(&((duck_ma *)handle)->lost) ? 1 : 0;
}

void duck_ma_close(void *handle) {
    duck_ma *self = (duck_ma *)handle;
    ma_device_uninit(&self->device);
    free(self);
}

#else /* no header: every call says there is no device */

int duck_ma_available(void) { return 0; }

void *duck_ma_open(uint32_t sample_rate, uint32_t channels, uint32_t block_frames,
                   duck_ma_render render, void *user, uint32_t *actual_rate, int *error) {
    (void)sample_rate;
    (void)channels;
    (void)block_frames;
    (void)render;
    (void)user;
    *actual_rate = 0;
    *error = DUCK_MA_NO_DEVICE;
    return NULL;
}

int duck_ma_start(void *handle) {
    (void)handle;
    return DUCK_MA_FAILED;
}

void duck_ma_stop(void *handle) { (void)handle; }

int duck_ma_lost(void *handle) {
    (void)handle;
    return 0;
}

void duck_ma_close(void *handle) { (void)handle; }

#endif

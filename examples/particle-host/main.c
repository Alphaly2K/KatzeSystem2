#include <fcntl.h>
#include <io.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <windows.h>

#define ABI_VERSION 0x00020002u
#define MESH_WORDS 37u
#define MAX_QUADS 262144u

_Static_assert(sizeof(void *) == 4 && sizeof(float) == 4, "Build the particle host for 32-bit Windows");

typedef uint32_t (__cdecl *PluginConstant)(void);
typedef void *(__cdecl *MakeInstance)(void *);
typedef void (__cdecl *ReleaseInstance)(void *);
typedef void (__attribute__((thiscall)) *Start)(void *, const uint32_t *);
typedef void (__attribute__((thiscall)) *Update)(void *, const uint32_t *, uint32_t, uint32_t);
typedef void (__attribute__((thiscall)) *Stop)(void *, const uint32_t *);

struct Instance {
    const FARPROC *vtable;
    void *owner;
    void *services;
    uint32_t *mesh;
};

static float __attribute__((thiscall)) interpolate(void *context, float from, float to, float first, float last, float time) {
    (void)context;
    if (time <= first) return from;
    if (time >= last) return to;
    return (time - first) / (last - first) * (to - from) + from;
}

static void __attribute__((thiscall)) unsupported_callback(void *context) {
    (void)context;
    fputs("Unsupported particle host callback\n", stderr);
    exit(6);
}

static void put_float(uint32_t *out, float value) { memcpy(out, &value, sizeof(value)); }
static float get_float(const uint32_t *in) {
    float value;
    memcpy(&value, in, sizeof(value));
    return value;
}

static int announce(int supported) {
    return fputc(supported, stdout) != EOF && fflush(stdout) == 0;
}

static void reset_mesh(uint32_t *mesh, uint32_t count, float width, float height) {
    memset(mesh, 0, (size_t)count * MESH_WORDS * sizeof(*mesh));
    for (uint32_t i = 0; i < count; ++i) {
        uint32_t *quad = mesh + (size_t)i * MESH_WORDS;
        for (size_t corner = 0; corner < 4; ++corner) {
            float x = (float)(corner & 1), y = (float)(corner >> 1);
            uint32_t *vertex = quad + corner * 6;
            put_float(&vertex[0], x * width);
            put_float(&vertex[1], y * height);
            vertex[3] = UINT32_MAX;
            put_float(&vertex[4], x);
            put_float(&vertex[5], y);
        }
        for (size_t axis = 33; axis < 36; ++axis) put_float(&quad[axis], 1.f);
    }
}

static int run(struct Instance *instance) {
    uint32_t config[MESH_WORDS] = {0};
    uint32_t count = 0;
    Start start = (Start)instance->vtable[1];
    Update update = (Update)instance->vtable[2];
    Stop stop = (Stop)instance->vtable[3];
    int operation;
    while ((operation = getchar()) != EOF && operation != 'Q') {
        if (operation == 'X') {
            if (instance->mesh) stop(instance, config);
            continue;
        }
        /* Elapsed time and delta precede the plugin configuration record. */
        uint32_t packet[2 + MESH_WORDS];
        if ((operation != 'S' && operation != 'A') ||
            fread(packet, sizeof(*packet), 2 + MESH_WORDS, stdin) != 2 + MESH_WORDS ||
            packet[2] > MAX_QUADS) return 4;
        memcpy(config, packet + 2, sizeof(config));
        if (operation == 'S') {
            if (!instance->mesh) {
                count = config[0];
                instance->mesh = calloc(count ? count : 1u, MESH_WORDS * sizeof(*instance->mesh));
                if (!instance->mesh) return 5;
            }
            if (config[0] != count) return 4;
            reset_mesh(instance->mesh, count, get_float(config + 3), get_float(config + 4));
            start(instance, config);
        } else {
            if (!instance->mesh || config[0] != count) return 4;
            update(instance, config, packet[0], packet[0] + packet[1]);
        }
        if (fwrite(instance->mesh, MESH_WORDS * sizeof(*instance->mesh), count, stdout) != count ||
            fflush(stdout) != 0) return 5;
    }
    return ferror(stdin) ? 4 : 0;
}

int wmain(int argc, wchar_t **argv) {
    if (argc != 2) return 1;
    SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
    if (_setmode(_fileno(stdin), _O_BINARY) == -1 || _setmode(_fileno(stdout), _O_BINARY) == -1) return 2;
    HMODULE module = LoadLibraryW(argv[1]);
    if (!module) {
        fprintf(stderr, "LoadLibraryW error=%lu\n", GetLastError());
        return announce(0) ? 0 : 3;
    }
    PluginConstant version = (PluginConstant)GetProcAddress(module, "GetPluginVersion");
    PluginConstant type = (PluginConstant)GetProcAddress(module, "GetPluginType");
    MakeInstance make = (MakeInstance)GetProcAddress(module, "MakeInstance");
    ReleaseInstance release = (ReleaseInstance)GetProcAddress(module, "ReleaseInstance");
    if (!version || !type || !make || !release || type() != 1 || version() != ABI_VERSION) {
        FreeLibrary(module);
        return announce(0) ? 0 : 3;
    }
    FARPROC services[48];
    for (size_t slot = 0; slot < 48; ++slot) services[slot] = (FARPROC)unsupported_callback;
    services[29] = (FARPROC)interpolate;
    FARPROC *services_object = services;
    struct Instance *instance = make(&services_object);
    if (!instance) {
        FreeLibrary(module);
        return announce(0) ? 0 : 3;
    }
    instance->services = &services_object;
    instance->mesh = NULL;
    int result = announce(1) ? run(instance) : 3;
    uint32_t *mesh = instance->mesh;
    release(instance);
    free(mesh);
    FreeLibrary(module);
    return result;
}

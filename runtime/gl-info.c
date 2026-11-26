// gizlet-gl-info: prints the OpenGL ES renderer the runtime's Mesa provides,
// then clears an off-screen texture and reads one pixel back.
//
// With a render node (/dev/dri/renderD*), Mesa drives the GPU through GBM;
// the R36S reports Mali-G31 (Panfrost). Without one, as in QEMU, EGL's
// surfaceless platform falls back to llvmpipe on the CPU. The libraries are
// opened at run time, so this builds without EGL or GLES headers.
#include <dirent.h>
#include <dlfcn.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef void *EGLDisplay;
typedef void *EGLConfig;
typedef void *EGLContext;
typedef int32_t EGLint;
typedef unsigned EGLBoolean;
typedef unsigned EGLenum;
typedef intptr_t EGLAttrib;

#define EGL_PLATFORM_GBM_KHR 0x31D7
#define EGL_PLATFORM_SURFACELESS_MESA 0x31DD
#define EGL_NONE 0x3038
#define EGL_RENDERABLE_TYPE 0x3040
#define EGL_SURFACE_TYPE 0x3033
#define EGL_OPENGL_ES3_BIT 0x0040
#define EGL_CONTEXT_CLIENT_VERSION 0x3098
#define EGL_OPENGL_ES_API 0x30A0

#define GL_VENDOR 0x1F00
#define GL_RENDERER 0x1F01
#define GL_VERSION 0x1F02
#define GL_TEXTURE_2D 0x0DE1
#define GL_RGBA 0x1908
#define GL_UNSIGNED_BYTE 0x1401
#define GL_FRAMEBUFFER 0x8D40
#define GL_COLOR_ATTACHMENT0 0x8CE0
#define GL_FRAMEBUFFER_COMPLETE 0x8CD5
#define GL_COLOR_BUFFER_BIT 0x4000

static int missing;

static void *symbol(void *lib, const char *name) {
    void *f = dlsym(lib, name);
    if (!f) {
        fprintf(stderr, "gizlet-gl-info: missing %s\n", name);
        missing = 1;
    }
    return f;
}

// The first render node that opens, or -1.
static int render_node(char *path, size_t size) {
    DIR *dir = opendir("/dev/dri");
    struct dirent *e;
    int fd = -1;
    if (!dir) return -1;
    while (fd < 0 && (e = readdir(dir))) {
        if (strncmp(e->d_name, "renderD", 7) != 0) continue;
        snprintf(path, size, "/dev/dri/%s", e->d_name);
        fd = open(path, O_RDWR | O_CLOEXEC);
    }
    closedir(dir);
    return fd;
}

int main(void) {
    void *egl = dlopen("libEGL.so.1", RTLD_NOW);
    void *gles = egl ? dlopen("libGLESv2.so.2", RTLD_NOW) : NULL;
    void *gbm = gles ? dlopen("libgbm.so.1", RTLD_NOW) : NULL;
    if (!gbm) {
        fprintf(stderr, "gizlet-gl-info: %s\n", dlerror());
        return 1;
    }
    EGLDisplay (*GetPlatformDisplay)(EGLenum, void *, const EGLAttrib *) = symbol(egl, "eglGetPlatformDisplay");
    EGLBoolean (*Initialize)(EGLDisplay, EGLint *, EGLint *) = symbol(egl, "eglInitialize");
    EGLBoolean (*BindAPI)(EGLenum) = symbol(egl, "eglBindAPI");
    EGLBoolean (*ChooseConfig)(EGLDisplay, const EGLint *, EGLConfig *, EGLint, EGLint *) = symbol(egl, "eglChooseConfig");
    EGLContext (*CreateContext)(EGLDisplay, EGLConfig, EGLContext, const EGLint *) = symbol(egl, "eglCreateContext");
    EGLBoolean (*MakeCurrent)(EGLDisplay, void *, void *, EGLContext) = symbol(egl, "eglMakeCurrent");
    EGLint (*GetError)(void) = symbol(egl, "eglGetError");
    EGLBoolean (*Terminate)(EGLDisplay) = symbol(egl, "eglTerminate");
    void *(*CreateDevice)(int) = symbol(gbm, "gbm_create_device");
    const char *(*GetString)(unsigned) = symbol(gles, "glGetString");
    void (*GenTextures)(int, unsigned *) = symbol(gles, "glGenTextures");
    void (*BindTexture)(unsigned, unsigned) = symbol(gles, "glBindTexture");
    void (*TexImage2D)(unsigned, int, int, int, int, int, unsigned, unsigned, const void *) = symbol(gles, "glTexImage2D");
    void (*GenFramebuffers)(int, unsigned *) = symbol(gles, "glGenFramebuffers");
    void (*BindFramebuffer)(unsigned, unsigned) = symbol(gles, "glBindFramebuffer");
    void (*FramebufferTexture2D)(unsigned, unsigned, unsigned, unsigned, int) = symbol(gles, "glFramebufferTexture2D");
    unsigned (*CheckFramebufferStatus)(unsigned) = symbol(gles, "glCheckFramebufferStatus");
    void (*ClearColor)(float, float, float, float) = symbol(gles, "glClearColor");
    void (*Clear)(unsigned) = symbol(gles, "glClear");
    void (*ReadPixels)(int, int, int, int, unsigned, unsigned, void *) = symbol(gles, "glReadPixels");
    if (missing) return 1;

    char node[300] = "";
    int fd = render_node(node, sizeof node);
    EGLDisplay display;
    if (fd >= 0) {
        void *device = CreateDevice(fd);
        printf("platform: gbm on %s\n", node);
        display = device ? GetPlatformDisplay(EGL_PLATFORM_GBM_KHR, device, NULL) : NULL;
    } else {
        printf("platform: surfaceless (no render node)\n");
        display = GetPlatformDisplay(EGL_PLATFORM_SURFACELESS_MESA, NULL, NULL);
    }
    EGLint major = 0, minor = 0, count = 0;
    EGLConfig config = NULL;
    // Rendering goes to a texture, so any surface type will do: the surfaceless
    // platform has no window configs, while EGL asks for one by default.
    const EGLint config_attribs[] = {EGL_RENDERABLE_TYPE, EGL_OPENGL_ES3_BIT, EGL_SURFACE_TYPE, 0, EGL_NONE};
    const EGLint context_attribs[] = {EGL_CONTEXT_CLIENT_VERSION, 3, EGL_NONE};
    if (!display) {
        fprintf(stderr, "gizlet-gl-info: no EGL display (0x%x)\n", GetError());
        return 1;
    }
    if (!Initialize(display, &major, &minor)) {
        fprintf(stderr, "gizlet-gl-info: eglInitialize failed (0x%x)\n", GetError());
        return 1;
    }
    if (!BindAPI(EGL_OPENGL_ES_API)) {
        fprintf(stderr, "gizlet-gl-info: no OpenGL ES API (0x%x)\n", GetError());
        return 1;
    }
    // Without a matching config, Mesa still makes a context (EGL_KHR_no_config_context).
    if (!ChooseConfig(display, config_attribs, &config, 1, &count) || count < 1) config = NULL;
    EGLContext context = CreateContext(display, config, NULL, context_attribs);
    if (!context || !MakeCurrent(display, NULL, NULL, context)) {
        fprintf(stderr, "gizlet-gl-info: no OpenGL ES 3.0 context (0x%x)\n", GetError());
        return 1;
    }
    printf("egl: %d.%d\nvendor: %s\nrenderer: %s\nversion: %s\n", major, minor, GetString(GL_VENDOR),
           GetString(GL_RENDERER), GetString(GL_VERSION));

    unsigned texture = 0, framebuffer = 0;
    unsigned char pixel[4] = {0};
    GenTextures(1, &texture);
    BindTexture(GL_TEXTURE_2D, texture);
    TexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, 16, 16, 0, GL_RGBA, GL_UNSIGNED_BYTE, NULL);
    GenFramebuffers(1, &framebuffer);
    BindFramebuffer(GL_FRAMEBUFFER, framebuffer);
    FramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, texture, 0);
    if (CheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE) {
        fprintf(stderr, "gizlet-gl-info: incomplete framebuffer\n");
        return 1;
    }
    ClearColor(0.25f, 0.5f, 0.75f, 1.0f);
    Clear(GL_COLOR_BUFFER_BIT);
    ReadPixels(0, 0, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, pixel);
    int ok = pixel[0] >= 63 && pixel[0] <= 65 && pixel[1] >= 127 && pixel[1] <= 129 && pixel[2] >= 190 &&
             pixel[2] <= 192;
    printf("render: %s (%u %u %u)\n", ok ? "ok" : "wrong", pixel[0], pixel[1], pixel[2]);
    Terminate(display);
    return ok ? 0 : 1;
}

// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#define EGL_EGLEXT_PROTOTYPES
#define GL_GLEXT_PROTOTYPES
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES3/gl3.h>
#include <GLES2/gl2ext.h>
#include <gbm.h>
#include <drm_fourcc.h>
#include <fcntl.h>
#include <sys/socket.h>
#include <array>
#include <climits>
#include <cstring>
#include <memory>
#include <stdexcept>

// A deliberately conservative GPU transport: one immutable allocation per
// frame, a GPU-to-GPU copy and glFinish before returning CEF's source buffer.
// No CEF-owned descriptor outlives OnAcceleratedPaint. SCM_RIGHTS transfers
// independent references; GTK closes them only when its texture is released.
// A bounded nonblocking socket drops frames when the consumer is behind.
class GpuBridge {
 public:
  explicit GpuBridge(int socket) : socket_(socket) {}
  ~GpuBridge() {
    if (display_ != EGL_NO_DISPLAY) {
      eglMakeCurrent(display_, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
      if (context_ != EGL_NO_CONTEXT) eglDestroyContext(display_, context_);
      eglTerminate(display_);
    }
    if (device_) gbm_device_destroy(device_);
    if (render_fd_ >= 0) close(render_fd_);
    if (socket_ >= 0) close(socket_);
  }
  bool Copy(const CefAcceleratedPaintInfo& info, uint64_t sequence, uint64_t view,
            const char* surface, uint64_t generation) {
    struct RestoreApi {
      EGLenum api = eglQueryAPI();
      ~RestoreApi() { eglBindAPI(api); }
    } restore_api;
    Initialize();
    // CEF can leave its own GL context current when calling client code.
    const auto previous_display = eglGetCurrentDisplay();
    const auto previous_context = eglGetCurrentContext();
    const auto previous_draw = eglGetCurrentSurface(EGL_DRAW);
    const auto previous_read = eglGetCurrentSurface(EGL_READ);
    Check(eglBindAPI(EGL_OPENGL_ES_API), "bind GPU copy GLES API");
    Check(eglMakeCurrent(display_, EGL_NO_SURFACE, EGL_NO_SURFACE, context_), "make GPU copy context current");
    struct Restore {
      EGLDisplay own, display; EGLContext context; EGLSurface draw, read;
      ~Restore() {
        if (display != EGL_NO_DISPLAY) eglMakeCurrent(display, draw, read, context);
        else eglMakeCurrent(own, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
      }
    } restore{display_, previous_display, previous_context, previous_draw, previous_read};
    const int w = info.extra.coded_size.width, h = info.extra.coded_size.height;
    Check(w > 0 && h > 0 && w <= 4096 && h <= 4096, "GPU dimensions");
    Check(info.plane_count >= 1 && info.plane_count <= 4, "GPU plane count");
    Check(info.extra.visible_rect.x == 0 && info.extra.visible_rect.y == 0 &&
          info.extra.visible_rect.width == w && info.extra.visible_rect.height == h,
          "GPU cropped frames are not supported by this probe");
    uint32_t source_format;
    if (info.format == CEF_COLOR_TYPE_BGRA_8888) source_format = DRM_FORMAT_ARGB8888;
    else if (info.format == CEF_COLOR_TYPE_RGBA_8888) source_format = DRM_FORMAT_ABGR8888;
    else throw std::runtime_error("unsupported CEF GPU format");
    Image source(display_, CreateImage(w, h, source_format, info.modifier, info.planes, info.plane_count));
    std::unique_ptr<gbm_bo, decltype(&gbm_bo_destroy)> destination(
        gbm_bo_create(device_, w, h, GBM_FORMAT_ARGB8888, GBM_BO_USE_RENDERING | GBM_BO_USE_LINEAR), gbm_bo_destroy);
    Check(destination != nullptr, "allocate client-owned GPU buffer");
    // LINEAR ARGB output intentionally keeps this initial contract single-plane.
    Check(gbm_bo_get_plane_count(destination.get()) == 1, "output GPU planes");
    FileDescriptor output(gbm_bo_get_fd(destination.get()));
    Check(output.fd >= 0, "export client-owned GPU buffer");
    cef_accelerated_paint_native_pixmap_plane_t plane{};
    plane.fd = output.fd;
    plane.stride = gbm_bo_get_stride(destination.get());
    plane.offset = gbm_bo_get_offset(destination.get(), 0);
    const auto modifier = gbm_bo_get_modifier(destination.get());
    Image target(display_, CreateImage(w, h, DRM_FORMAT_ARGB8888, modifier, &plane, 1));
    Textures textures;
    Framebuffers fbos;
    for (int i = 0; i < 2; ++i) {
      glBindTexture(GL_TEXTURE_2D, textures.ids[i]);
      image_target_(GL_TEXTURE_2D, i == 0 ? source.image : target.image);
      glBindFramebuffer(GL_FRAMEBUFFER, fbos.ids[i]);
      glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, textures.ids[i], 0);
      Check(glCheckFramebufferStatus(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE, "GPU copy framebuffer");
    }
    glBindFramebuffer(GL_READ_FRAMEBUFFER, fbos.ids[0]);
    glBindFramebuffer(GL_DRAW_FRAMEBUFFER, fbos.ids[1]);
    glBlitFramebuffer(0, 0, w, h, 0, 0, w, h, GL_COLOR_BUFFER_BIT, GL_NEAREST);
    glFinish();
    Check(glGetError() == GL_NO_ERROR, "GPU frame copy");
    const auto packet = nlohmann::json({{"protocol", 4}, {"view", view}, {"sequence", sequence},
        {"surface", surface}, {"generation", generation},
        {"width", w}, {"height", h}, {"fourcc", DRM_FORMAT_ARGB8888},
        {"modifier", modifier}, {"stride", plane.stride}, {"offset", plane.offset}}).dump();
    iovec iov{const_cast<char*>(packet.data()), packet.size()};
    alignas(cmsghdr) char ancillary[CMSG_SPACE(sizeof(int))]{};
    msghdr message{};
    message.msg_iov = &iov; message.msg_iovlen = 1;
    message.msg_control = ancillary; message.msg_controllen = sizeof(ancillary);
    auto* control = CMSG_FIRSTHDR(&message);
    control->cmsg_level = SOL_SOCKET; control->cmsg_type = SCM_RIGHTS;
    control->cmsg_len = CMSG_LEN(sizeof(int));
    std::memcpy(CMSG_DATA(control), &output.fd, sizeof(int));
    const auto sent = sendmsg(socket_, &message, MSG_DONTWAIT | MSG_NOSIGNAL);
    if (sent < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) return false;
    Check(sent == static_cast<ssize_t>(packet.size()), "send GPU frame");
    return true;
  }
 private:
  struct FileDescriptor { int fd; explicit FileDescriptor(int value) : fd(value) {}
    ~FileDescriptor() { if (fd >= 0) close(fd); } };
  struct Image { EGLDisplay display; EGLImageKHR image;
    Image(EGLDisplay d, EGLImageKHR i) : display(d), image(i) {}
    ~Image() { eglDestroyImage(display, image); } };
  struct Textures { GLuint ids[2]; Textures() { glGenTextures(2, ids); }
    ~Textures() { glDeleteTextures(2, ids); } };
  struct Framebuffers { GLuint ids[2]; Framebuffers() { glGenFramebuffers(2, ids); }
    ~Framebuffers() { glDeleteFramebuffers(2, ids); } };
  static void Check(bool value, const char* message) {
    if (!value) throw std::runtime_error(message);
  }
  void Initialize() {
    if (context_ != EGL_NO_CONTEXT) return;
    // Match a real render node; no software fallback can claim GPU acceptance.
    for (const auto& entry : std::filesystem::directory_iterator("/dev/dri")) {
      if (entry.path().filename().string().starts_with("renderD")) {
        render_fd_ = open(entry.path().c_str(), O_RDWR | O_CLOEXEC);
        if (render_fd_ >= 0) break;
      }
    }
    Check(render_fd_ >= 0, "open DRM render node");
    device_ = gbm_create_device(render_fd_);
    Check(device_ != nullptr, "create GBM device");
    display_ = eglGetPlatformDisplay(EGL_PLATFORM_GBM_KHR, device_, nullptr);
    Check(display_ != EGL_NO_DISPLAY && eglInitialize(display_, nullptr, nullptr), "initialize EGL/GBM");
    Check(eglBindAPI(EGL_OPENGL_ES_API), "bind GLES");
    const EGLint attrs[] = {EGL_SURFACE_TYPE, 0, EGL_RENDERABLE_TYPE,
        EGL_OPENGL_ES3_BIT, EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8, EGL_NONE};
    EGLConfig config; EGLint count = 0;
    Check(eglChooseConfig(display_, attrs, &config, 1, &count) && count == 1, "choose GLES config");
    const EGLint context_attrs[] = {EGL_CONTEXT_CLIENT_VERSION, 3, EGL_NONE};
    context_ = eglCreateContext(display_, config, EGL_NO_CONTEXT, context_attrs);
    Check(context_ != EGL_NO_CONTEXT, "create GPU copy context");
    image_target_ = reinterpret_cast<PFNGLEGLIMAGETARGETTEXTURE2DOESPROC>(eglGetProcAddress("glEGLImageTargetTexture2DOES"));
    Check(image_target_ != nullptr, "EGL image import extension");
  }
  EGLImageKHR CreateImage(int w, int h, uint32_t format, uint64_t modifier,
      const cef_accelerated_paint_native_pixmap_plane_t* planes, int count) {
    Check(count >= 1 && count <= 4, "EGL import plane count");
    std::array<EGLAttrib, 48> attrs{};
    size_t size = 0;
    const auto add = [&](EGLAttrib key, EGLAttrib value) {
      attrs.at(size++) = key;
      attrs.at(size++) = value;
    };
    add(EGL_WIDTH, w); add(EGL_HEIGHT, h); add(EGL_LINUX_DRM_FOURCC_EXT, format);
    const EGLint fd_keys[] = {EGL_DMA_BUF_PLANE0_FD_EXT, EGL_DMA_BUF_PLANE1_FD_EXT, EGL_DMA_BUF_PLANE2_FD_EXT, EGL_DMA_BUF_PLANE3_FD_EXT};
    const EGLint offset_keys[] = {EGL_DMA_BUF_PLANE0_OFFSET_EXT, EGL_DMA_BUF_PLANE1_OFFSET_EXT, EGL_DMA_BUF_PLANE2_OFFSET_EXT, EGL_DMA_BUF_PLANE3_OFFSET_EXT};
    const EGLint pitch_keys[] = {EGL_DMA_BUF_PLANE0_PITCH_EXT, EGL_DMA_BUF_PLANE1_PITCH_EXT, EGL_DMA_BUF_PLANE2_PITCH_EXT, EGL_DMA_BUF_PLANE3_PITCH_EXT};
    const EGLint modifier_keys[] = {EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT, EGL_DMA_BUF_PLANE1_MODIFIER_LO_EXT, EGL_DMA_BUF_PLANE2_MODIFIER_LO_EXT, EGL_DMA_BUF_PLANE3_MODIFIER_LO_EXT};
    for (int i = 0; i < count; ++i) {
      Check(planes[i].fd >= 0 && planes[i].offset <= INT_MAX && planes[i].stride <= INT_MAX, "GPU plane metadata");
      add(fd_keys[i], planes[i].fd); add(offset_keys[i], planes[i].offset); add(pitch_keys[i], planes[i].stride);
      if (modifier != DRM_FORMAT_MOD_INVALID) {
        add(modifier_keys[i], static_cast<EGLint>(modifier & 0xffffffff));
        add(modifier_keys[i] + 1, static_cast<EGLint>(modifier >> 32));
      }
    }
    attrs.at(size) = EGL_NONE;
    auto image = eglCreateImage(display_, EGL_NO_CONTEXT, EGL_LINUX_DMA_BUF_EXT, nullptr, attrs.data());
    Check(image != EGL_NO_IMAGE_KHR, "import DMA-BUF into EGL");
    return image;
  }
  int socket_, render_fd_ = -1;
  gbm_device* device_ = nullptr;
  EGLDisplay display_ = EGL_NO_DISPLAY;
  EGLContext context_ = EGL_NO_CONTEXT;
  PFNGLEGLIMAGETARGETTEXTURE2DOESPROC image_target_ = nullptr;
};

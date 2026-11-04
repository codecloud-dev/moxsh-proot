#!/bin/sh
# 配方：GPU / WebGPU 加速透传（ProoT 增值服务 proot-pro 专属）
# 在容器内启用 Mesa 软件/硬件 GL、Vulkan 与 WebGPU 转发支持，
# 让图形渲染与 AI 推理能调用宿主 GPU（需宿主启用相应设备透传）。
. /tmp/mox-proot-lib.sh
mox-proot_info "配置 GPU / WebGPU 加速透传（ProoT 增值能力）…"

case "$(detect_family)" in
  debian)
    pkginstall mesa-utils libgl1-mesa-dri libegl1 libgles2 \
      libvulkan1 vulkan-tools mesa-vulkan-drivers \
      libwebkit2gtk-4.1-0 2>/dev/null
    ;;
  alpine)
    pkginstall mesa mesa-dri-gallium mesa-egl mesa-gles vulkan-loader vulkan-tools 2>/dev/null
    ;;
  arch)
    pkginstall mesa vulkan-icd-loader vulkan-tools 2>/dev/null
    ;;
  redhat)
    pkginstall mesa-dri-drivers mesa-libGL mesa-libEGL vulkan-loader vulkan-tools 2>/dev/null
    ;;
  suse)
    pkginstall Mesa Mesa-dri Mesa-libGL Mesa-libEGL vulkan-loader vulkan-tools 2>/dev/null
    ;;
esac

# 默认开启软件渲染兜底 + 允许 EGL 使用 surfaceless 平台（无显示也可离屏渲染）
export LIBGL_ALWAYS_SOFTWARE="${LIBGL_ALWAYS_SOFTWARE:-0}"
export EGL_PLATFORM="${EGL_PLATFORM:-surfaceless}"

mox-proot_ok "GPU 透传就绪：glxinfo/vulkaninfo 可验证；WebGPU 由 Mesa 的 lavapipe 提供软件兜底"

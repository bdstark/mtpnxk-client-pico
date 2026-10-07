cmake_minimum_required(VERSION 3.13)
set(CMAKE_C_STANDARD 11)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

# Pico W only for now: the TinyUSB bundled with the SDK compiles the PIO-USB
# host port for RP2040 alone, and the otactl bootstrap targets pico-w.
set(PICO_BOARD pico_w CACHE STRING "Board type")

# Pico-PIO-USB is vendored as a submodule; the SDK's TinyUSB picks it up from
# PICO_PIO_USB_PATH while pico_sdk_init() runs, so this must be set first.
set(PICO_PIO_USB_PATH "${CMAKE_CURRENT_LIST_DIR}/lib/Pico-PIO-USB" CACHE PATH "Pico-PIO-USB source path")

include(pico_sdk_import.cmake)
project(mtpnxk_client_pico C CXX ASM)
pico_sdk_init()

# ---------------------------------------------------------------------------
# Build options
# ---------------------------------------------------------------------------
option(MTPNXK_OTACTL_SLOT
    "Link for the otactl runtime slot (flash offset 0x100000) and read Wi-Fi and options from the otactl flash store"
    ON)
set(MTPNXK_PIO_USB_DP_PIN 16 CACHE STRING "GPIO for the host port D+; D- is the next GPIO")
set(MTPNXK_WIFI_SSID "" CACHE STRING "Bench Wi-Fi SSID (standalone builds, or fallback when the store is empty)")
set(MTPNXK_WIFI_PASSWORD "" CACHE STRING "Bench Wi-Fi password")
set(MTPNXK_OSC_HOST "127.0.0.1" CACHE STRING "Default OSC target host (overridden by the otactl options form)")
set(MTPNXK_OSC_PORT 8000 CACHE STRING "Default OSC target UDP port")

if (NOT EXISTS "${PICO_PIO_USB_PATH}/src/pio_usb.c")
    message(FATAL_ERROR "Pico-PIO-USB not found at ${PICO_PIO_USB_PATH}: run 'git submodule update --init'")
endif()
if (NOT TARGET tinyusb_pico_pio_usb)
    message(FATAL_ERROR "The SDK's TinyUSB did not create tinyusb_pico_pio_usb; check PICO_PIO_USB_PATH and the compiler (GCC required)")
endif()

# ---------------------------------------------------------------------------
# Version from git
# ---------------------------------------------------------------------------
set(MTPNXK_VERSION "unknown")
set(MTPNXK_GIT_HASH "unknown")
find_package(Git QUIET)
if (Git_FOUND)
    execute_process(COMMAND ${GIT_EXECUTABLE} -C ${CMAKE_CURRENT_SOURCE_DIR} describe --tags --always --dirty
        OUTPUT_VARIABLE MTPNXK_VERSION OUTPUT_STRIP_TRAILING_WHITESPACE ERROR_QUIET)
    execute_process(COMMAND ${GIT_EXECUTABLE} -C ${CMAKE_CURRENT_SOURCE_DIR} rev-parse --short=12 HEAD
        OUTPUT_VARIABLE MTPNXK_GIT_HASH OUTPUT_STRIP_TRAILING_WHITESPACE ERROR_QUIET)
endif()
if (NOT MTPNXK_VERSION)
    set(MTPNXK_VERSION "unknown")
endif()
if (NOT MTPNXK_GIT_HASH)
    set(MTPNXK_GIT_HASH "unknown")
endif()
message(STATUS "mtpnxk ${MTPNXK_VERSION} (${MTPNXK_GIT_HASH}), otactl slot: ${MTPNXK_OTACTL_SLOT}")

# ---------------------------------------------------------------------------
# Firmware
# ---------------------------------------------------------------------------
add_executable(mtpnxk
    src/main.c
    src/config.c
    src/log.c
    src/nxk_host.c
    src/nxk_decode.c
    src/route.c
    src/hid_kbd.c
    src/usb_descriptors.c
    src/osc.c
    src/net.c
    src/otactl_store.c
)

target_include_directories(mtpnxk PRIVATE ${CMAKE_CURRENT_LIST_DIR}/src)

target_compile_definitions(mtpnxk PRIVATE
    PIO_USB_USE_TINYUSB=1
    MTPNXK_PIO_USB_DP_PIN=${MTPNXK_PIO_USB_DP_PIN}
    MTPNXK_WIFI_SSID="${MTPNXK_WIFI_SSID}"
    MTPNXK_WIFI_PASSWORD="${MTPNXK_WIFI_PASSWORD}"
    MTPNXK_OSC_HOST="${MTPNXK_OSC_HOST}"
    MTPNXK_OSC_PORT=${MTPNXK_OSC_PORT}
    MTPNXK_VERSION="${MTPNXK_VERSION}"
    MTPNXK_GIT_HASH="${MTPNXK_GIT_HASH}"
    CYW43_HOST_NAME="mtpnxk"
    # cyw43 and Pico-PIO-USB both claim PIO state machines; see main.c for the
    # start-up order that keeps them apart.
    PICO_CYW43_ARCH_THREADSAFE_BACKGROUND=1
)
if (MTPNXK_OTACTL_SLOT)
    target_compile_definitions(mtpnxk PRIVATE MTPNXK_OTACTL_SLOT=1)
    pico_add_linker_script_override_path(mtpnxk ${CMAKE_CURRENT_LIST_DIR}/ld/otactl-slot FILES memory_flash.incl)
else()
    target_compile_definitions(mtpnxk PRIVATE MTPNXK_OTACTL_SLOT=0)
endif()

target_compile_options(mtpnxk PRIVATE -Wall -Wextra)
target_link_options(mtpnxk PRIVATE -Xlinker --print-memory-usage)

target_link_libraries(mtpnxk PRIVATE
    pico_stdlib
    pico_multicore
    pico_unique_id
    hardware_watchdog
    hardware_flash
    tinyusb_device
    tinyusb_host
    tinyusb_pico_pio_usb
    pico_cyw43_arch_lwip_threadsafe_background
)

# Logs on UART0 (GP0/GP1). The native USB port is the HID keyboard facing the
# lighting PC, so USB stdio stays off.
pico_enable_stdio_uart(mtpnxk 1)
pico_enable_stdio_usb(mtpnxk 0)

pico_set_program_name(mtpnxk "mtpnxk")
pico_set_program_version(mtpnxk "${MTPNXK_VERSION}")
pico_add_extra_outputs(mtpnxk)

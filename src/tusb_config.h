#ifndef MTPNXK_TUSB_CONFIG_H
#define MTPNXK_TUSB_CONFIG_H

#ifdef __cplusplus
extern "C" {
#endif

// Common ---------------------------------------------------------------------
#define CFG_TUSB_OS OPT_OS_PICO

// Device stack on the native controller (roothub port 0): the HID keyboard
// the lighting PC sees.
#define CFG_TUD_ENABLED 1

// Host stack on the PIO port (roothub port 1): the NX-K keypad.
#define CFG_TUH_ENABLED 1
#define CFG_TUH_RPI_PIO_USB 1

#ifndef CFG_TUSB_MEM_SECTION
#define CFG_TUSB_MEM_SECTION
#endif
#ifndef CFG_TUSB_MEM_ALIGN
#define CFG_TUSB_MEM_ALIGN __attribute__((aligned(4)))
#endif

// Device ---------------------------------------------------------------------
#ifndef CFG_TUD_ENDPOINT0_SIZE
#define CFG_TUD_ENDPOINT0_SIZE 64
#endif
#define CFG_TUD_HID 1
#define CFG_TUD_CDC 1
#define CFG_TUD_CDC_RX_BUFSIZE 256
#define CFG_TUD_CDC_TX_BUFSIZE 1024
#define CFG_TUD_CDC_EP_BUFSIZE 64
#define CFG_TUD_MSC 0
#define CFG_TUD_MIDI 0
#define CFG_TUD_VENDOR 0
#define CFG_TUD_HID_EP_BUFSIZE 16

// Host -----------------------------------------------------------------------
#define CFG_TUH_ENUMERATION_BUFSIZE 256
#define CFG_TUH_HUB 0
#define CFG_TUH_DEVICE_MAX 1
#define CFG_TUH_HID 0
#define CFG_TUH_CDC 0
#define CFG_TUH_MSC 0
#define CFG_TUH_VENDOR 0
// The NX-K exposes a vendor-specific interface with one interrupt IN
// endpoint; it is driven through the raw endpoint API, not a class driver.
#define CFG_TUH_API_EDPT_XFER 1

#ifdef __cplusplus
}
#endif

#endif

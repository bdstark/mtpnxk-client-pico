#include "usb_reset.h"

#include "device/usbd_pvt.h"
#include "hardware/watchdog.h"
#include "pico/bootrom.h"
#include "tusb.h"

// TinyUSB refuses SET_CONFIGURATION unless every interface in the
// configuration is claimed by a class driver (the host then sees
// "can't set config #1"). The reset interface has no endpoints and no
// class, so this minimal application driver exists purely to claim it and
// to answer picotool's two vendor requests on it, as pico_stdio_usb does.

static void resetd_init(void)
{
}

static bool resetd_deinit(void)
{
    return true;
}

static void resetd_reset(uint8_t rhport)
{
    (void)rhport;
}

static uint16_t resetd_open(uint8_t rhport, tusb_desc_interface_t const *itf_desc, uint16_t max_len)
{
    (void)rhport;
    TU_VERIFY(TUSB_CLASS_VENDOR_SPECIFIC == itf_desc->bInterfaceClass &&
                  USB_RESET_INTERFACE_SUBCLASS == itf_desc->bInterfaceSubClass &&
                  USB_RESET_INTERFACE_PROTOCOL == itf_desc->bInterfaceProtocol,
              0);
    uint16_t const drv_len = sizeof(tusb_desc_interface_t);
    TU_VERIFY(max_len >= drv_len, 0);
    return drv_len;
}

static bool resetd_control_xfer_cb(uint8_t rhport, uint8_t stage, tusb_control_request_t const *request)
{
    (void)rhport;
    if (stage != CONTROL_STAGE_SETUP)
    {
        return true;
    }
    if (request->wIndex != USB_RESET_ITF_NUM || request->bmRequestType_bit.type != TUSB_REQ_TYPE_VENDOR)
    {
        return false;
    }
    switch (request->bRequest)
    {
    case USB_RESET_REQUEST_BOOTSEL:
        // Low 7 bits of wValue are picotool's interface-disable flags.
        reset_usb_boot(0, request->wValue & 0x7f);
        return true; // not reached
    case USB_RESET_REQUEST_FLASH:
        watchdog_reboot(0, 0, 100);
        return true;
    default:
        return false;
    }
}

static bool resetd_xfer_cb(uint8_t rhport, uint8_t ep_addr, xfer_result_t result, uint32_t xferred_bytes)
{
    (void)rhport;
    (void)ep_addr;
    (void)result;
    (void)xferred_bytes;
    return true;
}

static usbd_class_driver_t const s_reset_driver = {
    .name = "RESET",
    .init = resetd_init,
    .deinit = resetd_deinit,
    .reset = resetd_reset,
    .open = resetd_open,
    .control_xfer_cb = resetd_control_xfer_cb,
    .xfer_cb = resetd_xfer_cb,
    .sof = NULL,
};

usbd_class_driver_t const *usbd_app_driver_get_cb(uint8_t *driver_count)
{
    *driver_count = 1;
    return &s_reset_driver;
}

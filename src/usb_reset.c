#include "usb_reset.h"

#include "hardware/watchdog.h"
#include "pico/bootrom.h"
#include "tusb.h"

// Vendor control requests that no class driver claimed land here.
bool tud_vendor_control_xfer_cb(uint8_t rhport, uint8_t stage, tusb_control_request_t const *request)
{
    (void)rhport;
    if (stage != CONTROL_STAGE_SETUP)
    {
        return true;
    }
    if (request->bmRequestType_bit.type != TUSB_REQ_TYPE_VENDOR || request->wIndex != USB_RESET_ITF_NUM)
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

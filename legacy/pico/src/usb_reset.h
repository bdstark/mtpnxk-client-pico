#ifndef MTPNXK_USB_RESET_H
#define MTPNXK_USB_RESET_H

// Raspberry Pi's vendor "reset" interface, as pico_stdio_usb exposes it, so
// picotool can reboot the board into BOOTSEL (`picotool load -f`) or back
// into the application (`picotool reboot`) over the running device port.
// The constants mirror pico/usb_reset_interface.h; picotool recognises the
// interface by vendor id 0x2E8A plus class FF / subclass 00 / protocol 01.

#define USB_RESET_ITF_NUM 3

#define USB_RESET_INTERFACE_SUBCLASS 0x00
#define USB_RESET_INTERFACE_PROTOCOL 0x01
#define USB_RESET_REQUEST_BOOTSEL 0x01
#define USB_RESET_REQUEST_FLASH 0x02

#define USB_RESET_DESC_LEN 9
#define USB_RESET_DESCRIPTOR(_itfnum, _stridx)                                                                        \
    9, TUSB_DESC_INTERFACE, _itfnum, 0, 0, TUSB_CLASS_VENDOR_SPECIFIC, USB_RESET_INTERFACE_SUBCLASS,                 \
        USB_RESET_INTERFACE_PROTOCOL, _stridx

#endif

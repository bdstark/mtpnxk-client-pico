// USB device descriptors: a composite of
//   - a boot-protocol HID keyboard (the console keys for grandMA3),
//   - a CDC serial port (log and bench console),
//   - the Raspberry Pi reset interface so `picotool load -f` and
//     `picotool reboot` work without touching BOOTSEL.
//
// grandMA3's keyboard-shortcut layer treats any keyboard as console keys, so
// the NX-K keypad is presented to the lighting PC as a plain keyboard. The
// serial port shows up as a COM port there, which is harmless.

#include <string.h>

#include "pico/unique_id.h"
#include "tusb.h"

#include "usb_reset.h"

// Raspberry Pi's vendor id with a product id from the range they allow for
// hobby/internal use of RP2040 designs. Not registered anywhere; it only has
// to be distinct on the lighting PC. picotool keys its reset interface
// detection on this vendor id.
#define USB_VID 0x2E8A
#define USB_PID 0x104E
#define USB_BCD 0x0200

static tusb_desc_device_t const desc_device = {
    .bLength = sizeof(tusb_desc_device_t),
    .bDescriptorType = TUSB_DESC_DEVICE,
    .bcdUSB = USB_BCD,
    // Composite with an IAD (for CDC): class must be MISC/COMMON/IAD.
    .bDeviceClass = TUSB_CLASS_MISC,
    .bDeviceSubClass = MISC_SUBCLASS_COMMON,
    .bDeviceProtocol = MISC_PROTOCOL_IAD,
    .bMaxPacketSize0 = CFG_TUD_ENDPOINT0_SIZE,
    .idVendor = USB_VID,
    .idProduct = USB_PID,
    .bcdDevice = 0x0100,
    .iManufacturer = 0x01,
    .iProduct = 0x02,
    .iSerialNumber = 0x03,
    .bNumConfigurations = 0x01,
};

uint8_t const *tud_descriptor_device_cb(void)
{
    return (uint8_t const *)&desc_device;
}

static uint8_t const desc_hid_report[] = {
    TUD_HID_REPORT_DESC_KEYBOARD(),
};

uint8_t const *tud_hid_descriptor_report_cb(uint8_t itf)
{
    (void)itf;
    return desc_hid_report;
}

// CDC first: that is the ordering the Pico SDK's own stdio_usb uses and
// macOS binds its ACM driver to without complaint.
enum
{
    ITF_NUM_CDC = 0,  // CDC takes two interfaces
    ITF_NUM_CDC_DATA, //
    ITF_NUM_HID,
    ITF_NUM_RESET,
    ITF_NUM_TOTAL,
};

_Static_assert(ITF_NUM_RESET == USB_RESET_ITF_NUM, "usb_reset.h must agree on the reset interface number");

#define CONFIG_TOTAL_LEN (TUD_CONFIG_DESC_LEN + TUD_HID_DESC_LEN + TUD_CDC_DESC_LEN + USB_RESET_DESC_LEN)

#define EPNUM_CDC_NOTIF 0x81
#define EPNUM_CDC_OUT 0x02
#define EPNUM_CDC_IN 0x82
#define EPNUM_HID_IN 0x83

enum
{
    STRID_LANGID = 0,
    STRID_MANUFACTURER,
    STRID_PRODUCT,
    STRID_SERIAL,
    STRID_HID,
    STRID_CDC,
    STRID_RESET,
};

// Bus powered, and the NX-K on the host port is fed from this VBUS, so ask
// for the full 500 mA.
static uint8_t const desc_configuration[] = {
    TUD_CONFIG_DESCRIPTOR(1, ITF_NUM_TOTAL, 0, CONFIG_TOTAL_LEN, 0x00, 500),
    TUD_CDC_DESCRIPTOR(ITF_NUM_CDC, STRID_CDC, EPNUM_CDC_NOTIF, 8, EPNUM_CDC_OUT, EPNUM_CDC_IN, CFG_TUD_CDC_EP_BUFSIZE),
    TUD_HID_DESCRIPTOR(ITF_NUM_HID, STRID_HID, HID_ITF_PROTOCOL_KEYBOARD, sizeof(desc_hid_report), EPNUM_HID_IN,
                       CFG_TUD_HID_EP_BUFSIZE, 5),
    USB_RESET_DESCRIPTOR(ITF_NUM_RESET, STRID_RESET),
};

uint8_t const *tud_descriptor_configuration_cb(uint8_t index)
{
    (void)index;
    return desc_configuration;
}

static char const *string_desc_arr[] = {
    [STRID_LANGID] = (const char[]){0x09, 0x04}, // English (US)
    [STRID_MANUFACTURER] = "newtonhaus",
    [STRID_PRODUCT] = "mtpnxk NX-K bridge",
    [STRID_SERIAL] = NULL, // board unique id
    [STRID_HID] = "NX-K keys",
    [STRID_CDC] = "mtpnxk console",
    [STRID_RESET] = "Reset",
};

static uint16_t desc_str[32 + 1];

uint16_t const *tud_descriptor_string_cb(uint8_t index, uint16_t langid)
{
    (void)langid;
    size_t chr_count;

    if (index == STRID_LANGID)
    {
        memcpy(&desc_str[1], string_desc_arr[0], 2);
        chr_count = 1;
    }
    else
    {
        char serial[2 * PICO_UNIQUE_BOARD_ID_SIZE_BYTES + 1];
        const char *str;
        if (index == STRID_SERIAL)
        {
            pico_get_unique_board_id_string(serial, sizeof(serial));
            str = serial;
        }
        else
        {
            if (index >= sizeof(string_desc_arr) / sizeof(string_desc_arr[0]))
            {
                return NULL;
            }
            str = string_desc_arr[index];
            if (str == NULL)
            {
                return NULL;
            }
        }
        chr_count = strlen(str);
        if (chr_count > 32)
        {
            chr_count = 32;
        }
        for (size_t i = 0; i < chr_count; i++)
        {
            desc_str[1 + i] = (uint16_t)(uint8_t)str[i];
        }
    }

    desc_str[0] = (uint16_t)((TUSB_DESC_STRING << 8) | (2 * chr_count + 2));
    return desc_str;
}

// Host-to-device reports (keyboard LEDs) are ignored; nothing to read back.
uint16_t tud_hid_get_report_cb(uint8_t itf, uint8_t report_id, hid_report_type_t report_type, uint8_t *buffer,
                               uint16_t reqlen)
{
    (void)itf;
    (void)report_id;
    (void)report_type;
    (void)buffer;
    (void)reqlen;
    return 0;
}

void tud_hid_set_report_cb(uint8_t itf, uint8_t report_id, hid_report_type_t report_type, uint8_t const *buffer,
                           uint16_t bufsize)
{
    (void)itf;
    (void)report_id;
    (void)report_type;
    (void)buffer;
    (void)bufsize;
}

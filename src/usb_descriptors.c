// USB device descriptors: one boot-protocol HID keyboard.
//
// grandMA3's keyboard-shortcut layer treats any keyboard as console keys, so
// the NX-K keypad is presented to the lighting PC as a plain keyboard. No
// driver, no focus tricks, no software on the PC.

#include <string.h>

#include "pico/unique_id.h"
#include "tusb.h"

// Raspberry Pi's vendor id with a product id from the range they allow for
// hobby/internal use of RP2040 designs. Not registered anywhere; it only has
// to be distinct on the lighting PC.
#define USB_VID 0x2E8A
#define USB_PID 0x104E
#define USB_BCD 0x0200

static tusb_desc_device_t const desc_device = {
    .bLength = sizeof(tusb_desc_device_t),
    .bDescriptorType = TUSB_DESC_DEVICE,
    .bcdUSB = USB_BCD,
    .bDeviceClass = 0x00,
    .bDeviceSubClass = 0x00,
    .bDeviceProtocol = 0x00,
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

enum
{
    ITF_NUM_HID = 0,
    ITF_NUM_TOTAL,
};

#define CONFIG_TOTAL_LEN (TUD_CONFIG_DESC_LEN + TUD_HID_DESC_LEN)
#define EPNUM_HID 0x81

// Bus powered, and the NX-K on the host port is fed from this VBUS, so ask
// for the full 500 mA.
static uint8_t const desc_configuration[] = {
    TUD_CONFIG_DESCRIPTOR(1, ITF_NUM_TOTAL, 0, CONFIG_TOTAL_LEN, 0x00, 500),
    TUD_HID_DESCRIPTOR(ITF_NUM_HID, 0, HID_ITF_PROTOCOL_KEYBOARD, sizeof(desc_hid_report),
                       EPNUM_HID, CFG_TUD_HID_EP_BUFSIZE, 5),
};

uint8_t const *tud_descriptor_configuration_cb(uint8_t index)
{
    (void)index;
    return desc_configuration;
}

enum
{
    STRID_LANGID = 0,
    STRID_MANUFACTURER,
    STRID_PRODUCT,
    STRID_SERIAL,
};

static char const *string_desc_arr[] = {
    (const char[]){0x09, 0x04}, // English (US)
    "newtonhaus",
    "mtpnxk NX-K bridge",
    NULL, // serial: board unique id
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
uint16_t tud_hid_get_report_cb(uint8_t itf, uint8_t report_id, hid_report_type_t report_type,
                               uint8_t *buffer, uint16_t reqlen)
{
    (void)itf;
    (void)report_id;
    (void)report_type;
    (void)buffer;
    (void)reqlen;
    return 0;
}

void tud_hid_set_report_cb(uint8_t itf, uint8_t report_id, hid_report_type_t report_type,
                           uint8_t const *buffer, uint16_t bufsize)
{
    (void)itf;
    (void)report_id;
    (void)report_type;
    (void)buffer;
    (void)bufsize;
}

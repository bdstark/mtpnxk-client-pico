#ifndef MTPNXK_NXK_DECODE_H
#define MTPNXK_NXK_DECODE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// Obsidian (Elation) NX-K ONYX keypad. Ported from avrsvc cmd/nxk/decode.go.

#define NXK_VID 0x11be
#define NXK_PID 0xe102
#define NXK_INTERFACE 0
#define NXK_ALT_SETTING 1
#define NXK_ENDPOINT_IN 0x82
#define NXK_PACKET_MAX 64

// Every control has a 16-bit firmware address: group in the high byte,
// control in the low byte. It is what the keypad reports and, as on the
// sister M-Touch (MTouchPlay docs/protocol.md), what the LED write takes
// as wIndex. The encoders report presses at 0x59n1 and turns at 0x59n2;
// their status LED is at the press address.
#define NXK_CONTROL_ID(group, control) ((uint16_t)(((group) << 8) | (control)))

// LED state, the wValue of vendor request 0x80 (bench survey 2026-10-08):
// every NX-K LED is single-colour and answers only the M-Touch green lane.
// bit 0 on, bit 4 blink (with bit 0), bit 5 forces off; bits 1, 2, 6 and 8
// do nothing. The keypad keys (digits, ., /, -, +, @, Enter, Thru, Full)
// have no LED; writes to them are accepted and ignored.
#define NXK_LED_OFF 0x0000
#define NXK_LED_ON 0x0001
#define NXK_LED_BLINK 0x0011

typedef enum
{
    NXK_EVENT_UNKNOWN = 0,
    NXK_EVENT_KEY_DOWN,
    NXK_EVENT_KEY_UP,
    NXK_EVENT_PRESS_DOWN, // encoder pushed
    NXK_EVENT_PRESS_UP,
    NXK_EVENT_ROTATE,
} nxk_event_type_t;

typedef struct
{
    nxk_event_type_t type;
    const char *name; // static string, NULL for unknown
    int delta;        // rotate only; positive is clockwise
    uint16_t id;      // control address, 0 for unknown
} nxk_event_t;

// Returns false only for an empty packet. Unrecognised packets decode to
// NXK_EVENT_UNKNOWN so the caller can log them.
bool nxk_decode(const uint8_t *data, size_t len, nxk_event_t *out);

const char *nxk_event_type_name(nxk_event_type_t type);

// Fills out with the control addresses of every known button (not the
// encoders) and returns how many there are; at most max are written.
size_t nxk_button_ids(uint16_t *out, size_t max);

#endif

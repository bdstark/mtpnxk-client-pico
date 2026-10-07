#ifndef MTPNXK_NXK_DECODE_H
#define MTPNXK_NXK_DECODE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// ETC/Shell NX-K keypad. Ported from avrsvc cmd/nxk/decode.go.

#define NXK_VID 0x11be
#define NXK_PID 0xe102
#define NXK_INTERFACE 0
#define NXK_ALT_SETTING 1
#define NXK_ENDPOINT_IN 0x82
#define NXK_PACKET_MAX 64

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
} nxk_event_t;

// Returns false only for an empty packet. Unrecognised packets decode to
// NXK_EVENT_UNKNOWN so the caller can log them.
bool nxk_decode(const uint8_t *data, size_t len, nxk_event_t *out);

const char *nxk_event_type_name(nxk_event_type_t type);

#endif

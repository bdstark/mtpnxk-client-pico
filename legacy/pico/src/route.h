#ifndef MTPNXK_ROUTE_H
#define MTPNXK_ROUTE_H

#include "nxk_decode.h"

// Mixed-mode routing, mirroring avrsvc cmd/nxk (publisher.go + osc.go +
// keyboard.go): keypad keys become HID keystrokes to the lighting PC, every
// other control goes out as OSC.

void route_init(void);
void route_event(const nxk_event_t *evt);

#endif

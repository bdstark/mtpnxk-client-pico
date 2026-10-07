#ifndef MTPNXK_OSC_H
#define MTPNXK_OSC_H

#include <stdbool.h>
#include <stdint.h>

// Minimal OSC sender over lwIP UDP, matching what avrsvc cmd/nxk/osc.go
// emits: one message per datagram, no bundles.

bool osc_init(const char *host, uint16_t port);
bool osc_send_string(const char *path, const char *value);
bool osc_send_int32(const char *path, int32_t value);
bool osc_send_float(const char *path, float value);

uint32_t osc_sent(void);
uint32_t osc_dropped(void);

#endif

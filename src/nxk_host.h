#ifndef MTPNXK_NXK_HOST_H
#define MTPNXK_NXK_HOST_H

#include <stdbool.h>
#include <stdint.h>

#include "nxk_decode.h"

// USB host side (core 1): enumerates the NX-K on the PIO port, selects its
// vendor interface, and streams interrupt IN packets into a queue that
// core 0 drains with nxk_host_take_packet().

typedef struct
{
    uint8_t len;
    uint8_t data[NXK_PACKET_MAX];
} nxk_packet_t;

// Called on core 0 before core 1 starts.
void nxk_host_init(void);

// Core 1 entry: initialises the PIO host stack and never returns.
void nxk_host_core1_main(void);

// True once tuh_init() has claimed its PIO state machines; core 0 waits for
// this before bringing up the cyw43 Wi-Fi driver, which claims its own.
bool nxk_host_ready(void);

bool nxk_host_take_packet(nxk_packet_t *out);
bool nxk_host_connected(void);
uint32_t nxk_host_packets(void);
uint32_t nxk_host_dropped(void);

#endif

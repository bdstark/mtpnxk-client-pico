#ifndef MTPNXK_NET_H
#define MTPNXK_NET_H

#include <stdbool.h>

// Wi-Fi station bring-up on the cyw43 with automatic reconnect. Wi-Fi is for
// OSC, logs and updates; keystrokes never depend on it.

bool net_init(const char *ssid, const char *password);
void net_task(void);
bool net_is_up(void);
const char *net_ip_string(void);

#endif

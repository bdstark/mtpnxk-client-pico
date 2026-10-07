#ifndef MTPNXK_CONFIG_H
#define MTPNXK_CONFIG_H

#include <stdbool.h>
#include <stdint.h>

// Effective runtime configuration. In an otactl slot build it comes from the
// bootstrap's flash store (Wi-Fi from provisioning, the rest from the app's
// options form); in a standalone build from the CMake cache. CMake values
// are the fallback either way, so a bench board with an empty store still
// comes up.

#define CONFIG_SSID_MAX 32
#define CONFIG_PASS_MAX 64
#define CONFIG_HOST_MAX 64
#define CONFIG_DEVICE_ID_MAX 64

typedef struct
{
    char wifi_ssid[CONFIG_SSID_MAX + 1];
    char wifi_password[CONFIG_PASS_MAX + 1];
    char osc_host[CONFIG_HOST_MAX + 1];
    uint16_t osc_port;
    char device_id[CONFIG_DEVICE_ID_MAX + 1];
    bool from_store; // true when any value came from the otactl flash store
} config_t;

void config_load(config_t *cfg);
void config_log(const config_t *cfg);

#endif

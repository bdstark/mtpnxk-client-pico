#include "config.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "log.h"
#include "otactl_store.h"

#ifndef MTPNXK_WIFI_SSID
#define MTPNXK_WIFI_SSID ""
#endif
#ifndef MTPNXK_WIFI_PASSWORD
#define MTPNXK_WIFI_PASSWORD ""
#endif
#ifndef MTPNXK_OSC_HOST
#define MTPNXK_OSC_HOST "127.0.0.1"
#endif
#ifndef MTPNXK_OSC_PORT
#define MTPNXK_OSC_PORT 8000
#endif

static void copy_str(char *dst, size_t dst_size, const char *src)
{
    snprintf(dst, dst_size, "%s", src);
}

void config_load(config_t *cfg)
{
    memset(cfg, 0, sizeof(*cfg));
    copy_str(cfg->wifi_ssid, sizeof(cfg->wifi_ssid), MTPNXK_WIFI_SSID);
    copy_str(cfg->wifi_password, sizeof(cfg->wifi_password), MTPNXK_WIFI_PASSWORD);
    copy_str(cfg->osc_host, sizeof(cfg->osc_host), MTPNXK_OSC_HOST);
    cfg->osc_port = (uint16_t)MTPNXK_OSC_PORT;
    copy_str(cfg->device_id, sizeof(cfg->device_id), "standalone");

#if MTPNXK_OTACTL_SLOT
    otactl_config_t store;
    if (otactl_store_load(&store))
    {
        cfg->from_store = true;
        if (store.wifi_ssid[0] != '\0')
        {
            copy_str(cfg->wifi_ssid, sizeof(cfg->wifi_ssid), store.wifi_ssid);
            copy_str(cfg->wifi_password, sizeof(cfg->wifi_password), store.wifi_password);
        }
        if (store.device_id[0] != '\0')
        {
            copy_str(cfg->device_id, sizeof(cfg->device_id), store.device_id);
        }

        char value[CONFIG_HOST_MAX + 1];
        if (otactl_store_option(&store, "osc_host", value, sizeof(value)) && value[0] != '\0')
        {
            copy_str(cfg->osc_host, sizeof(cfg->osc_host), value);
        }
        if (otactl_store_option(&store, "osc_port", value, sizeof(value)))
        {
            long port = strtol(value, NULL, 10);
            if (port > 0 && port < 65536)
            {
                cfg->osc_port = (uint16_t)port;
            }
        }
    }
    else
    {
        log_printf("config: otactl flash store empty or invalid, using build defaults");
    }
#endif
}

void config_log(const config_t *cfg)
{
    log_printf("config: source=%s device=%s ssid=%s osc=%s:%u", cfg->from_store ? "otactl-store" : "build",
               cfg->device_id, cfg->wifi_ssid[0] ? cfg->wifi_ssid : "(none)", cfg->osc_host, cfg->osc_port);
}

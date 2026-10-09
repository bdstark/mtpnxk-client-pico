#include "net.h"

#include <string.h>

#include "lwip/netif.h"
#include "pico/cyw43_arch.h"
#include "pico/stdlib.h"

#include "log.h"

#define NET_RETRY_MS 10000

static char s_ssid[33];
static char s_password[65];
static bool s_enabled;
static bool s_up;
static bool s_connecting;
static absolute_time_t s_next_attempt;

bool net_init(const char *ssid, const char *password)
{
    strncpy(s_ssid, ssid, sizeof(s_ssid) - 1);
    strncpy(s_password, password, sizeof(s_password) - 1);

    if (cyw43_arch_init() != 0)
    {
        log_printf("net: cyw43_arch_init failed");
        return false;
    }
    cyw43_arch_enable_sta_mode();

    if (s_ssid[0] == '\0')
    {
        log_printf("net: no Wi-Fi credentials, staying offline");
        return true;
    }
    s_enabled = true;
    s_next_attempt = get_absolute_time();
    return true;
}

static void start_connect(void)
{
    log_printf("net: connecting to %s", s_ssid);
    uint32_t auth = s_password[0] ? CYW43_AUTH_WPA2_AES_PSK : CYW43_AUTH_OPEN;
    int rc = cyw43_arch_wifi_connect_async(s_ssid, s_password[0] ? s_password : NULL, auth);
    if (rc != 0)
    {
        log_printf("net: connect_async failed (%d)", rc);
        s_next_attempt = make_timeout_time_ms(NET_RETRY_MS);
        return;
    }
    s_connecting = true;
}

void net_task(void)
{
    if (!s_enabled)
    {
        return;
    }

    int status = cyw43_tcpip_link_status(&cyw43_state, CYW43_ITF_STA);
    bool up = status == CYW43_LINK_UP;
    if (up != s_up)
    {
        s_up = up;
        if (up)
        {
            log_printf("net: up, ip %s", net_ip_string());
            s_connecting = false;
        }
        else
        {
            log_printf("net: link down (status %d)", status);
            s_next_attempt = make_timeout_time_ms(NET_RETRY_MS);
        }
    }

    if (!s_up && !s_connecting && time_reached(s_next_attempt))
    {
        start_connect();
    }
    if (s_connecting && status < 0)
    {
        // CYW43_LINK_FAIL, CYW43_LINK_NONET, CYW43_LINK_BADAUTH
        log_printf("net: join failed (status %d), retrying", status);
        s_connecting = false;
        s_next_attempt = make_timeout_time_ms(NET_RETRY_MS);
    }
}

bool net_is_up(void)
{
    return s_up;
}

const char *net_ip_string(void)
{
    static char buf[16];
    const ip4_addr_t *ip = netif_ip4_addr(netif_default);
    if (ip == NULL)
    {
        return "0.0.0.0";
    }
    strncpy(buf, ip4addr_ntoa(ip), sizeof(buf) - 1);
    buf[sizeof(buf) - 1] = '\0';
    return buf;
}

#include "osc.h"

#include <string.h>

#include "lwip/ip_addr.h"
#include "lwip/pbuf.h"
#include "lwip/udp.h"
#include "pico/cyw43_arch.h"

#include "log.h"
#include "net.h"

#define OSC_MAX_MESSAGE 128

static struct udp_pcb *s_pcb;
static ip_addr_t s_remote;
static uint16_t s_port;
static uint32_t s_sent;
static uint32_t s_dropped;

bool osc_init(const char *host, uint16_t port)
{
    if (!ipaddr_aton(host, &s_remote))
    {
        log_printf("osc: target %s is not an IP address (DNS not supported yet)", host);
        return false;
    }
    s_port = port;
    cyw43_arch_lwip_begin();
    s_pcb = udp_new_ip_type(IPADDR_TYPE_V4);
    cyw43_arch_lwip_end();
    if (s_pcb == NULL)
    {
        log_printf("osc: udp_new failed");
        return false;
    }
    log_printf("osc: target %s:%u", host, port);
    return true;
}

// Appends s padded with NULs to a 4-byte boundary. Returns the new length or 0 on overflow.
static size_t put_padded(uint8_t *buf, size_t len, const char *s)
{
    size_t n = strlen(s) + 1;
    size_t padded = (n + 3u) & ~3u;
    if (len + padded > OSC_MAX_MESSAGE)
    {
        return 0;
    }
    memcpy(buf + len, s, n);
    memset(buf + len + n, 0, padded - n);
    return len + padded;
}

static size_t put_be32(uint8_t *buf, size_t len, uint32_t v)
{
    if (len + 4 > OSC_MAX_MESSAGE)
    {
        return 0;
    }
    buf[len] = (uint8_t)(v >> 24);
    buf[len + 1] = (uint8_t)(v >> 16);
    buf[len + 2] = (uint8_t)(v >> 8);
    buf[len + 3] = (uint8_t)v;
    return len + 4;
}

static bool send_datagram(const uint8_t *buf, size_t len)
{
    if (s_pcb == NULL || !net_is_up())
    {
        s_dropped++;
        return false;
    }
    cyw43_arch_lwip_begin();
    struct pbuf *p = pbuf_alloc(PBUF_TRANSPORT, (u16_t)len, PBUF_RAM);
    bool ok = false;
    if (p != NULL)
    {
        memcpy(p->payload, buf, len);
        ok = udp_sendto(s_pcb, p, &s_remote, s_port) == ERR_OK;
        pbuf_free(p);
    }
    cyw43_arch_lwip_end();
    if (ok)
    {
        s_sent++;
    }
    else
    {
        s_dropped++;
    }
    return ok;
}

bool osc_send_string(const char *path, const char *value)
{
    uint8_t buf[OSC_MAX_MESSAGE];
    size_t len = put_padded(buf, 0, path);
    if (len)
        len = put_padded(buf, len, ",s");
    if (len)
        len = put_padded(buf, len, value);
    if (!len)
    {
        s_dropped++;
        return false;
    }
    return send_datagram(buf, len);
}

bool osc_send_int32(const char *path, int32_t value)
{
    uint8_t buf[OSC_MAX_MESSAGE];
    size_t len = put_padded(buf, 0, path);
    if (len)
        len = put_padded(buf, len, ",i");
    if (len)
        len = put_be32(buf, len, (uint32_t)value);
    if (!len)
    {
        s_dropped++;
        return false;
    }
    return send_datagram(buf, len);
}

bool osc_send_float(const char *path, float value)
{
    uint32_t bits;
    memcpy(&bits, &value, sizeof(bits));
    uint8_t buf[OSC_MAX_MESSAGE];
    size_t len = put_padded(buf, 0, path);
    if (len)
        len = put_padded(buf, len, ",f");
    if (len)
        len = put_be32(buf, len, bits);
    if (!len)
    {
        s_dropped++;
        return false;
    }
    return send_datagram(buf, len);
}

uint32_t osc_sent(void)
{
    return s_sent;
}

uint32_t osc_dropped(void)
{
    return s_dropped;
}

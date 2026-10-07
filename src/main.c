// mtpnxk: ETC/Shell NX-K keypad bridge on a Raspberry Pi Pico W.
//
// Core 1 hosts the NX-K on a PIO USB port. Core 0 is a HID keyboard on the
// native USB port towards the lighting PC, runs Wi-Fi for OSC, logs and
// updates, and routes decoded keypad events to one or the other.

#include <stdio.h>

#include "hardware/clocks.h"
#include "hardware/watchdog.h"
#include "pico/multicore.h"
#include "pico/stdlib.h"
#include "tusb.h"

#include "config.h"
#include "hid_kbd.h"
#include "log.h"
#include "net.h"
#include "nxk_decode.h"
#include "nxk_host.h"
#include "osc.h"
#include "otactl_store.h"
#include "route.h"

#ifndef MTPNXK_VERSION
#define MTPNXK_VERSION "unknown"
#endif
#ifndef MTPNXK_GIT_HASH
#define MTPNXK_GIT_HASH "unknown"
#endif

static config_t s_config;

static void print_status(void)
{
    log_printf("status: nxk=%s packets=%lu dropped=%lu | kbd=%s dropped=%lu | net=%s ip=%s | osc sent=%lu dropped=%lu",
               nxk_host_connected() ? "connected" : "absent", (unsigned long)nxk_host_packets(),
               (unsigned long)nxk_host_dropped(), hid_kbd_mounted() ? "mounted" : "unmounted",
               (unsigned long)hid_kbd_dropped(), net_is_up() ? "up" : "down", net_ip_string(),
               (unsigned long)osc_sent(), (unsigned long)osc_dropped());
}

// Single-character bench console on the UART.
static void console_poll(void)
{
    int c = getchar_timeout_us(0);
    if (c == PICO_ERROR_TIMEOUT)
    {
        return;
    }
    switch (c)
    {
    case 's':
        print_status();
        break;
    case 'c':
        config_log(&s_config);
        break;
    case 'u':
#if MTPNXK_OTACTL_SLOT
        otactl_request_update_and_reboot();
#else
        log_printf("console: standalone build, no otactl update handoff");
#endif
        break;
    case 'r':
        log_printf("console: rebooting");
        sleep_ms(50);
        watchdog_reboot(0, 0, 0);
        break;
    case 'h':
    case '?':
        log_printf("console: s=status c=config u=update(otactl) r=reboot");
        break;
    default:
        break;
    }
}

static void drain_nxk(void)
{
    nxk_packet_t pkt;
    while (nxk_host_take_packet(&pkt))
    {
        nxk_event_t evt;
        if (!nxk_decode(pkt.data, pkt.len, &evt))
        {
            continue;
        }
        if (evt.type == NXK_EVENT_UNKNOWN)
        {
            char hex[3 * NXK_PACKET_MAX + 1];
            size_t n = 0;
            for (uint8_t i = 0; i < pkt.len && n + 3 < sizeof(hex); ++i)
            {
                n += (size_t)snprintf(hex + n, sizeof(hex) - n, "%02x ", pkt.data[i]);
            }
            log_printf("nxk: unknown packet %s", hex);
            continue;
        }
        if (evt.type == NXK_EVENT_ROTATE)
        {
            log_printf("nxk: %s %s %+d", evt.name, nxk_event_type_name(evt.type), evt.delta);
        }
        else
        {
            log_printf("nxk: %s %s", evt.name, nxk_event_type_name(evt.type));
        }
        route_event(&evt);
    }
}

int main(void)
{
    // Pico-PIO-USB needs the system clock to be a multiple of 12 MHz.
    set_sys_clock_khz(120000, true);
    stdio_init_all();
    log_init();
    log_printf("mtpnxk %s (%s) starting, otactl slot build: %d", MTPNXK_VERSION, MTPNXK_GIT_HASH,
               (int)MTPNXK_OTACTL_SLOT);

    config_load(&s_config);
    config_log(&s_config);

    // USB first, on both cores: the PIO host claims its PIO state machines
    // during tuh_init(), and the cyw43 driver claims its own afterwards, so
    // the two never fight over a state machine.
    nxk_host_init();
    hid_kbd_init();
    route_init();
    multicore_reset_core1();
    multicore_launch_core1(nxk_host_core1_main);
    while (!nxk_host_ready())
    {
        sleep_ms(1);
    }
    tud_init(0);

    net_init(s_config.wifi_ssid, s_config.wifi_password);
    osc_init(s_config.osc_host, s_config.osc_port);

    log_printf("ready");
    while (true)
    {
        tud_task();
        hid_kbd_task();
        net_task();
        drain_nxk();
        console_poll();
    }
    return 0;
}

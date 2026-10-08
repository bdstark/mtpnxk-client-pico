// mtpnxk: ETC/Shell NX-K keypad bridge on a Raspberry Pi Pico W.
//
// Core 1 hosts the NX-K on a PIO USB port. Core 0 is a HID keyboard on the
// native USB port towards the lighting PC, runs Wi-Fi for OSC, logs and
// updates, and routes decoded keypad events to one or the other.

#include <stdio.h>

#include "hardware/clocks.h"
#include "hardware/vreg.h"
#include "hardware/watchdog.h"
#include "pico/bootrom.h"
#include "pico/multicore.h"
#include "pico/stdlib.h"
#include "tusb.h"

#include "config.h"
#include "console.h"
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

// Single-character bench console (UART and USB CDC).
static void console_handle(int c)
{
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
    case 'b':
        // Into the ROM bootloader, for hosts whose picotool cannot drive the
        // reset interface with a custom product id.
        log_printf("console: rebooting into BOOTSEL");
        sleep_ms(50);
        reset_usb_boot(0, 0);
        break;
    case 'h':
    case '?':
        log_printf("console: s=status c=config u=update(otactl) r=reboot b=bootsel");
        break;
    default:
        break;
    }
}

// Keep the device stack and console alive for a while between stages.
static void service_device_for_ms(uint32_t ms)
{
    absolute_time_t deadline = make_timeout_time_ms(ms);
    while (!time_reached(deadline))
    {
        tud_task();
        console_task();
        sleep_ms(1);
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
    // MTPNXK_SYS_CLOCK_KHZ selects it; above 133 MHz the core needs more voltage.
#ifndef MTPNXK_SYS_CLOCK_KHZ
#define MTPNXK_SYS_CLOCK_KHZ 240000
#endif
#if MTPNXK_SYS_CLOCK_KHZ > 133000
    vreg_set_voltage(VREG_VOLTAGE_1_15);
    sleep_ms(2);
#endif
    bool clock_ok = set_sys_clock_khz(MTPNXK_SYS_CLOCK_KHZ, true);
    stdio_init_all();
    log_init();
    log_printf("mtpnxk %s (%s) starting, otactl slot build: %d", MTPNXK_VERSION, MTPNXK_GIT_HASH,
               (int)MTPNXK_OTACTL_SLOT);
    log_printf("clocks: set_sys_clock %d kHz %s, clk_sys=%lu Hz clk_peri=%lu Hz clk_usb=%lu Hz", (int)MTPNXK_SYS_CLOCK_KHZ, clock_ok ? "ok" : "FAILED",
               (unsigned long)clock_get_hz(clk_sys), (unsigned long)clock_get_hz(clk_peri),
               (unsigned long)clock_get_hz(clk_usb));

    config_load(&s_config);
    config_log(&s_config);

    // Order matters:
    //  1. Device stack on the native port first, so the USB console exists
    //     even if everything after it hangs.
    //  2. PIO host on core 1: tuh_init() claims its PIO state machines.
    //  3. cyw43 last: it claims its own state machines after the host has
    //     taken the ones it wants, so the two never fight.
    hid_kbd_init();
    route_init();
    console_init(console_handle);
    tud_init(0);
    log_printf("usb device: initialised, enumerating");

    // Stage A: device only, so the console is up and mirroring before the
    // riskier stages start. Long enough for a terminal to attach.
    service_device_for_ms(4000);

    // Stage B: PIO host on core 1.
    log_printf("usb host: starting on core 1");
    nxk_host_init();
    multicore_reset_core1();
    multicore_launch_core1(nxk_host_core1_main);
    absolute_time_t host_deadline = make_timeout_time_ms(3000);
    while (!nxk_host_ready() && !time_reached(host_deadline))
    {
        tud_task();
        console_task();
        sleep_ms(1);
    }
    log_printf("usb host: %s", nxk_host_ready() ? "initialised on core 1" : "NOT ready after 3 s, continuing without it");
    service_device_for_ms(3000);

    // Stage C: Wi-Fi.
    log_printf("net: starting cyw43");
    bool net_ok = net_init(s_config.wifi_ssid, s_config.wifi_password);
    log_printf("net: init %s", net_ok ? "ok" : "FAILED");
    osc_init(s_config.osc_host, s_config.osc_port);

    log_printf("ready");
    while (true)
    {
        tud_task();
        hid_kbd_task();
        net_task();
        drain_nxk();
        console_task();
    }
    return 0;
}

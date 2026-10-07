#include "nxk_host.h"

#include <string.h>

#include "pico/stdlib.h"
#include "pico/util/queue.h"
#include "pio_usb.h"
#include "tusb.h"

#include "log.h"

#ifndef MTPNXK_PIO_USB_DP_PIN
#define MTPNXK_PIO_USB_DP_PIN 16
#endif

#define NXK_QUEUE_DEPTH 64

static queue_t s_queue;
static volatile bool s_ready;
static volatile bool s_connected;
static volatile uint32_t s_packets;
static volatile uint32_t s_dropped;

static uint8_t s_daddr;
static tusb_desc_endpoint_t s_ep_desc;
static bool s_have_ep;

// Buffers the host controller DMAs into.
CFG_TUH_MEM_SECTION CFG_TUH_MEM_ALIGN static uint8_t s_cfg_desc[CFG_TUH_ENUMERATION_BUFSIZE];
CFG_TUH_MEM_SECTION CFG_TUH_MEM_ALIGN static uint8_t s_in_buf[NXK_PACKET_MAX];

void nxk_host_init(void)
{
    queue_init(&s_queue, sizeof(nxk_packet_t), NXK_QUEUE_DEPTH);
}

void nxk_host_core1_main(void)
{
    sleep_ms(10);

    pio_usb_configuration_t pio_cfg = PIO_USB_DEFAULT_CONFIG;
    pio_cfg.pin_dp = MTPNXK_PIO_USB_DP_PIN;
    tuh_configure(1, TUH_CFGID_RPI_PIO_USB_CONFIGURATION, &pio_cfg);

    // Host stack for the PIO port (roothub port 1) lives on this core so its
    // SOF interrupt does too.
    tuh_init(1);
    s_ready = true;

    while (true)
    {
        tuh_task();
    }
}

bool nxk_host_ready(void)
{
    return s_ready;
}

bool nxk_host_take_packet(nxk_packet_t *out)
{
    return queue_try_remove(&s_queue, out);
}

bool nxk_host_connected(void)
{
    return s_connected;
}

uint32_t nxk_host_packets(void)
{
    return s_packets;
}

uint32_t nxk_host_dropped(void)
{
    return s_dropped;
}

// Enumeration state machine (all asynchronous, all on core 1) --------------

static void read_complete(tuh_xfer_t *xfer);

static void start_read(void)
{
    tuh_xfer_t xfer = {
        .daddr = s_daddr,
        .ep_addr = s_ep_desc.bEndpointAddress,
        .buflen = tu_edpt_packet_size(&s_ep_desc) < NXK_PACKET_MAX ? tu_edpt_packet_size(&s_ep_desc) : NXK_PACKET_MAX,
        .buffer = s_in_buf,
        .complete_cb = read_complete,
        .user_data = 0,
    };
    if (!tuh_edpt_xfer(&xfer))
    {
        log_printf("nxk host: endpoint read could not be queued");
    }
}

static void read_complete(tuh_xfer_t *xfer)
{
    if (xfer->result == XFER_RESULT_SUCCESS && xfer->actual_len > 0)
    {
        nxk_packet_t pkt;
        pkt.len = (uint8_t)(xfer->actual_len > NXK_PACKET_MAX ? NXK_PACKET_MAX : xfer->actual_len);
        memcpy(pkt.data, s_in_buf, pkt.len);
        if (queue_try_add(&s_queue, &pkt))
        {
            s_packets++;
        }
        else
        {
            s_dropped++;
        }
    }
    else if (xfer->result != XFER_RESULT_SUCCESS)
    {
        log_printf("nxk host: read failed (result %u)", (unsigned)xfer->result);
    }

    if (s_connected && s_daddr == xfer->daddr)
    {
        start_read();
    }
}

static void interface_set_complete(tuh_xfer_t *xfer)
{
    if (xfer != NULL && xfer->result != XFER_RESULT_SUCCESS)
    {
        log_printf("nxk host: SET_INTERFACE alt %u failed (result %u)", NXK_ALT_SETTING, (unsigned)xfer->result);
        return;
    }
    if (!tuh_edpt_open(s_daddr, &s_ep_desc))
    {
        log_printf("nxk host: could not open endpoint 0x%02x", s_ep_desc.bEndpointAddress);
        return;
    }
    s_connected = true;
    log_printf("nxk host: NX-K ready on endpoint 0x%02x (%u byte packets)", s_ep_desc.bEndpointAddress,
               (unsigned)tu_edpt_packet_size(&s_ep_desc));
    start_read();
}

static bool find_endpoint(const uint8_t *desc, uint16_t total_len)
{
    const uint8_t *p = desc;
    const uint8_t *end = desc + total_len;
    bool in_target_itf = false;

    // Skip the configuration descriptor itself.
    p += tu_desc_len(p);
    while (p < end)
    {
        uint8_t type = tu_desc_type(p);
        if (type == TUSB_DESC_INTERFACE)
        {
            const tusb_desc_interface_t *itf = (const tusb_desc_interface_t *)p;
            in_target_itf = itf->bInterfaceNumber == NXK_INTERFACE && itf->bAlternateSetting == NXK_ALT_SETTING;
        }
        else if (type == TUSB_DESC_ENDPOINT && in_target_itf)
        {
            const tusb_desc_endpoint_t *ep = (const tusb_desc_endpoint_t *)p;
            if (ep->bEndpointAddress == NXK_ENDPOINT_IN)
            {
                memcpy(&s_ep_desc, ep, sizeof(s_ep_desc));
                return true;
            }
        }
        p = tu_desc_next(p);
    }
    return false;
}

static void config_desc_complete(tuh_xfer_t *xfer)
{
    if (xfer->result != XFER_RESULT_SUCCESS)
    {
        log_printf("nxk host: GET_DESCRIPTOR(configuration) failed (result %u)", (unsigned)xfer->result);
        return;
    }
    const tusb_desc_configuration_t *cfg = (const tusb_desc_configuration_t *)s_cfg_desc;
    uint16_t total = tu_le16toh(cfg->wTotalLength);
    if (total > sizeof(s_cfg_desc))
    {
        total = sizeof(s_cfg_desc);
    }
    if (!find_endpoint(s_cfg_desc, total))
    {
        log_printf("nxk host: interface %u alt %u endpoint 0x%02x not found in %u byte descriptor", NXK_INTERFACE,
                   NXK_ALT_SETTING, NXK_ENDPOINT_IN, (unsigned)total);
        return;
    }
    s_have_ep = true;

    if (NXK_ALT_SETTING != 0)
    {
        if (!tuh_interface_set(s_daddr, NXK_INTERFACE, NXK_ALT_SETTING, interface_set_complete, 0))
        {
            log_printf("nxk host: SET_INTERFACE could not be queued");
        }
    }
    else
    {
        interface_set_complete(NULL);
    }
}

void tuh_mount_cb(uint8_t daddr)
{
    uint16_t vid = 0, pid = 0;
    tuh_vid_pid_get(daddr, &vid, &pid);
    log_printf("usb host: device %04x:%04x at address %u", vid, pid, daddr);

    if (vid != NXK_VID || pid != NXK_PID)
    {
        log_printf("usb host: not an NX-K, ignoring");
        return;
    }

    s_daddr = daddr;
    s_have_ep = false;
    if (!tuh_descriptor_get_configuration(daddr, 0, s_cfg_desc, sizeof(s_cfg_desc), config_desc_complete, 0))
    {
        log_printf("nxk host: GET_DESCRIPTOR(configuration) could not be queued");
    }
}

void tuh_umount_cb(uint8_t daddr)
{
    log_printf("usb host: device at address %u removed", daddr);
    if (daddr == s_daddr)
    {
        s_connected = false;
        s_have_ep = false;
        s_daddr = 0;
    }
}

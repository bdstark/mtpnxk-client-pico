#include "otactl_store.h"

#include <string.h>

#include "hardware/flash.h"
#include "hardware/watchdog.h"
#include "pico/stdlib.h"

#include "log.h"

// Mirrors flash_store.c in otactl-boot-pico.
#define FLASH_STORE_MAGIC 0x4F544143u
#define FLASH_STORE_REGION_BYTES (32u * 1024u)
#define FLASH_STORE_SLOT_SIZE_BYTES (12u * 1024u)
#define FLASH_STORE_SLOT_COUNT 2u

// Mirrors runtime_slot.h: watchdog scratch 0 carries the update request
// across a watchdog reboot (and not across a power cycle, by design).
#define RUNTIME_UPDATE_REQUEST_SCRATCH 0u
#define RUNTIME_UPDATE_REQUEST_MAGIC 0xB0075EEDu

typedef struct
{
    uint32_t magic;
    uint32_t schema_version;
    uint32_t sequence;
    uint32_t payload_size;
    uint32_t payload_crc32;
} flash_slot_header_t;

static uint32_t crc32_calc(const uint8_t *data, size_t len)
{
    uint32_t crc = 0xFFFFFFFFu;
    for (size_t i = 0; i < len; ++i)
    {
        crc ^= data[i];
        for (uint32_t bit = 0; bit < 8u; ++bit)
        {
            uint32_t mask = (uint32_t) - (int32_t)(crc & 1u);
            crc = (crc >> 1u) ^ (0xEDB88320u & mask);
        }
    }
    return ~crc;
}

static uintptr_t slot_xip_addr(uint32_t slot_index)
{
    return (uintptr_t)XIP_BASE + (uint32_t)PICO_FLASH_SIZE_BYTES - FLASH_STORE_REGION_BYTES +
           slot_index * FLASH_STORE_SLOT_SIZE_BYTES;
}

// Returns the slot's sequence number when valid, 0 otherwise.
static uint32_t slot_sequence(uint32_t slot_index)
{
    const flash_slot_header_t *hdr = (const flash_slot_header_t *)slot_xip_addr(slot_index);
    if (hdr->magic != FLASH_STORE_MAGIC || hdr->schema_version != OTACTL_FLASH_STORE_SCHEMA_VERSION ||
        hdr->payload_size != sizeof(otactl_config_t))
    {
        return 0;
    }
    const uint8_t *payload = (const uint8_t *)(slot_xip_addr(slot_index) + sizeof(flash_slot_header_t));
    if (crc32_calc(payload, sizeof(otactl_config_t)) != hdr->payload_crc32)
    {
        return 0;
    }
    return hdr->sequence == 0 ? 1u : hdr->sequence;
}

bool otactl_store_load(otactl_config_t *out)
{
    uint32_t best_slot = 0;
    uint32_t best_seq = 0;
    for (uint32_t i = 0; i < FLASH_STORE_SLOT_COUNT; ++i)
    {
        uint32_t seq = slot_sequence(i);
        if (seq > best_seq)
        {
            best_seq = seq;
            best_slot = i;
        }
    }
    if (best_seq == 0)
    {
        return false;
    }
    const uint8_t *payload = (const uint8_t *)(slot_xip_addr(best_slot) + sizeof(flash_slot_header_t));
    memcpy(out, payload, sizeof(*out));
    // Belt and braces: the strings are fixed-width and should be terminated,
    // but nothing downstream should depend on the bootstrap having done so.
    out->device_id[OTACTL_DEVICE_ID_MAX - 1] = '\0';
    out->wifi_ssid[OTACTL_WIFI_SSID_MAX - 1] = '\0';
    out->wifi_password[OTACTL_WIFI_PASS_MAX - 1] = '\0';
    out->runtime_options_form_data[OTACTL_RUNTIME_OPTIONS_MAX - 1] = '\0';
    return true;
}

static int hexval(char c)
{
    if (c >= '0' && c <= '9')
        return c - '0';
    if (c >= 'a' && c <= 'f')
        return c - 'a' + 10;
    if (c >= 'A' && c <= 'F')
        return c - 'A' + 10;
    return -1;
}

static void url_decode(const char *src, size_t src_len, char *out, size_t out_size)
{
    size_t o = 0;
    for (size_t i = 0; i < src_len && o + 1 < out_size; ++i)
    {
        char c = src[i];
        if (c == '+')
        {
            out[o++] = ' ';
        }
        else if (c == '%' && i + 2 < src_len && hexval(src[i + 1]) >= 0 && hexval(src[i + 2]) >= 0)
        {
            out[o++] = (char)((hexval(src[i + 1]) << 4) | hexval(src[i + 2]));
            i += 2;
        }
        else
        {
            out[o++] = c;
        }
    }
    out[o] = '\0';
}

bool otactl_store_option(const otactl_config_t *cfg, const char *key, char *out, size_t out_size)
{
    const char *p = cfg->runtime_options_form_data;
    size_t key_len = strlen(key);
    while (*p != '\0')
    {
        const char *amp = strchr(p, '&');
        size_t pair_len = amp ? (size_t)(amp - p) : strlen(p);
        const char *eq = memchr(p, '=', pair_len);
        size_t k_len = eq ? (size_t)(eq - p) : pair_len;
        if (k_len == key_len && memcmp(p, key, key_len) == 0)
        {
            if (eq)
            {
                url_decode(eq + 1, pair_len - k_len - 1, out, out_size);
            }
            else if (out_size > 0)
            {
                out[0] = '\0';
            }
            return true;
        }
        if (!amp)
        {
            break;
        }
        p = amp + 1;
    }
    return false;
}

void otactl_request_update_and_reboot(void)
{
    log_printf("otactl: requesting update, rebooting into the bootstrap");
    watchdog_hw->scratch[RUNTIME_UPDATE_REQUEST_SCRATCH] = RUNTIME_UPDATE_REQUEST_MAGIC;
    sleep_ms(50);
    watchdog_reboot(0, 0, 0);
    while (true)
    {
        tight_loop_contents();
    }
}

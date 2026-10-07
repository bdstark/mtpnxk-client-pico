#ifndef MTPNXK_OTACTL_STORE_H
#define MTPNXK_OTACTL_STORE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// Read-only view of the otactl bootstrap's flash store, plus the update
// handoff. The on-flash layout is owned by otactl-boot-pico
// (include/flash_store.h, src/flash_store.c, include/runtime_slot.h); the
// struct below must stay byte-identical to flash_config_t at
// FLASH_STORE_SCHEMA_VERSION 4, which is why the deprecated fields are kept.

#define OTACTL_WIFI_SSID_MAX 32
#define OTACTL_WIFI_PASS_MAX 64
#define OTACTL_DEVICE_ID_MAX 64
#define OTACTL_DEVICE_NAME_MAX 64
#define OTACTL_TOKEN_MAX 128
#define OTACTL_RUNTIME_APP_MAX 64
#define OTACTL_RUNTIME_ARCH_MAX 32
#define OTACTL_RUNTIME_VERSION_MAX 64
#define OTACTL_RUNTIME_OPTIONS_MAX 1024
#define OTACTL_CERT_PEM_MAX 4096
#define OTACTL_CHAIN_PEM_MAX 4096

#define OTACTL_FLASH_STORE_SCHEMA_VERSION 4u

typedef enum
{
    OTACTL_ENROLL_RECORD_NONE = 0,
    OTACTL_ENROLL_RECORD_PENDING,
    OTACTL_ENROLL_RECORD_COMMITTED
} otactl_enroll_record_state_t;

typedef struct
{
    uint32_t version;
    char device_id[OTACTL_DEVICE_ID_MAX];
    char device_name[OTACTL_DEVICE_NAME_MAX];
    char setup_ap_password_deprecated[OTACTL_WIFI_PASS_MAX];
    bool setup_ap_password_present_deprecated;
    char wifi_ssid[OTACTL_WIFI_SSID_MAX];
    char wifi_password[OTACTL_WIFI_PASS_MAX];
    char runtime_app[OTACTL_RUNTIME_APP_MAX];
    char runtime_arch[OTACTL_RUNTIME_ARCH_MAX];
    char runtime_version[OTACTL_RUNTIME_VERSION_MAX];
    char runtime_options_form_data[OTACTL_RUNTIME_OPTIONS_MAX]; // application/x-www-form-urlencoded
    bool runtime_selection_confirmed;
    char enrollment_token[OTACTL_TOKEN_MAX];
    bool enrollment_token_present;
    uint8_t p256_private_key[32];
    bool private_key_present;
    char leaf_cert_pem[OTACTL_CERT_PEM_MAX];
    bool leaf_cert_present;
    char chain_cert_pem[OTACTL_CHAIN_PEM_MAX];
    bool chain_cert_present;
    otactl_enroll_record_state_t enroll_record_state;
    uint32_t crc32;
} otactl_config_t;

// Loads the newest valid config slot. ~9.8 KB: the caller's buffer must not
// be on the default 2 KB stack. Returns false when no slot is valid.
bool otactl_store_load(otactl_config_t *out);

// Looks up one key of the options form data (URL-decoded). Returns false
// when the key is absent.
bool otactl_store_option(const otactl_config_t *cfg, const char *key, char *out, size_t out_size);

// Asks the bootstrap to install the current manifest on the next boot and
// reboots. Does not return.
void otactl_request_update_and_reboot(void) __attribute__((noreturn));

#endif

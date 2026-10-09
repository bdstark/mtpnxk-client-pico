#include "hid_kbd.h"

#include <string.h>

#include "pico/util/queue.h"
#include "tusb.h"

#include "log.h"

typedef struct
{
    uint8_t modifier;
    uint8_t keycode;
} kbd_report_t;

#define KBD_QUEUE_DEPTH 128

static queue_t s_queue;
static uint32_t s_dropped;

// {shift, keycode} per ASCII character, from TinyUSB's hid.h.
static uint8_t const s_ascii_to_keycode[128][2] = {HID_ASCII_TO_KEYCODE};

void hid_kbd_init(void)
{
    queue_init(&s_queue, sizeof(kbd_report_t), KBD_QUEUE_DEPTH);
}

static bool enqueue(uint8_t modifier, uint8_t keycode)
{
    kbd_report_t rep = {.modifier = modifier, .keycode = keycode};
    if (!queue_try_add(&s_queue, &rep))
    {
        s_dropped++;
        return false;
    }
    return true;
}

bool hid_kbd_press(uint8_t modifier, uint8_t keycode)
{
    return enqueue(modifier, keycode);
}

bool hid_kbd_release(void)
{
    return enqueue(0, 0);
}

bool hid_kbd_type(const char *text)
{
    bool ok = true;
    for (const char *p = text; *p != '\0'; ++p)
    {
        uint8_t ch = (uint8_t)*p;
        if (ch >= 128)
        {
            continue;
        }
        uint8_t keycode = s_ascii_to_keycode[ch][1];
        uint8_t modifier = s_ascii_to_keycode[ch][0] ? KEYBOARD_MODIFIER_LEFTSHIFT : 0;
        if (keycode == 0)
        {
            continue;
        }
        ok = enqueue(modifier, keycode) && ok;
        ok = enqueue(0, 0) && ok;
    }
    return ok;
}

void hid_kbd_task(void)
{
    if (!tud_hid_ready())
    {
        return;
    }
    kbd_report_t rep;
    if (!queue_try_remove(&s_queue, &rep))
    {
        return;
    }
    uint8_t keycodes[6] = {rep.keycode, 0, 0, 0, 0, 0};
    if (!tud_hid_keyboard_report(0, rep.modifier, rep.keycode ? keycodes : NULL))
    {
        s_dropped++;
    }
}

uint32_t hid_kbd_dropped(void)
{
    return s_dropped;
}

bool hid_kbd_mounted(void)
{
    return tud_mounted();
}

// Device stack callbacks ------------------------------------------------------

void tud_mount_cb(void)
{
    log_printf("usb device: mounted by lighting PC");
}

void tud_umount_cb(void)
{
    log_printf("usb device: unmounted");
}

void tud_suspend_cb(bool remote_wakeup_en)
{
    (void)remote_wakeup_en;
    log_printf("usb device: suspended");
}

void tud_resume_cb(void)
{
    log_printf("usb device: resumed");
}

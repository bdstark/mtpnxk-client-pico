#ifndef MTPNXK_HID_KBD_H
#define MTPNXK_HID_KBD_H

#include <stdbool.h>
#include <stdint.h>

// Queued keyboard reports towards the lighting PC. Every call enqueues whole
// reports; hid_kbd_task() drains the queue one report per HID interval, so a
// burst of text never outruns the endpoint.

void hid_kbd_init(void);
void hid_kbd_task(void);

// Hold a key (with modifier) until hid_kbd_release() sends the empty report.
bool hid_kbd_press(uint8_t modifier, uint8_t keycode);
bool hid_kbd_release(void);

// Type ASCII text as press/release pairs using the US layout.
bool hid_kbd_type(const char *text);

uint32_t hid_kbd_dropped(void);
bool hid_kbd_mounted(void);

#endif

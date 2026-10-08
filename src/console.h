#ifndef MTPNXK_CONSOLE_H
#define MTPNXK_CONSOLE_H

// Bench console: single-character commands on the UART and on the USB CDC
// port; log lines are mirrored to the CDC port from core 0.

typedef void (*console_handler_t)(int c);

void console_init(console_handler_t handler);
void console_task(void);

#endif

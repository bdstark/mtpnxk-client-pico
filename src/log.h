#ifndef MTPNXK_LOG_H
#define MTPNXK_LOG_H

#include <stdint.h>

// Timestamped lines to stdio (UART0). Kept in a ring buffer as well so a
// later web/MQTT log sink can replay recent history, following rotec.

#define LOG_MAX_LINES 64
#define LOG_MAX_LINE_LEN 128

void log_init(void);
void log_printf(const char *fmt, ...) __attribute__((format(printf, 1, 2)));

// Monotonic count of lines logged; line i lives at index i % LOG_MAX_LINES
// until overwritten.
uint32_t log_count(void);
const char *log_line(uint32_t index);

#endif

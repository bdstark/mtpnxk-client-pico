#include "log.h"

#include <stdarg.h>
#include <stdio.h>
#include <string.h>

#include "pico/stdlib.h"
#include "pico/sync.h"

static char s_lines[LOG_MAX_LINES][LOG_MAX_LINE_LEN];
static volatile uint32_t s_count;
static critical_section_t s_lock;

void log_init(void)
{
    critical_section_init(&s_lock);
    s_count = 0;
}

void log_printf(const char *fmt, ...)
{
    char buf[LOG_MAX_LINE_LEN];
    uint32_t ms = to_ms_since_boot(get_absolute_time());
    int n = snprintf(buf, sizeof(buf), "[%6lu.%03lu] ", (unsigned long)(ms / 1000u), (unsigned long)(ms % 1000u));
    if (n < 0)
    {
        n = 0;
    }
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(buf + n, sizeof(buf) - (size_t)n, fmt, ap);
    va_end(ap);

    critical_section_enter_blocking(&s_lock);
    uint32_t idx = s_count % LOG_MAX_LINES;
    memcpy(s_lines[idx], buf, LOG_MAX_LINE_LEN);
    s_count++;
    critical_section_exit(&s_lock);

    puts(buf);
}

uint32_t log_count(void)
{
    return s_count;
}

const char *log_line(uint32_t index)
{
    return s_lines[index % LOG_MAX_LINES];
}

// TinyUSB debug output arrives in fragments; assemble into lines.
int mtpnxk_tusb_printf(const char *fmt, ...)
{
    static char line[LOG_MAX_LINE_LEN];
    static size_t len;
    char buf[LOG_MAX_LINE_LEN];
    va_list ap;
    va_start(ap, fmt);
    int n = vsnprintf(buf, sizeof(buf), fmt, ap);
    va_end(ap);
    for (int i = 0; i < n && buf[i] != '\0'; ++i)
    {
        char c = buf[i];
        if (c == '\r')
            continue;
        if (c == '\n' || len + 1 >= sizeof(line))
        {
            line[len] = '\0';
            // Drop the device-side chatter: logging our own console writes
            // would feed back into more console writes.
            if (len > 0 && strncmp(line, "USBD", 4) != 0 && strncmp(line, "  Queue EP", 10) != 0 &&
                strncmp(line, "  CDC", 5) != 0 && strncmp(line, "  HID", 5) != 0 && strncmp(line, "HID ", 4) != 0 &&
                strncmp(line, "  Get Descriptor", 16) != 0)
                log_printf("tusb: %s", line);
            len = 0;
            continue;
        }
        line[len++] = c;
    }
    return n;
}

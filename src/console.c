#include "console.h"

#include <string.h>

#include "pico/stdlib.h"
#include "tusb.h"

#include "log.h"

static console_handler_t s_handler;
static uint32_t s_cdc_sent; // log lines already written to CDC

void console_init(console_handler_t handler)
{
    s_handler = handler;
    s_cdc_sent = log_count();
}

static void mirror_log_to_cdc(void)
{
    if (!tud_cdc_connected())
    {
        s_cdc_sent = log_count();
        return;
    }
    uint32_t count = log_count();
    if (count - s_cdc_sent > LOG_MAX_LINES)
    {
        s_cdc_sent = count - LOG_MAX_LINES; // ring overrun: skip what is gone
    }
    while (s_cdc_sent < count)
    {
        const char *line = log_line(s_cdc_sent);
        size_t len = strlen(line);
        if (tud_cdc_write_available() < len + 2)
        {
            break; // try again next loop
        }
        tud_cdc_write(line, len);
        tud_cdc_write("\r\n", 2);
        s_cdc_sent++;
    }
    tud_cdc_write_flush();
}

void console_task(void)
{
    mirror_log_to_cdc();

    int c = getchar_timeout_us(0);
    if (c != PICO_ERROR_TIMEOUT && s_handler)
    {
        s_handler(c);
    }
    if (tud_cdc_connected() && tud_cdc_available())
    {
        uint8_t buf[16];
        uint32_t n = tud_cdc_read(buf, sizeof(buf));
        for (uint32_t i = 0; i < n && s_handler; ++i)
        {
            s_handler(buf[i]);
        }
    }
}

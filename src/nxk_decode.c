#include "nxk_decode.h"

typedef struct
{
    uint8_t event_class;
    uint8_t group_id;
    uint8_t control_id;
    const char *name;
} nxk_button_t;

// {eventClass, groupID, controlID}: same triples as the Go table.
static const nxk_button_t s_buttons[] = {
    {66, 82, 0, "0"},
    {66, 82, 1, "1"},
    {66, 82, 2, "2"},
    {66, 82, 3, "3"},
    {66, 82, 4, "4"},
    {66, 82, 5, "5"},
    {66, 82, 6, "6"},
    {66, 82, 7, "7"},
    {66, 82, 8, "8"},
    {66, 82, 9, "9"},
    {66, 82, 18, "."},
    {66, 82, 19, "Enter"},
    {66, 82, 20, "/"},
    {66, 82, 16, "-"},
    {66, 82, 17, "+"},
    {2, 82, 21, "Back"},
    {66, 83, 2, "Thru"},
    {66, 83, 1, "Full"},
    {66, 82, 22, "@"},
    {2, 84, 19, "Cue"},
    {2, 84, 18, "Group"},
    {2, 84, 17, "Load"},
    {2, 84, 2, "Update"},
    {2, 84, 1, "Record"},
    {2, 81, 1, "Edit"},
    {2, 81, 2, "Undo"},
    {2, 81, 3, "Clear"},
    {2, 81, 4, "Copy"},
    {2, 81, 6, "Move"},
    {2, 81, 7, "Delete"},
    {2, 32, 3, "Menu"},
    {2, 32, 1, "Macro"},
    {2, 67, 49, "Snap Shot"},
    {2, 67, 50, "Bank"},
    {2, 32, 2, "Preview"},
    {2, 96, 1, "HighLight"},
    {2, 67, 33, "Fade"},
    {2, 67, 34, "Delay"},
    {2, 100, 17, "Swap Prog"},
    {2, 97, 8, "Link"},
    {2, 100, 1, "Last"},
    {2, 100, 2, "Next"},
};

static const char *encoder_rotate_name(uint8_t control_id)
{
    switch (control_id)
    {
    case 2:
        return "Rotary1";
    case 18:
        return "Rotary2";
    case 34:
        return "Rotary3";
    case 50:
        return "Rotary4";
    default:
        return NULL;
    }
}

static const char *encoder_press_name(uint8_t control_id)
{
    switch (control_id)
    {
    case 1:
        return "Rotary1";
    case 17:
        return "Rotary2";
    case 33:
        return "Rotary3";
    case 49:
        return "Rotary4";
    default:
        return NULL;
    }
}

static int signed16(uint8_t lo, uint8_t hi)
{
    int value = (int)lo | ((int)hi << 8);
    if (value >= 0x8000)
    {
        value -= 0x10000;
    }
    return value;
}

bool nxk_decode(const uint8_t *data, size_t len, nxk_event_t *out)
{
    out->type = NXK_EVENT_UNKNOWN;
    out->name = NULL;
    out->delta = 0;
    out->id = 0;

    if (len == 0)
    {
        return false;
    }

    if (len == 5 && data[0] == 1)
    {
        uint8_t event_class = data[1];
        uint8_t control_id = data[2];
        uint8_t group_id = data[3];
        uint8_t state = data[4];
        out->id = NXK_CONTROL_ID(group_id, control_id);

        if (event_class == 2 && group_id == 89)
        {
            const char *name = encoder_press_name(control_id);
            if (name != NULL)
            {
                out->type = state != 0 ? NXK_EVENT_PRESS_DOWN : NXK_EVENT_PRESS_UP;
                out->name = name;
                return true;
            }
        }

        for (size_t i = 0; i < sizeof(s_buttons) / sizeof(s_buttons[0]); ++i)
        {
            const nxk_button_t *b = &s_buttons[i];
            if (b->event_class == event_class && b->group_id == group_id && b->control_id == control_id)
            {
                out->type = state != 0 ? NXK_EVENT_KEY_DOWN : NXK_EVENT_KEY_UP;
                out->name = b->name;
                return true;
            }
        }
        return true;
    }

    if (len == 7 && data[0] == 2 && data[1] == 66 && data[3] == 89)
    {
        const char *name = encoder_rotate_name(data[2]);
        out->id = NXK_CONTROL_ID(data[3], data[2]);
        if (name != NULL)
        {
            out->type = NXK_EVENT_ROTATE;
            out->name = name;
            out->delta = signed16(data[4], data[5]);
        }
        return true;
    }

    return true;
}

size_t nxk_button_ids(uint16_t *out, size_t max)
{
    size_t n = sizeof(s_buttons) / sizeof(s_buttons[0]);
    for (size_t i = 0; i < n && i < max; ++i)
    {
        out[i] = NXK_CONTROL_ID(s_buttons[i].group_id, s_buttons[i].control_id);
    }
    return n;
}

const char *nxk_event_type_name(nxk_event_type_t type)
{
    switch (type)
    {
    case NXK_EVENT_KEY_DOWN:
        return "KeyDown";
    case NXK_EVENT_KEY_UP:
        return "KeyUp";
    case NXK_EVENT_PRESS_DOWN:
        return "PressDown";
    case NXK_EVENT_PRESS_UP:
        return "PressUp";
    case NXK_EVENT_ROTATE:
        return "Rotate";
    default:
        return "Unknown";
    }
}

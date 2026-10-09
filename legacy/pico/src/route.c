#include "route.h"

#include <stdio.h>
#include <string.h>

#include "class/hid/hid.h"

#include "hid_kbd.h"
#include "log.h"
#include "osc.h"

// Keypad keys sent as keystrokes (mixedKeyboardOverrides + defaultKeyboardKeys
// in nxk). HID usages replace the macOS virtual key codes.
typedef struct
{
    const char *name;
    uint8_t modifier;
    uint8_t keycode;
    const char *text; // typed instead of a single key when set
} key_binding_t;

static const key_binding_t s_keys[] = {
    {"0", 0, HID_KEY_KEYPAD_0, NULL},
    {"1", 0, HID_KEY_KEYPAD_1, NULL},
    {"2", 0, HID_KEY_KEYPAD_2, NULL},
    {"3", 0, HID_KEY_KEYPAD_3, NULL},
    {"4", 0, HID_KEY_KEYPAD_4, NULL},
    {"5", 0, HID_KEY_KEYPAD_5, NULL},
    {"6", 0, HID_KEY_KEYPAD_6, NULL},
    {"7", 0, HID_KEY_KEYPAD_7, NULL},
    {"8", 0, HID_KEY_KEYPAD_8, NULL},
    {"9", 0, HID_KEY_KEYPAD_9, NULL},
    {".", 0, HID_KEY_KEYPAD_DECIMAL, NULL},
    {"Enter", 0, HID_KEY_KEYPAD_ENTER, NULL},
    {"/", 0, HID_KEY_KEYPAD_DIVIDE, NULL},
    {"-", 0, HID_KEY_KEYPAD_SUBTRACT, NULL},
    {"+", 0, HID_KEY_KEYPAD_ADD, NULL},
    {"Back", 0, HID_KEY_BACKSPACE, NULL},
    {"@", KEYBOARD_MODIFIER_LEFTSHIFT, HID_KEY_2, NULL},
    {"Thru", 0, 0, " Thru "},
    {"Full", 0, 0, " Full "},
};

// Command-line buttons: "/cmd" with one string on key down (defaultOSCCommands).
typedef struct
{
    const char *name;
    const char *command;
} command_binding_t;

static const command_binding_t s_commands[] = {
    {"HighLight", "Highlight"},
    {"Preview", "Preview"},
    {"Clear", "Macro 9950"},
    {"Undo", "Undo"},
    {"Next", "Next"},
    {"Last", "Last"},
    {"Load", "Load "},
    {"Delete", "Delete "},
    {"Copy", "Copy "},
    {"Move", "Move "},
    {"Edit", "Edit "},
    {"Update", "Update "},
    {"Fade", "Fade "},
    {"Delay", "Delay "},
    {"Menu", "Menu"},
    {"Macro", "Macro "},
    {"Snap Shot", "Snapshot"},
    // Intentionally unmapped: Swap Prog, Link. Bank is the wheel modifier.
};

// Key-style buttons: integer 1 on down, 0 on up (the OSC half of
// defaultOSCKeys; the keypad half is handled by s_keys above).
typedef struct
{
    const char *name;
    const char *path;
} key_path_binding_t;

static const key_path_binding_t s_osc_keys[] = {
    {"Cue", "/key/Cue"},
    {"Group", "/key/Group"},
    {"Record", "/key/Store"},
    {"Rotary1", "/key/Wheel1"},
    {"Rotary2", "/key/Wheel2"},
    {"Rotary3", "/key/Wheel3"},
    {"Rotary4", "/key/Wheel4"},
};

static bool s_bank_held;

void route_init(void)
{
    s_bank_held = false;
}

static const key_binding_t *find_key(const char *name)
{
    for (size_t i = 0; i < sizeof(s_keys) / sizeof(s_keys[0]); ++i)
    {
        if (strcmp(s_keys[i].name, name) == 0)
            return &s_keys[i];
    }
    return NULL;
}

static const char *find_command(const char *name)
{
    for (size_t i = 0; i < sizeof(s_commands) / sizeof(s_commands[0]); ++i)
    {
        if (strcmp(s_commands[i].name, name) == 0)
            return s_commands[i].command;
    }
    return NULL;
}

static const char *find_osc_key(const char *name)
{
    for (size_t i = 0; i < sizeof(s_osc_keys) / sizeof(s_osc_keys[0]); ++i)
    {
        if (strcmp(s_osc_keys[i].name, name) == 0)
            return s_osc_keys[i].path;
    }
    return NULL;
}

static bool is_rotary(const char *name)
{
    return strncmp(name, "Rotary", 6) == 0;
}

static void route_wheel(const nxk_event_t *evt)
{
    // "Rotary1".."Rotary4" -> /wheel/1../wheel/4, or /wheel/101.. with Bank held.
    char path[16];
    snprintf(path, sizeof(path), "/wheel/%s%c", s_bank_held ? "10" : "", evt->name[6]);
    osc_send_float(path, (float)evt->delta);
}

void route_event(const nxk_event_t *evt)
{
    if (evt->type == NXK_EVENT_UNKNOWN || evt->name == NULL)
    {
        return;
    }

    if (strcmp(evt->name, "Bank") == 0)
    {
        if (evt->type == NXK_EVENT_KEY_DOWN)
            s_bank_held = true;
        else if (evt->type == NXK_EVENT_KEY_UP)
            s_bank_held = false;
        return;
    }

    if (evt->type == NXK_EVENT_ROTATE)
    {
        if (is_rotary(evt->name))
        {
            route_wheel(evt);
        }
        return;
    }

    // Keypad keys -> keystrokes.
    const key_binding_t *key = find_key(evt->name);
    if (key != NULL && (evt->type == NXK_EVENT_KEY_DOWN || evt->type == NXK_EVENT_KEY_UP))
    {
        if (key->text != NULL)
        {
            if (evt->type == NXK_EVENT_KEY_DOWN)
                hid_kbd_type(key->text);
        }
        else if (evt->type == NXK_EVENT_KEY_DOWN)
        {
            hid_kbd_press(key->modifier, key->keycode);
        }
        else
        {
            hid_kbd_release();
        }
        return;
    }

    // Key-style OSC buttons and encoder presses.
    const char *path = find_osc_key(evt->name);
    if (path != NULL)
    {
        if (s_bank_held && is_rotary(evt->name) &&
            (evt->type == NXK_EVENT_PRESS_DOWN || evt->type == NXK_EVENT_PRESS_UP))
        {
            return;
        }
        switch (evt->type)
        {
        case NXK_EVENT_KEY_DOWN:
        case NXK_EVENT_PRESS_DOWN:
            osc_send_int32(path, 1);
            break;
        case NXK_EVENT_KEY_UP:
        case NXK_EVENT_PRESS_UP:
            osc_send_int32(path, 0);
            break;
        default:
            break;
        }
        return;
    }

    // Command buttons fire on key down only.
    if (evt->type != NXK_EVENT_KEY_DOWN)
    {
        return;
    }
    const char *command = find_command(evt->name);
    if (command != NULL)
    {
        osc_send_string("/cmd", command);
    }
}

using System.Collections.Generic;
using System.Linq;

namespace DXO.UI.ViewModels;

public sealed record KeyLabel(string Key, string Label);
public sealed record VkLabel(ushort Vk, string Label);

/// Names shared by the Mappings page and the Profiles editor. Button keys are
/// the engine's (mapping.rs `button_from_name`); key codes are Windows virtual keys.
public static class ButtonCatalog
{
    public static readonly KeyLabel[] Buttons =
    {
        new("cross", "Cross / A"), new("circle", "Circle / B"), new("square", "Square / X"), new("triangle", "Triangle / Y"),
        new("l1", "L1 / LB"), new("r1", "R1 / RB"), new("l2", "L2 / LT"), new("r2", "R2 / RT"),
        new("l3", "L3 / left stick click"), new("r3", "R3 / right stick click"),
        new("share", "Share / Back"), new("options", "Options / Start"), new("ps", "PS / Guide"), new("touchpad", "Touchpad click"),
        new("up", "D-pad up"), new("down", "D-pad down"), new("left", "D-pad left"), new("right", "D-pad right"),
        new("mute", "Mute (DualSense)"), new("paddle_l", "Left paddle (Edge)"), new("paddle_r", "Right paddle (Edge)"),
    };

    public static readonly KeyLabel[] Mouse =
    {
        new("left", "Left click"), new("right", "Right click"), new("middle", "Middle click"),
        new("back", "Side button: back"), new("forward", "Side button: forward"),
        new("wheel_up", "Scroll up"), new("wheel_down", "Scroll down"),
    };

    public static readonly VkLabel[] Keys = BuildKeys();

    private static VkLabel[] BuildKeys()
    {
        var k = new List<VkLabel>
        {
            new(0x20, "Space"), new(0x0D, "Enter"), new(0x1B, "Esc"), new(0x09, "Tab"), new(0x08, "Backspace"),
            new(0xA0, "Left Shift"), new(0xA2, "Left Ctrl"), new(0xA4, "Left Alt"),
            new(0xA1, "Right Shift"), new(0xA3, "Right Ctrl"), new(0xA5, "Right Alt"),
            new(0x26, "Arrow up"), new(0x28, "Arrow down"), new(0x25, "Arrow left"), new(0x27, "Arrow right"),
        };
        for (char c = 'A'; c <= 'Z'; c++) k.Add(new((ushort)c, c.ToString()));
        for (char c = '0'; c <= '9'; c++) k.Add(new((ushort)c, c.ToString()));
        for (int i = 1; i <= 12; i++) k.Add(new((ushort)(0x6F + i), $"F{i}"));
        k.AddRange(new VkLabel[]
        {
            new(0x14, "Caps Lock"), new(0x2D, "Insert"), new(0x2E, "Delete"), new(0x24, "Home"), new(0x23, "End"),
            new(0x21, "Page Up"), new(0x22, "Page Down"), new(0x2C, "Print Screen"), new(0x5B, "Windows key"),
            new(0xC0, "` (backtick)"), new(0xBD, "- (minus)"), new(0xBB, "= (equals)"), new(0xDB, "["), new(0xDD, "]"),
            new(0xBA, ";"), new(0xDE, "' (quote)"), new(0xBC, ", (comma)"), new(0xBE, ". (full stop)"), new(0xBF, "/"), new(0xDC, "\\"),
        });
        for (int i = 0; i <= 9; i++) k.Add(new((ushort)(0x60 + i), $"Numpad {i}"));
        k.AddRange(new VkLabel[]
        {
            new(0xAF, "Volume up"), new(0xAE, "Volume down"), new(0xAD, "Volume mute"),
            new(0xB3, "Media play/pause"), new(0xB0, "Media next"), new(0xB1, "Media previous"),
        });
        return k.ToArray();
    }

    public static string ButtonLabel(string key) => Buttons.FirstOrDefault(b => b.Key == key)?.Label ?? key;
    public static string KeyName(ushort vk) => Keys.FirstOrDefault(k => k.Vk == vk)?.Label ?? $"key 0x{vk:X2}";
    public static string MouseLabel(string? key) => Mouse.FirstOrDefault(m => m.Key == key)?.Label ?? key ?? "?";

    /// "Space key", "Left click (turbo)", ... or null if the binding changes nothing.
    public static string? Describe(BindingModel b)
    {
        string? what = b.Action.Type switch
        {
            "button" => ButtonLabel(b.Action.To ?? ""),
            "key" => b.Action.Vk is ushort vk ? $"{KeyName(vk)} key" : null,
            "mouse" => MouseLabel(b.Action.Button),
            "disabled" => "Nothing (disabled)",
            _ => null,
        };
        var extras = new List<string>();
        if (b.Turbo) extras.Add($"turbo {b.TurboHz:0}/s");
        if (b.Toggle) extras.Add("toggle");
        if (what is null && extras.Count == 0) return null;
        what ??= "Itself";
        return extras.Count == 0 ? what : $"{what} ({string.Join(", ", extras)})";
    }

    /// Short summary of a profile's mappings for the Profiles editor.
    public static string Summary(ProfileModel m)
    {
        var parts = m.Bindings.Select(b => (b, d: Describe(b))).Where(x => x.d is not null)
            .Select(x => $"{ButtonLabel(x.b.Button).Split(" /")[0]} → {x.d}")
            .Concat(m.Remap.Where(p => p.Count == 2).Select(p => $"{ButtonLabel(p[0]).Split(" /")[0]} → {ButtonLabel(p[1])}"))
            .ToList();
        if (parts.Count == 0) return "None: every button does what it says.";
        return parts.Count <= 3 ? string.Join(",  ", parts) : string.Join(",  ", parts.Take(3)) + $",  +{parts.Count - 3} more";
    }
}

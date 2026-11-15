using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;

namespace DXO.UI.ViewModels;

/// Rust: chords.rs `SysAction` (#[serde(tag = "type")]): what a chord or a
/// quick-menu item does. Only the fields the chosen type uses are sent.
public sealed class SysActionModel
{
    /// "none" | "power_off" | "volume_up" | "volume_down" | "mute" | "screenshot" |
    /// "toggle_window" | "toggle_keyboard" | "radial_menu" | "media_play_pause" |
    /// "media_next" | "media_prev" | "keys" | "launch"
    public string Type { get; set; } = "none";
    /// "keys": virtual-key codes pressed together.
    public List<int>? Keys { get; set; }
    /// "launch": a program, file, folder or web address.
    public string? Target { get; set; }
    public string? Args { get; set; }

    /// Make sure the fields the engine requires for this type are there, and
    /// the others are not.
    public void Normalize()
    {
        if (Type == "keys") Keys ??= new();
        else Keys = null;
        if (Type == "launch") { Target ??= ""; Args ??= ""; }
        else { Target = null; Args = null; }
    }

    /// The choices the Chords page offers, in order.
    public static readonly (string Type, string Label)[] Choices =
    {
        ("none", "Nothing"),
        ("power_off", "Turn the controller off"),
        ("volume_up", "Volume up"),
        ("volume_down", "Volume down"),
        ("mute", "Mute"),
        ("media_play_pause", "Play / pause"),
        ("media_next", "Next track"),
        ("media_prev", "Previous track"),
        ("screenshot", "Take a screenshot"),
        ("toggle_window", "Show / hide DXO"),
        ("toggle_keyboard", "On-screen keyboard"),
        ("radial_menu", "Quick menu"),
        ("keys", "Press keys..."),
        ("launch", "Open an app, file or link..."),
    };
}

/// Rust: chords.rs `SysBinding`: PS + Button -> Action.
public sealed class SysBindingModel
{
    public string Button { get; set; } = "";
    public SysActionModel Action { get; set; } = new();

    public static List<SysBindingModel> ListFromJson(string json)
    {
        try
        {
            if (!string.IsNullOrWhiteSpace(json))
                return JsonSerializer.Deserialize<List<SysBindingModel>>(json, ProfileModel.Json) ?? new();
        }
        catch (JsonException) { }
        return new();
    }

    public static string ListToJson(IEnumerable<SysBindingModel> list)
    {
        var rows = list.Where(b => b.Action.Type != "none").ToList();
        foreach (var b in rows) b.Action.Normalize();
        return JsonSerializer.Serialize(rows, ProfileModel.Json);
    }
}

/// Rust: radial.rs `RadialItem`: one entry of the quick menu.
public sealed class RadialItemModel
{
    public string Label { get; set; } = "";
    public SysActionModel Action { get; set; } = new();

    public static List<RadialItemModel> ListFromJson(string json)
    {
        try
        {
            if (!string.IsNullOrWhiteSpace(json))
                return JsonSerializer.Deserialize<List<RadialItemModel>>(json, ProfileModel.Json) ?? new();
        }
        catch (JsonException) { }
        return new();
    }

    public static string ListToJson(IEnumerable<RadialItemModel> list)
    {
        var rows = list.Where(i => i.Label.Trim().Length > 0 && i.Action.Type is not ("none" or "radial_menu")).ToList();
        foreach (var i in rows) i.Action.Normalize();
        return JsonSerializer.Serialize(rows, ProfileModel.Json);
    }
}

/// "Ctrl+Shift+Esc" <-> Windows virtual-key codes, for the "Press keys" action.
public static class KeyNames
{
    private static readonly (string Name, int Vk)[] Named =
    {
        ("Ctrl", 0x11), ("Shift", 0x10), ("Alt", 0x12), ("Win", 0x5B),
        ("Esc", 0x1B), ("Tab", 0x09), ("Enter", 0x0D), ("Space", 0x20), ("Backspace", 0x08),
        ("Del", 0x2E), ("Ins", 0x2D), ("Home", 0x24), ("End", 0x23), ("PgUp", 0x21), ("PgDn", 0x22),
        ("Up", 0x26), ("Down", 0x28), ("Left", 0x25), ("Right", 0x27), ("PrtScn", 0x2C),
    };

    private static readonly Dictionary<string, int> Aliases = new(StringComparer.OrdinalIgnoreCase)
    {
        ["control"] = 0x11, ["windows"] = 0x5B, ["escape"] = 0x1B, ["return"] = 0x0D, ["delete"] = 0x2E,
        ["insert"] = 0x2D, ["pageup"] = 0x21, ["pagedown"] = 0x22, ["printscreen"] = 0x2C,
    };

    /// Unknown names are skipped. At most six keys (what the engine presses).
    public static List<int> Parse(string text)
    {
        var keys = new List<int>();
        foreach (var raw in text.Split(new[] { '+', ',', ' ' }, StringSplitOptions.RemoveEmptyEntries))
        {
            string t = raw.Trim();
            int vk = 0;
            foreach (var (name, code) in Named)
                if (name.Equals(t, StringComparison.OrdinalIgnoreCase)) vk = code;
            if (vk == 0 && Aliases.TryGetValue(t, out int alias)) vk = alias;
            if (vk == 0 && t.Length == 1 && char.IsAsciiLetterOrDigit(t[0])) vk = char.ToUpperInvariant(t[0]); // A-Z, 0-9
            if (vk == 0 && t.Length is 2 or 3 && (t[0] is 'F' or 'f') && int.TryParse(t.AsSpan(1), out int f) && f is >= 1 and <= 24)
                vk = 0x70 + f - 1;
            if (vk != 0 && !keys.Contains(vk) && keys.Count < 6) keys.Add(vk);
        }
        return keys;
    }

    public static string Format(IEnumerable<int>? keys)
    {
        if (keys is null) return "";
        return string.Join("+", keys.Select(vk =>
        {
            foreach (var (name, code) in Named)
                if (code == vk) return name;
            if (vk is >= 0x70 and <= 0x87) return "F" + (vk - 0x70 + 1);
            if (vk is (>= 0x30 and <= 0x39) or (>= 0x41 and <= 0x5A)) return ((char)vk).ToString();
            return "0x" + vk.ToString("X2");
        }));
    }
}

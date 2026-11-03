using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;

namespace DXO.UI.ViewModels;

/// C# mirror of dxo-core `ControllerSettings` (controllers.rs): what one
/// physical controller is called, emulates, and which profiles it uses.
public sealed class ControllerModel
{
    public string Name { get; set; } = "";
    /// Profile used outside games; "" = the saved "Default".
    public string DefaultProfile { get; set; } = "";
    /// "xbox360" | "dual_shock4" | "none". Changed by hand only.
    public string Target { get; set; } = "xbox360";
    public bool HidePhysical { get; set; } = true;
    /// exe name -> profile name, for this controller only.
    public Dictionary<string, string> GameProfiles { get; set; } = new();
    /// "" = player colour, "profile" = the profile's colour, "#rrggbb" = fixed.
    public string Lightbar { get; set; } = "";
    /// Bluetooth: turn off after this many idle minutes; 0 = never.
    public uint IdleDisconnectMin { get; set; }

    /// PS4-style player colours (controllers.rs PLAYER_COLOURS).
    public static readonly Windows.UI.Color[] PlayerColours =
    {
        Windows.UI.Color.FromArgb(255, 0, 64, 255), Windows.UI.Color.FromArgb(255, 255, 0, 32),
        Windows.UI.Color.FromArgb(255, 0, 200, 40), Windows.UI.Color.FromArgb(255, 255, 64, 160),
    };

    /// The colour this controller's light shows (mirror of controllers.rs `lightbar_rgb`).
    public static Windows.UI.Color LightColour(ControllerModel? c, int slot, string profileName)
    {
        var player = PlayerColours[Math.Clamp(slot, 0, PlayerColours.Length - 1)];
        if (c is null) return player;
        if (c.Lightbar.Equals("profile", StringComparison.OrdinalIgnoreCase))
        {
            var lb = DXO.UI.Services.ProfileStore.All().FirstOrDefault(p => p.Model.Name == profileName).Model?.Lightbar;
            return lb is { Length: 3 } ? Windows.UI.Color.FromArgb(255, (byte)lb[0], (byte)lb[1], (byte)lb[2]) : player;
        }
        return ParseHex(c.Lightbar) ?? player;
    }

    public static string Hex(Windows.UI.Color c) => $"#{c.R:x2}{c.G:x2}{c.B:x2}";

    public static Windows.UI.Color? ParseHex(string s)
    {
        if (s.Length != 7 || s[0] != '#') return null;
        try
        {
            byte b(int i) => Convert.ToByte(s.Substring(i, 2), 16);
            return Windows.UI.Color.FromArgb(255, b(1), b(3), b(5));
        }
        catch { return null; }
    }

    public static ControllerModel? FromJson(string json) =>
        string.IsNullOrWhiteSpace(json) ? null : JsonSerializer.Deserialize<ControllerModel>(json, ProfileModel.Json);

    public string ToJson() => JsonSerializer.Serialize(this, ProfileModel.Json);

    /// This controller's own profile for a game, or null = "same as all controllers".
    public string? ProfileFor(string exe) =>
        GameProfiles.FirstOrDefault(kv => kv.Key.Equals(exe, StringComparison.OrdinalIgnoreCase)).Value;

    public void SetProfileFor(string exe, string? profile)
    {
        foreach (var k in GameProfiles.Keys.Where(k => k.Equals(exe, StringComparison.OrdinalIgnoreCase)).ToList())
            GameProfiles.Remove(k);
        if (!string.IsNullOrEmpty(profile)) GameProfiles[exe.ToLowerInvariant()] = profile;
    }

    /// Engine target codes (DxoVirtual.Target) <-> names.
    public static string TargetName(int index) => index switch { 1 => "dual_shock4", 2 => "none", _ => "xbox360" };
    public static int TargetIndex(string t) => t switch { "dual_shock4" => 1, "none" => 2, _ => 0 };
}

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

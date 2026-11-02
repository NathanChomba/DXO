using System.Collections.Generic;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace DXO.UI.ViewModels;

/// C# mirror of dxo-core `Profile` (profile.rs). Field names are snake_case on
/// the wire; Rust parses and validates every save, so a field we don't model
/// here simply falls back to Rust's default.
public sealed class ProfileModel
{
    public string Name { get; set; } = "New profile";
    public List<string> MatchProcess { get; set; } = new();
    /// "xbox360" | "dual_shock4" | "none"
    public string Target { get; set; } = "xbox360";
    public StickModel LeftStick { get; set; } = new();
    public StickModel RightStick { get; set; } = new();
    public float TriggerDeadzone { get; set; } = 0.02f;
    /// [from, to] pairs, e.g. ["cross", "circle"]
    public List<List<string>> Remap { get; set; } = new();
    public int[] Lightbar { get; set; } = { 0, 64, 255 };
    public byte PlayerLeds { get; set; } = 0b00100;
    public bool HidePhysical { get; set; } = true;
    public bool RumblePassthrough { get; set; } = true;
    public GyroModel Gyro { get; set; } = new();
    public uint IdleDisconnectMin { get; set; } = 15;

    public static readonly JsonSerializerOptions Json = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
        WriteIndented = false,
    };

    public static ProfileModel? FromJson(string json) =>
        string.IsNullOrWhiteSpace(json) ? null : JsonSerializer.Deserialize<ProfileModel>(json, Json);

    public string ToJson() => JsonSerializer.Serialize(this, Json);

    public ProfileModel Clone() => FromJson(ToJson())!;
}

public sealed class StickModel
{
    public float Deadzone { get; set; } = 0.08f;
    public float Outer { get; set; } = 0.98f;
    public CurveModel Curve { get; set; } = new();
    public bool InvertY { get; set; }
}

/// Rust: #[serde(tag = "type")] enum Curve { Linear, Power { exponent }, Smooth { amount } }
public sealed class CurveModel
{
    /// "linear" | "power" | "smooth"
    public string Type { get; set; } = "linear";
    public float? Exponent { get; set; }
    public float? Amount { get; set; }
}

public sealed class GyroModel
{
    public bool CemuhookServer { get; set; }
    public ushort CemuhookPort { get; set; } = 26760;
    public bool ToMouse { get; set; }
    public float Sensitivity { get; set; } = 1.0f;
}

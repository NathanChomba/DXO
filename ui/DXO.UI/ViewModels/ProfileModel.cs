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
    /// "xbox360" | "dual_shock4" | "none"  (Rust VirtualTarget, snake_case)
    public string Target { get; set; } = "xbox360";
    public StickModel LeftStick { get; set; } = new();
    public StickModel RightStick { get; set; } = new();
    public float TriggerDeadzone { get; set; } = 0.02f;
    /// [from, to] pairs, e.g. ["cross", "circle"]
    public List<List<string>> Remap { get; set; } = new();
    /// Per-button actions (Mappings page). Rust: bindings.rs `Binding`.
    public List<BindingModel> Bindings { get; set; } = new();
    public int[] Lightbar { get; set; } = { 0, 64, 255 };
    public byte PlayerLeds { get; set; } = 0b00100;
    public bool HidePhysical { get; set; } = true;
    public bool RumblePassthrough { get; set; } = true;
    /// 0 = off, 1 = as the game sends, up to 2 = boosted
    public float RumbleStrength { get; set; } = 1.0f;
    public bool RumbleSwap { get; set; }
    /// Sound to the controller's speaker / headset jack (USB)
    public bool ControllerAudio { get; set; }
    public byte AudioVolume { get; set; } = 70;
    public GyroModel Gyro { get; set; } = new();
    /// Hold this button for the shift layer; "" = off.
    public string ShiftButton { get; set; } = "";
    /// Bindings used while the shift button is held.
    public List<BindingModel> ShiftBindings { get; set; } = new();
    public MouseModel Mouse { get; set; } = new();

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

public sealed class BindingModel
{
    /// Source button key, e.g. "cross", "r2"
    public string Button { get; set; } = "";
    public ActionModel Action { get; set; } = new();
    public bool Turbo { get; set; }
    public float TurboHz { get; set; } = 10f;
    public bool Toggle { get; set; }
}

/// Rust: #[serde(tag = "type")] enum Action { Same, Button { to }, Key { vk }, Mouse { button }, Disabled, Macro { steps, repeat } }
public sealed class ActionModel
{
    /// "same" | "button" | "key" | "mouse" | "disabled" | "macro"
    public string Type { get; set; } = "same";
    /// Type = "macro"
    public List<MacroStepModel>? Steps { get; set; }
    public bool? Repeat { get; set; }
    /// target pad button (Type = "button")
    public string? To { get; set; }
    /// Windows virtual-key code (Type = "key")
    public ushort? Vk { get; set; }
    /// "left" | "right" | "middle" | "back" | "forward" | "wheel_up" | "wheel_down" (Type = "mouse")
    public string? Button { get; set; }
}

/// One macro step: press everything in Presses together for HoldMs, then wait GapMs.
public sealed class MacroStepModel
{
    public List<PressModel> Presses { get; set; } = new();
    public uint HoldMs { get; set; } = 50;
    public uint GapMs { get; set; } = 50;
}

/// Rust: #[serde(tag = "type")] enum Press { Button { to }, Key { vk }, Mouse { button } }
public sealed class PressModel
{
    /// "button" | "key" | "mouse"
    public string Type { get; set; } = "key";
    public string? To { get; set; }
    public ushort? Vk { get; set; }
    public string? Button { get; set; }
}

/// Touchpad and stick as mouse (pointer.rs MouseConfig).
public sealed class MouseModel
{
    public bool Touchpad { get; set; }
    public float TouchpadSpeed { get; set; } = 1.0f;
    public bool TouchpadButtons { get; set; } = true;
    /// "" = off, "left", "right"
    public string Stick { get; set; } = "";
    public float StickSpeed { get; set; } = 1200f;
}

public sealed class GyroModel
{
    public bool CemuhookServer { get; set; }
    public ushort CemuhookPort { get; set; } = 26760;
    public bool ToMouse { get; set; }
    public float Sensitivity { get; set; } = 1.0f;
    /// "" = always, "touch" = finger on touchpad, or a button key ("l2", "r1"...)
    public string MouseHold { get; set; } = "l2";
    /// deg/s ignored before the cursor moves (hand tremor)
    public float MouseDeadzone { get; set; } = 1.0f;
    public bool InvertX { get; set; }
    public bool InvertY { get; set; }
}

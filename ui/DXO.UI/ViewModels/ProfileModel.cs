using System;
using System.Collections.Generic;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace DXO.UI.ViewModels;

/// C# mirror of dxo-core `Profile` (profile.rs). Field names are snake_case on
/// the wire; Rust parses and validates every save. A field the engine sends
/// that isn't modelled here is carried in `Extra` and written back untouched,
/// so a newer engine's settings survive a round trip through an older page.
public sealed class ProfileModel
{
    [JsonExtensionData] public Dictionary<string, JsonElement>? Extra { get; set; }

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
    /// Two buttons held together do something else (Rust: bindings.rs `ChordBinding`).
    public List<ChordModel> Chords { get; set; } = new();
    /// Tap / hold / double-tap timing, milliseconds.
    public uint HoldMs { get; set; } = 250;
    public uint DoubleTapMs { get; set; } = 250;
    /// Dual-stage triggers: how far (0..1) a trigger is pulled for "soft" and "full".
    public float TriggerSoftPoint { get; set; } = 0.12f;
    public float TriggerFullPoint { get; set; } = 0.9f;
    /// Adaptive-trigger effects (DualSense).
    public TriggerEffectModel TriggerLeft { get; set; } = new();
    public TriggerEffectModel TriggerRight { get; set; } = new();
    public LightModel Light { get; set; } = new();
    public RumbleShapeModel RumbleShape { get; set; } = new();
    public AudioHapticsModel AudioHaptics { get; set; } = new();
    public FlickModel Flick { get; set; } = new();

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
    /// Size of the GAME's own deadzone to cancel out (0 = off).
    public float AntiDeadzone { get; set; }
}

/// Rust: #[serde(tag = "type")] enum Curve { Linear, Power { exponent }, Smooth { amount }, Bezier { x1, y1, x2, y2 } }
public sealed class CurveModel
{
    /// "linear" | "power" | "smooth" | "bezier"
    public string Type { get; set; } = "linear";
    public float? Exponent { get; set; }
    public float? Amount { get; set; }
    /// Bezier handles (like CSS cubic-bezier).
    public float? X1 { get; set; }
    public float? Y1 { get; set; }
    public float? X2 { get; set; }
    public float? Y2 { get; set; }

    public static CurveModel Power(float exponent) => new() { Type = "power", Exponent = exponent };
    public static CurveModel Bezier(float x1, float y1, float x2, float y2) =>
        new() { Type = "bezier", X1 = x1, Y1 = y1, X2 = x2, Y2 = y2 };

    /// The curve's output for an input of 0..1 (mirror of profile.rs `Curve::apply`), for previews.
    public double Apply(double t)
    {
        t = Math.Clamp(t, 0, 1);
        switch (Type)
        {
            case "power": return Math.Pow(t, Math.Max(0.1, Exponent ?? 1));
            case "smooth":
                double a = Math.Clamp(Amount ?? 0, 0, 1);
                return t * (1 - a) + t * t * t * a;
            case "bezier":
                if (t <= 0) return 0;
                if (t >= 1) return 1;
                double x1 = Math.Clamp(X1 ?? 0.3, 0, 1), x2 = Math.Clamp(X2 ?? 0.7, 0, 1), y1 = Y1 ?? 0.3, y2 = Y2 ?? 0.7;
                static double At(double u, double p1, double p2)
                {
                    double v = 1 - u;
                    return 3 * v * v * u * p1 + 3 * v * u * u * p2 + u * u * u;
                }
                double lo = 0, hi = 1;
                for (int i = 0; i < 20; i++)
                {
                    double mid = (lo + hi) / 2;
                    if (At(mid, x1, x2) < t) lo = mid; else hi = mid;
                }
                return Math.Clamp(At((lo + hi) / 2, y1, y2), 0, 1);
            default: return t;
        }
    }
}

public sealed class BindingModel
{
    /// Source button key, e.g. "cross", "r2"
    public string Button { get; set; } = "";
    public ActionModel Action { get; set; } = new();
    public bool Turbo { get; set; }
    public float TurboHz { get; set; } = 10f;
    public bool Toggle { get; set; }
    /// What holding the button does (null = nothing special).
    public ActionModel? Hold { get; set; }
    /// What two quick presses do (null = nothing special).
    public ActionModel? DoubleTap { get; set; }
}

/// Buttons held together -> one action.
public sealed class ChordModel
{
    public List<string> Buttons { get; set; } = new();
    public ActionModel Action { get; set; } = new() { Type = "disabled" };
}

/// Rust: #[serde(tag = "type")] enum Action { Same, Button { to }, Key { vk }, Mouse { button }, Disabled, Macro { steps, repeat }, Launch { target, args } }
public sealed class ActionModel
{
    /// "same" | "button" | "key" | "mouse" | "disabled" | "macro" | "launch"
    public string Type { get; set; } = "same";
    /// Type = "launch": a program, folder, file or web address, and optional arguments.
    public string? Target { get; set; }
    public string? Args { get; set; }
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
    /// Slide two fingers to scroll / pinch them to zoom.
    public bool TwoFingerScroll { get; set; } = true;
    public float ScrollSpeed { get; set; } = 1.0f;
    public bool PinchZoom { get; set; } = true;
    /// "" = off, "left", "right"
    public string Stick { get; set; } = "";
    public float StickSpeed { get; set; } = 1200f;
    /// How cursor speed grows with stick push (default: power 2).
    public CurveModel StickCurve { get; set; } = CurveModel.Power(2f);
    /// Give the touchpad/stick back to a full-screen game.
    public bool SuspendInFullscreen { get; set; } = true;
    [JsonExtensionData] public Dictionary<string, JsonElement>? Extra { get; set; }
}

/// Rust: output.rs `TriggerEffect` (#[serde(tag = "type")]). Only the fields
/// the chosen type uses are sent.
public sealed class TriggerEffectModel
{
    /// "off" | "feedback" | "weapon" | "vibration" | "bow" | "galloping" | "machine" | "raw"
    public string Type { get; set; } = "off";
    public byte? Start { get; set; }
    public byte? End { get; set; }
    public byte? Strength { get; set; }
    public byte? Snap { get; set; }
    public byte? Amplitude { get; set; }
    public byte? Frequency { get; set; }
    public byte? FirstFoot { get; set; }
    public byte? SecondFoot { get; set; }
    public byte? AmplitudeA { get; set; }
    public byte? AmplitudeB { get; set; }
    public byte? Period { get; set; }
    /// Type = "raw": the 11-byte effect block.
    public int[]? Data { get; set; }

    public TriggerEffectModel Clone() => (TriggerEffectModel)MemberwiseClone();

    /// Same effect with the same numbers? (Used to recognise a preset.)
    public bool SameAs(TriggerEffectModel o) =>
        Type == o.Type && Start == o.Start && End == o.End && Strength == o.Strength && Snap == o.Snap &&
        Amplitude == o.Amplitude && Frequency == o.Frequency && FirstFoot == o.FirstFoot && SecondFoot == o.SecondFoot &&
        AmplitudeA == o.AmplitudeA && AmplitudeB == o.AmplitudeB && Period == o.Period;
}

/// Rust: light.rs `LightConfig`.
public sealed class LightModel
{
    /// "static" | "breathing" | "pulse" | "rainbow" | "battery"
    public string Mode { get; set; } = "static";
    /// Cycles per second for the moving modes.
    public float Speed { get; set; } = 0.25f;
    public float Brightness { get; set; } = 1f;
    public bool FlashOnRumble { get; set; }
}

/// Rust: profile.rs `RumbleShape`.
public sealed class RumbleShapeModel
{
    public float Floor { get; set; }
    public float Ceiling { get; set; } = 1f;
    public float Curve { get; set; } = 1f;
}

/// Rust: profile.rs `AudioHaptics`.
public sealed class AudioHapticsModel
{
    /// "off" | "rumble"
    public string Mode { get; set; } = "off";
    public float Gain { get; set; } = 1f;
}

/// Rust: flick.rs `FlickConfig`.
public sealed class FlickModel
{
    public bool Enabled { get; set; }
    public float PxPerTurn { get; set; } = 6000f;
    public uint FlickMs { get; set; } = 100;
    public float Threshold { get; set; } = 0.9f;
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
    /// Hold this button to pause gyro aim while re-centring; "" = off.
    public string Ratchet { get; set; } = "";
    /// Jitter filter 0..1.
    public float Smoothing { get; set; }
    /// "local" (the pad's own axes) | "player" (turning about gravity)
    public string Space { get; set; } = "local";
    [JsonExtensionData] public Dictionary<string, JsonElement>? Extra { get; set; }
}

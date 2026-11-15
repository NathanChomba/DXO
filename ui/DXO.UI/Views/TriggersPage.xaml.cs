using System;
using System.ComponentModel;
using System.Linq;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Navigation;
using DXO.UI.Services;
using DXO.UI.ViewModels;

namespace DXO.UI.Views;

/// A named, ready-made trigger effect. `Uses` lists the sliders that make
/// sense for it (start, end, strength, snap, frequency).
public sealed record TriggerPreset(string Label, string About, TriggerEffectModel Effect, string Uses);

/// Adaptive-trigger effects for the ACTIVE profile (live). Pick a preset per
/// trigger, fine-tune it with the sliders; Save writes the profile file.
/// Encodings live in the engine (output.rs `TriggerEffect`).
public sealed partial class TriggersPage : Page, INotifyPropertyChanged
{
    public DeviceViewModel Device => App.Device;
    public bool NotDualSense => !Device.IsDualSense;
    public event PropertyChangedEventHandler? PropertyChanged;

    /// The preset library. A profile's effect is matched to the first preset of
    /// the same kind, so hand-tuned numbers still show under a sensible name.
    public static readonly TriggerPreset[] Presets =
    {
        new("Off", "A normal trigger.", new TriggerEffectModel { Type = "off" }, ""),
        new("Resistance", "Pushes back evenly from the start position to the end.",
            new TriggerEffectModel { Type = "feedback", Start = 3, Strength = 5 }, "start,strength"),
        new("Heavy trigger", "Stiff all the way down, like a strong spring.",
            new TriggerEffectModel { Type = "feedback", Start = 0, Strength = 8 }, "start,strength"),
        new("Car throttle", "Light resistance through the whole pull, for feathering the accelerator.",
            new TriggerEffectModel { Type = "feedback", Start = 1, Strength = 3 }, "start,strength"),
        new("Pistol", "Take up the slack, then a firm click as it breaks.",
            new TriggerEffectModel { Type = "weapon", Start = 2, End = 5, Strength = 8 }, "start,end,strength"),
        new("Hair trigger", "A short, light click right at the top.",
            new TriggerEffectModel { Type = "weapon", Start = 2, End = 3, Strength = 4 }, "start,end,strength"),
        new("Bow", "Tension builds as you draw, then lets go with a kick.",
            new TriggerEffectModel { Type = "bow", Start = 1, End = 6, Strength = 6, Snap = 8 }, "start,end,strength,snap"),
        new("Machine gun", "Rattles while held past the start, like automatic fire.",
            new TriggerEffectModel { Type = "machine", Start = 2, End = 9, AmplitudeA = 7, AmplitudeB = 3, Frequency = 12, Period = 2 }, "start,end,strength,frequency"),
        new("Brake (ABS)", "Buzzes under your finger near the bottom of the pull.",
            new TriggerEffectModel { Type = "vibration", Start = 5, Amplitude = 6, Frequency = 25 }, "start,strength,frequency"),
        new("Engine rumble", "A slow, deep throb from the first touch.",
            new TriggerEffectModel { Type = "vibration", Start = 1, Amplitude = 4, Frequency = 8 }, "start,strength,frequency"),
        new("Galloping", "A two-beat thump, like hooves or a heartbeat.",
            new TriggerEffectModel { Type = "galloping", Start = 0, End = 9, FirstFoot = 2, SecondFoot = 5, Frequency = 3 }, "start,end,frequency"),
    };

    private ProfileModel? _model;
    private bool _loading;

    public TriggersPage()
    {
        _loading = true;
        InitializeComponent();
        LeftEffect.ItemsSource = Presets;
        RightEffect.ItemsSource = Presets;
        _loading = false;
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        if (e.Parameter is "embedded") PageTitle.Visibility = Visibility.Collapsed; // the tab already says it
        Device.PropertyChanged += OnDevice;
        Device.UseProfileOutput(); // a page's preview must not hide the profile's effects
        Load();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e) => Device.PropertyChanged -= OnDevice;

    private void OnDevice(object? s, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(DeviceViewModel.IsDualSense))
            PropertyChanged?.Invoke(this, new(nameof(NotDualSense)));
    }

    // ================= profile <-> controls =================

    private void Load()
    {
        _model = ProfileStore.Active();
        bool ok = _model is not null;
        foreach (var c in new Control[] { LeftEffect, RightEffect, SaveButton }) c.IsEnabled = ok;
        if (_model is null) { SaveStatus.Text = "The engine isn't running."; ShowSliders(); return; }
        _loading = true;
        Show(_model.TriggerLeft, left: true);
        Show(_model.TriggerRight, left: false);
        _loading = false;
        ShowSliders();
        SubTitle.Text = $"Editing \"{_model.Name}\" (the active profile). Pick a ready-made effect, then fine-tune it. Changes apply straight away; Save keeps them.";
        SaveStatus.Text = "";
    }

    private static TriggerPreset PresetFor(TriggerEffectModel e) =>
        Presets.FirstOrDefault(p => p.Effect.SameAs(e)) ?? Presets.FirstOrDefault(p => p.Effect.Type == e.Type) ?? Presets[0];

    /// Effect -> one trigger's controls.
    private void Show(TriggerEffectModel e, bool left)
    {
        var (effect, _, start, end, strength, snap, frequency) = Controls(left);
        effect.SelectedItem = PresetFor(e);
        start.Value = e.Start ?? 0;
        end.Value = Math.Max(1, (int)(e.End ?? 9));
        strength.Value = Math.Clamp((int)(e.Type switch
        {
            "vibration" => e.Amplitude ?? 5,
            "machine" => (byte)((e.AmplitudeA ?? 6) + 1), // the engine counts this one 0..7
            _ => e.Strength ?? 5,
        }), 1, 8);
        snap.Value = Math.Clamp((int)(e.Snap ?? 8), 1, 8);
        frequency.Value = Math.Clamp((int)(e.Frequency ?? 10), 1, 60);
    }

    private (ComboBox Effect, TextBlock About, Slider Start, Slider End, Slider Strength, Slider Snap, Slider Frequency) Controls(bool left) =>
        left ? (LeftEffect, LeftAbout, LeftStart, LeftEnd, LeftStrength, LeftSnap, LeftFrequency)
             : (RightEffect, RightAbout, RightStart, RightEnd, RightStrength, RightSnap, RightFrequency);

    /// One trigger's controls -> effect: the chosen preset with the sliders'
    /// numbers laid over the fields that preset uses.
    private TriggerEffectModel Read(bool left)
    {
        var (effect, _, start, end, strength, snap, frequency) = Controls(left);
        var preset = effect.SelectedItem as TriggerPreset ?? Presets[0];
        var e = preset.Effect.Clone();
        byte s0 = (byte)start.Value, e0 = (byte)end.Value, str = (byte)strength.Value, fr = (byte)frequency.Value;
        if (e.Type == "off") return e;
        e.Start = s0;
        if (preset.Uses.Contains("end")) e.End = (byte)Math.Max(e0, s0 + 1);
        if (preset.Uses.Contains("snap")) e.Snap = (byte)snap.Value;
        if (preset.Uses.Contains("frequency")) e.Frequency = fr;
        switch (e.Type)
        {
            case "vibration": e.Amplitude = str; break;
            case "machine": e.AmplitudeA = (byte)(str - 1); break;
            case "galloping": break; // no strength: the beat is fixed by the pad
            default: e.Strength = str; break;
        }
        return e;
    }

    /// Show only the sliders the chosen effect uses.
    private void ShowSliders()
    {
        foreach (bool left in new[] { true, false })
        {
            var (effect, about, start, end, strength, snap, frequency) = Controls(left);
            var preset = effect.SelectedItem as TriggerPreset ?? Presets[0];
            about.Text = preset.About;
            Visibility Vis(string name) => preset.Uses.Contains(name) ? Visibility.Visible : Visibility.Collapsed;
            start.Visibility = Vis("start");
            end.Visibility = Vis("end");
            strength.Visibility = Vis("strength");
            snap.Visibility = Vis("snap");
            frequency.Visibility = Vis("frequency");
        }
    }

    private void Push()
    {
        if (_loading || _model is null) return;
        _model.TriggerLeft = Read(left: true);
        _model.TriggerRight = Read(left: false);
        SaveStatus.Text = ProfileStore.ApplyActive(_model)
            ? $"Applied. Save to keep it in \"{_model.Name}\"."
            : "The engine rejected that setting.";
    }

    // Typed handlers: the XAML compiler wants exact event signatures.
    private void EffectChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_loading) return;
        // A newly picked preset starts from its own numbers.
        bool left = ReferenceEquals(sender, LeftEffect);
        if ((sender as ComboBox)?.SelectedItem is TriggerPreset preset)
        {
            _loading = true;
            Show(preset.Effect, left);
            _loading = false;
        }
        ShowSliders();
        Push();
    }

    private void SliderChanged(object sender, Microsoft.UI.Xaml.Controls.Primitives.RangeBaseValueChangedEventArgs e) => Push();

    private void CopyLeft_Click(object sender, RoutedEventArgs e)
    {
        if (_model is null) return;
        var copy = Read(left: true);
        _loading = true;
        Show(copy, left: false);
        _loading = false;
        ShowSliders();
        Push();
    }

    private void BothOff_Click(object sender, RoutedEventArgs e)
    {
        if (_model is null) return;
        _loading = true;
        Show(Presets[0].Effect, left: true);
        Show(Presets[0].Effect, left: false);
        _loading = false;
        ShowSliders();
        Push();
    }

    private void Save_Click(object sender, RoutedEventArgs e)
    {
        // Save what the engine is running (it also carries edits from other tabs).
        var m = ProfileStore.Active();
        if (m is null) return;
        SaveStatus.Text = ProfileStore.Save(m) ? $"Saved to \"{m.Name}\"." : "Couldn't save the profile.";
    }
}

using System;
using System.ComponentModel;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;
using DXO.UI.ViewModels;
using Windows.UI;

namespace DXO.UI.Views;

public sealed partial class LightingPage : Page, INotifyPropertyChanged
{
    public DeviceViewModel Device => App.Device;
    public bool NotDualSense => !Device.IsDualSense;
    public event PropertyChangedEventHandler? PropertyChanged;

    // Bit masks for the 5 DualSense player LEDs, left to right = bit 4..0
    private static readonly byte[] PlayerMasks = { 0b00000, 0b00100, 0b01010, 0b10101, 0b11011, 0b11111 };

    private static readonly Color[] SwatchColors =
    {
        ColorHelper.FromArgb(255, 0xFF, 0x3B, 0x30), ColorHelper.FromArgb(255, 0xFF, 0x95, 0x00),
        ColorHelper.FromArgb(255, 0xFF, 0xD6, 0x0A), ColorHelper.FromArgb(255, 0x34, 0xC7, 0x59),
        ColorHelper.FromArgb(255, 0x00, 0xC7, 0xBE), ColorHelper.FromArgb(255, 0x0A, 0x84, 0xFF),
        ColorHelper.FromArgb(255, 0xB2, 0x6B, 0xFF), ColorHelper.FromArgb(255, 0xFF, 0x2D, 0x92),
        ColorHelper.FromArgb(255, 0xFF, 0xFF, 0xFF),
    };

    /// LightMode radio order -> light.rs `LightMode`.
    private static readonly string[] Modes = { "static", "breathing", "pulse", "rainbow", "battery" };

    private bool _ready;
    private bool _loading;
    private ProfileModel? _model;

    public LightingPage()
    {
        _loading = true;
        InitializeComponent();
        var brushes = new System.Collections.Generic.List<SolidColorBrush>();
        foreach (var c in SwatchColors) brushes.Add(new SolidColorBrush(c));
        Swatches.ItemsSource = brushes;
        _loading = false;
        _ready = true;
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        Device.PropertyChanged += OnDevice;
        Load();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e) => Device.PropertyChanged -= OnDevice;

    private void OnDevice(object? s, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(DeviceViewModel.IsDualSense))
            PropertyChanged?.Invoke(this, new(nameof(NotDualSense)));
    }

    private void Tabs_SelectionChanged(SelectorBar sender, SelectorBarSelectionChangedEventArgs args)
    {
        bool light = sender.SelectedItem == sender.Items[0];
        LightBarTab.Visibility = light ? Visibility.Visible : Visibility.Collapsed;
        PlayerTab.Visibility = light ? Visibility.Collapsed : Visibility.Visible;
    }

    // ================= profile <-> controls =================

    private void Load()
    {
        _model = DXO.UI.Services.ProfileStore.Active();
        SaveButton.IsEnabled = _model is not null;
        if (_model is null) { SaveStatus.Text = "The engine isn't running."; return; }
        _loading = true;
        var lb = _model.Lightbar;
        if (lb is { Length: 3 }) Picker.Color = ColorHelper.FromArgb(255, (byte)lb[0], (byte)lb[1], (byte)lb[2]);
        Brightness.Value = Math.Clamp(Math.Round(_model.Light.Brightness * 100), 5, 100);
        LightMode.SelectedIndex = Math.Max(0, Array.IndexOf(Modes, _model.Light.Mode));
        // stored as cycles per second; shown as seconds per cycle
        SpeedSlider.Value = Math.Clamp(Math.Round(1.0 / Math.Max(0.05, _model.Light.Speed) * 2) / 2, 1, 12);
        FlashToggle.IsOn = _model.Light.FlashOnRumble;
        _loading = false;
        UpdateLabels();
        SaveStatus.Text = "";
    }

    /// Controls -> the active profile, live.
    private void Apply()
    {
        if (!_ready || _loading || _model is null) return;
        var c = Picker.Color;
        _model.Lightbar = new[] { (int)c.R, c.G, c.B };
        _model.Light.Brightness = (float)(Brightness.Value / 100.0);
        _model.Light.Mode = Modes[Math.Clamp(LightMode.SelectedIndex, 0, Modes.Length - 1)];
        _model.Light.Speed = (float)Math.Round(1.0 / Math.Max(0.5, SpeedSlider.Value), 3);
        _model.Light.FlashOnRumble = FlashToggle.IsOn;
        Device.UseProfileOutput(); // drop any LED preview so the profile's light shows
        SaveStatus.Text = DXO.UI.Services.ProfileStore.ApplyActive(_model)
            ? $"Applied. Save to keep it in \"{_model.Name}\"."
            : "The engine rejected that setting.";
        UpdateLabels();
    }

    private void UpdateLabels()
    {
        if (SpeedText is null || ModeHint is null || SpeedCard is null) return;
        int mode = Math.Max(0, LightMode.SelectedIndex);
        SpeedText.Text = $"{SpeedSlider.Value:0.#} s cycle";
        SpeedCard.Visibility = mode is 1 or 2 or 3 ? Visibility.Visible : Visibility.Collapsed;
        ModeHint.Text = mode switch
        {
            3 => "Rainbow ignores the colour on the right.",
            4 => "Shows the charge instead of the colour, and breathes while charging. A controller that doesn't report its battery keeps its normal colour.",
            _ => "",
        };
    }

    private void Picker_ColorChanged(ColorPicker sender, ColorChangedEventArgs args) => Apply();
    private void Slider_Changed(object sender, Microsoft.UI.Xaml.Controls.Primitives.RangeBaseValueChangedEventArgs e) => Apply();
    private void Mode_Changed(object sender, SelectionChangedEventArgs e) => Apply();
    private void Toggle_Changed(object sender, RoutedEventArgs e) => Apply();

    private void Swatch_Click(object sender, RoutedEventArgs e)
    {
        if (((Button)sender).Background is SolidColorBrush b) Picker.Color = b.Color; // triggers Apply via ColorChanged
    }

    /// Player LEDs aren't part of a profile (each player slot has its own
    /// pattern): this is a live preview only.
    private void PlayerLeds_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (!_ready || PlayerLeds.SelectedIndex < 0) return;
        Device.SetPlayerLeds(PlayerMasks[PlayerLeds.SelectedIndex]);
    }

    private void Reset_Click(object sender, RoutedEventArgs e) => Device.UseProfileOutput();

    private void Save_Click(object sender, RoutedEventArgs e)
    {
        var m = DXO.UI.Services.ProfileStore.Active();
        if (m is null) return;
        SaveStatus.Text = DXO.UI.Services.ProfileStore.Save(m) ? $"Saved to \"{m.Name}\"." : "Couldn't save the profile.";
    }
}

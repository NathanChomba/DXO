using System;
using System.Diagnostics;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Navigation;
using DXO.UI.Services;
using DXO.UI.ViewModels;

namespace DXO.UI.Views;

/// Rumble strength / swap / passthrough and controller audio for the ACTIVE
/// profile (live), plus motor tests. Save writes the profile file.
public sealed partial class HapticsPage : Page
{
    private readonly DispatcherTimer _testTimer = new() { Interval = TimeSpan.FromMilliseconds(900) };
    private ProfileModel? _model;
    private bool _loading;

    public HapticsPage()
    {
        _loading = true;
        InitializeComponent();
        _loading = false;
        _testTimer.Tick += (_, _) => StopTest();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e) => Load();

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        if (_testTimer.IsEnabled) StopTest();
    }

    private void Load()
    {
        _model = ProfileStore.Active();
        bool ok = _model is not null;
        foreach (var c in new Control[] { PassToggle, StrengthSlider, SwapToggle, AudioToggle, VolumeSlider, SaveButton, TestHeavy, TestLight, TestBoth })
            c.IsEnabled = ok;
        if (_model is null) { SaveStatus.Text = "The engine isn't running."; return; }

        _loading = true;
        PassToggle.IsOn = _model.RumblePassthrough;
        StrengthSlider.Value = Math.Round(_model.RumbleStrength * 100);
        SwapToggle.IsOn = _model.RumbleSwap;
        AudioToggle.IsOn = _model.ControllerAudio;
        VolumeSlider.Value = _model.AudioVolume;
        _loading = false;

        SubTitle.Text = $"Editing \"{_model.Name}\" (the active profile). Changes apply straight away; Save keeps them.";
        SaveStatus.Text = "";
        UpdateLabels();
        AudioInfo.Severity = App.Device.TransportText == "Bluetooth" ? InfoBarSeverity.Warning : InfoBarSeverity.Informational;
    }

    private void Push()
    {
        if (_loading || _model is null) return;
        _model.RumblePassthrough = PassToggle.IsOn;
        _model.RumbleStrength = (float)(StrengthSlider.Value / 100.0);
        _model.RumbleSwap = SwapToggle.IsOn;
        _model.ControllerAudio = AudioToggle.IsOn;
        _model.AudioVolume = (byte)Math.Clamp(Math.Round(VolumeSlider.Value), 0, 100);
        SaveStatus.Text = ProfileStore.ApplyActive(_model)
            ? $"Applied. Save to keep it in \"{_model.Name}\"."
            : "The engine rejected that setting.";
        UpdateLabels();
    }

    private void UpdateLabels()
    {
        if (StrengthText is null || VolumeText is null) return;
        StrengthText.Text = $"{StrengthSlider.Value:0}%";
        VolumeText.Text = $"{VolumeSlider.Value:0}%";
        StrengthSlider.IsEnabled = SwapToggle.IsEnabled = PassToggle.IsOn && _model is not null;
        VolumeSlider.IsEnabled = AudioToggle.IsOn && _model is not null;
    }

    private void Toggle_Changed(object sender, RoutedEventArgs e) => Push();
    private void Slider_Changed(object sender, RangeBaseValueChangedEventArgs e) => Push();

    // ---- motor test: a short buzz through the UI output override ----

    private void StartTest(byte heavy, byte light)
    {
        App.Device.SetRumble(heavy, light);
        _testTimer.Stop();
        _testTimer.Start();
    }

    private void StopTest()
    {
        _testTimer.Stop();
        App.Device.SetRumble(0, 0);
        App.Device.UseProfileOutput(); // back to the profile's lightbar etc.
    }

    private void TestHeavy_Click(object sender, RoutedEventArgs e) => StartTest(200, 0);
    private void TestLight_Click(object sender, RoutedEventArgs e) => StartTest(0, 200);
    private void TestBoth_Click(object sender, RoutedEventArgs e) => StartTest(200, 200);

    private void SoundSettings_Click(object sender, RoutedEventArgs e)
    {
        try { Process.Start(new ProcessStartInfo("ms-settings:sound") { UseShellExecute = true }); }
        catch (Exception ex) { App.LogCrash("Open sound settings", ex); }
    }

    private void Save_Click(object sender, RoutedEventArgs e)
    {
        // Save what the engine is running (it also carries edits from other pages).
        var m = ProfileStore.Active();
        if (m is null) return;
        SaveStatus.Text = ProfileStore.Save(m) ? $"Saved to \"{m.Name}\"." : "Couldn't save the profile.";
    }
}

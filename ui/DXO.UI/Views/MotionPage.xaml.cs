using System;
using System.IO;
using System.Linq;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;
using DXO.UI.Native;
using DXO.UI.ViewModels;
using IOPath = System.IO.Path;

namespace DXO.UI.Views;

/// Gyro calibration, gyro-to-mouse and the CemuHook server. Edits the ACTIVE
/// profile live; "Save to profile" writes it back to its file.
public sealed partial class MotionPage : Page
{
    /// HoldBox order -> GyroConfig.mouse_hold keys (mapping.rs button names).
    private static readonly string[] HoldKeys = { "", "l2", "r2", "l1", "r1", "touch" };
    private static readonly string[] HoldLabels = { "", "L2", "R2", "L1", "R1", "a finger on the touchpad" };

    /// Bar full scale, degrees/second (a quick flick is ~300).
    private const float BarMaxDps = 300f;

    private readonly DispatcherTimer _timer = new() { Interval = TimeSpan.FromMilliseconds(33) };
    private bool _loading;
    private bool _unsaved;
    private string _profileName = "";

    public MotionPage()
    {
        // Building the XAML fires ValueChanged/Toggled before every control exists.
        _loading = true;
        InitializeComponent();
        _loading = false;
        _timer.Tick += (_, _) => Tick();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        LoadFromProfile();
        _timer.Start();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e) => _timer.Stop();

    // ================= profile <-> controls =================

    private ProfileModel? ActiveProfile()
    {
        try { return ProfileModel.FromJson(App.Engine?.ActiveProfileJson ?? ""); }
        catch (Exception ex) { App.LogCrash("MotionPage.ActiveProfile", ex); return null; }
    }

    private void LoadFromProfile()
    {
        var p = ActiveProfile();
        bool ok = p is not null;
        foreach (var c in new Control[] { MouseToggle, HoldBox, SensSlider, DeadSlider, InvertXToggle, InvertYToggle, CemuToggle, PortBox, CalButton, SaveButton })
            c.IsEnabled = ok;
        if (p is null) { SaveStatus.Text = "The engine isn't running."; return; }

        _loading = true;
        var g = p.Gyro;
        _profileName = p.Name;
        MouseToggle.IsOn = g.ToMouse;
        int hold = Array.IndexOf(HoldKeys, (g.MouseHold ?? "").Trim().ToLowerInvariant());
        HoldBox.SelectedIndex = hold < 0 ? 1 : hold;
        SensSlider.Value = g.Sensitivity;
        DeadSlider.Value = g.MouseDeadzone;
        InvertXToggle.IsOn = g.InvertX;
        InvertYToggle.IsOn = g.InvertY;
        CemuToggle.IsOn = g.CemuhookServer;
        PortBox.Value = g.CemuhookPort;
        _loading = false;

        _unsaved = false;
        UpdateLabels();
        SaveStatus.Text = $"Profile: {_profileName}";
    }

    /// Send the controls to the engine (live, not saved yet).
    private void Push()
    {
        if (_loading || App.Engine is null) return;
        var p = ActiveProfile();
        if (p is null) return;
        var g = p.Gyro;
        g.ToMouse = MouseToggle.IsOn;
        g.MouseHold = HoldKeys[Math.Clamp(HoldBox.SelectedIndex, 0, HoldKeys.Length - 1)];
        g.Sensitivity = (float)Math.Round(SensSlider.Value, 2);
        g.MouseDeadzone = (float)Math.Round(DeadSlider.Value, 1);
        g.InvertX = InvertXToggle.IsOn;
        g.InvertY = InvertYToggle.IsOn;
        g.CemuhookServer = CemuToggle.IsOn;
        if (!double.IsNaN(PortBox.Value)) g.CemuhookPort = (ushort)Math.Clamp((int)PortBox.Value, 1024, 65535);

        if (App.Engine.SetActiveProfileJson(p.ToJson()))
        {
            _unsaved = true;
            SaveStatus.Text = $"Applied. Save to keep it in \"{_profileName}\".";
        }
        else SaveStatus.Text = "The engine rejected that setting.";
        UpdateLabels();
    }

    private void Toggle_Changed(object sender, RoutedEventArgs e) => Push();
    private void Combo_Changed(object sender, SelectionChangedEventArgs e) => Push();
    private void Slider_Changed(object sender, RangeBaseValueChangedEventArgs e) => Push();
    private void Port_Changed(NumberBox sender, NumberBoxValueChangedEventArgs args) => Push();

    private void UpdateLabels()
    {
        if (SensText is null || DeadText is null) return;
        SensText.Text = $"{SensSlider.Value:0.00}";
        DeadText.Text = $"{DeadSlider.Value:0.0}°/s";
        bool on = MouseToggle.IsOn;
        HoldBox.IsEnabled = SensSlider.IsEnabled = DeadSlider.IsEnabled =
            InvertXToggle.IsEnabled = InvertYToggle.IsEnabled = on && App.Engine is not null;
        PortBox.IsEnabled = !CemuToggle.IsOn && App.Engine is not null; // change the port while it's off
    }

    private void Calibrate_Click(object sender, RoutedEventArgs e) => App.Engine?.CalibrateGyro();

    // ================= save =================

    private static string FileFor(string name)
    {
        var safe = new string(name.Select(ch => IOPath.GetInvalidFileNameChars().Contains(ch) ? '_' : ch).ToArray()).Trim();
        return IOPath.Combine(App.ProfilesDir, (safe.Length == 0 ? "profile" : safe) + ".toml");
    }

    /// The file whose profile has this name, else a new file named after it.
    private static string PathForProfile(string name)
    {
        Directory.CreateDirectory(App.ProfilesDir);
        foreach (var f in Directory.GetFiles(App.ProfilesDir, "*.toml"))
        {
            try
            {
                if (ProfileModel.FromJson(DxoEngine.ProfileFileToJson(f))?.Name == name) return f;
            }
            catch { /* unreadable file: not ours */ }
        }
        return FileFor(name);
    }

    private void Save_Click(object sender, RoutedEventArgs e)
    {
        var json = App.Engine?.ActiveProfileJson;
        if (string.IsNullOrEmpty(json)) return;
        var path = PathForProfile(_profileName);
        if (DxoEngine.ProfileJsonToFile(json, path))
        {
            App.Engine!.ReloadProfiles();
            _unsaved = false;
            SaveStatus.Text = $"Saved to \"{_profileName}\".";
        }
        else SaveStatus.Text = "Couldn't save the profile.";
    }

    // ================= live =================

    private void Tick()
    {
        var engine = App.Engine;
        if (engine is null) return;

        // A game profile may have switched in: show its settings (unless mid-edit).
        if (!_unsaved)
        {
            var name = ActiveProfile()?.Name;
            if (name is not null && name != _profileName) LoadFromProfile();
        }

        var m = engine.GetMotion();
        bool connected = App.Device.Connected;
        var res = Application.Current.Resources;
        Brush B(string key) => (Brush)res[key];

        SetBar(PitchNeg, PitchPos, PitchText, connected ? m.GyroPitch : 0);
        SetBar(YawNeg, YawPos, YawText, connected ? m.GyroYaw : 0);
        SetBar(RollNeg, RollPos, RollText, connected ? m.GyroRoll : 0);

        double total = Math.Sqrt(m.AccelX * m.AccelX + m.AccelY * m.AccelY + m.AccelZ * m.AccelZ);
        AccelText.Text = connected ? $"Acceleration {total:0.00} g  (about 1.00 g at rest)" : "No controller connected";

        // Tilt bubble: gravity's sideways (X) and front-back (Z) parts.
        double tx = Math.Clamp(connected ? m.AccelX : 0, -1, 1), tz = Math.Clamp(connected ? m.AccelZ : 0, -1, 1);
        Canvas.SetLeft(TiltDot, 48 + tx * 48);
        Canvas.SetTop(TiltDot, 48 + tz * 48);

        (CalText.Text, CalDot.Fill) = !connected ? ("Connect a controller", B("DxoTextSecondary")) : m.CalState switch
        {
            1 => ("Calibrating... keep the controller still", B("DxoAccent")),
            2 => ("Calibrated", B("DxoSuccess")),
            3 => ("Calibration failed: the controller moved. Lay it flat and try again.", B("DxoDanger")),
            _ => ("Not calibrated yet: set the controller down for a second, or click Calibrate", B("DxoWarning")),
        };
        CalButton.IsEnabled = connected && m.CalState != 1;

        int hold = Math.Clamp(HoldBox.SelectedIndex, 0, HoldKeys.Length - 1);
        (MouseText.Text, MouseDot.Fill) =
            !MouseToggle.IsOn ? ("Gyro aim is off", B("DxoTextSecondary"))
            : m.MouseActive != 0 ? ("Aiming with gyro now", B("DxoSuccess"))
            : ($"Ready: hold {HoldLabels[hold]} to aim", B("DxoAccent"));

        (CemuText.Text, CemuDot.Fill) =
            !CemuToggle.IsOn ? ("Off", B("DxoTextSecondary"))
            : m.CemuFailed != 0 ? ($"Port {m.CemuPort} is in use by another app. Pick another port.", B("DxoDanger"))
            : m.CemuListening == 0 ? ("Starting... (needs a connected controller)", B("DxoWarning"))
            : m.CemuClients > 0 ? ($"127.0.0.1:{m.CemuPort}  ·  {m.CemuClients} emulator{(m.CemuClients == 1 ? "" : "s")} connected", B("DxoSuccess"))
            : ($"Listening on 127.0.0.1:{m.CemuPort}  ·  waiting for an emulator", B("DxoAccent"));
    }

    private static void SetBar(FrameworkElement neg, FrameworkElement pos, TextBlock text, float dps)
    {
        double half = (neg.Parent as FrameworkElement)?.ActualWidth / 2 ?? 0;
        double w = Math.Min(Math.Abs(dps) / BarMaxDps, 1) * half;
        neg.Width = dps < 0 ? w : 0;
        pos.Width = dps > 0 ? w : 0;
        text.Text = $"{dps,6:0}°/s";
    }
}

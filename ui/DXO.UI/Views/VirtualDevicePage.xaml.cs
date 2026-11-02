using System;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;
using DXO.UI.Native;

namespace DXO.UI.Views;

public sealed partial class VirtualDevicePage : Page
{
    private bool _loading;
    private readonly DispatcherTimer _status = new() { Interval = TimeSpan.FromMilliseconds(500) };

    public VirtualDevicePage()
    {
        InitializeComponent();
        _status.Tick += (_, _) => ShowStatus();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        LoadFromEngine();
        _status.Start();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e) => _status.Stop();

    /// Put the engine's current setup into the controls without echoing it back.
    private void LoadFromEngine()
    {
        var engine = App.Engine;
        bool ok = engine is not null;
        Mode.IsEnabled = HideToggle.IsEnabled = RumbleToggle.IsEnabled = FixButton.IsEnabled = ok;
        if (!ok) { ModeNote.Text = "The engine isn't running."; return; }

        _loading = true;
        var v = engine!.GetVirtual();
        Mode.SelectedIndex = v.Target switch { 0 => 0, 2 => 2, _ => 1 };
        HideToggle.IsOn = v.HidePhysical != 0;
        RumbleToggle.IsOn = v.RumblePassthrough != 0;
        _loading = false;
        UpdateNote();
        ShowStatus();
    }

    private void Push()
    {
        if (_loading || App.Engine is null) return;
        byte target = Mode.SelectedIndex switch { 0 => 0, 2 => 2, _ => 1 };
        App.Engine.SetVirtual(target, HideToggle.IsOn, RumbleToggle.IsOn);
        UpdateNote();
    }

    private void Mode_SelectionChanged(object sender, SelectionChangedEventArgs e) => Push();
    private void Option_Toggled(object sender, RoutedEventArgs e) => Push();

    private void Fix_Click(object sender, RoutedEventArgs e) => App.Engine?.Reconnect();

    private void UpdateNote()
    {
        bool native = Mode.SelectedIndex == 0;
        ModeNote.Text = native
            ? "Native mode: games read your controller directly. Some only support Xbox controllers and won't see it."
            : Mode.SelectedIndex == 2
                ? "Switching mode briefly reconnects your controller. Game vibration can't reach your pad in DualShock 4 mode yet; use Xbox 360 mode if you want rumble."
                : "Switching mode briefly reconnects your controller.";
        // Hiding and rumble passthrough only mean something with a virtual pad.
        HideToggle.IsEnabled = RumbleToggle.IsEnabled = !native && App.Engine is not null;
    }

    private void ShowStatus()
    {
        var engine = App.Engine;
        if (engine is null) return;
        var v = engine.GetVirtual();
        var res = Application.Current.Resources;
        bool connected = App.Device.Connected;

        VirtualText.Text = !connected ? "No controller connected"
            : v.VirtualActive != 0 ? "Games see: Xbox 360 controller"
            : "Games see: your real controller";
        VirtualDot.Fill = (Brush)res[connected && v.VirtualActive != 0 ? "DxoSuccess" : "DxoTextSecondary"];

        HiddenText.Text = v.PadHidden != 0 ? "Real controller: hidden" : "Real controller: visible";
        HiddenDot.Fill = (Brush)res[v.PadHidden != 0 ? "DxoAccent" : "DxoTextSecondary"];
    }
}

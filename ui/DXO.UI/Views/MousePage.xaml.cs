using System;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Navigation;
using DXO.UI.Services;
using DXO.UI.ViewModels;

namespace DXO.UI.Views;

/// Touchpad / stick as mouse for the ACTIVE profile (live). Save writes the profile file.
public sealed partial class MousePage : Page
{
    private static readonly string[] Sticks = { "", "left", "right" };
    private ProfileModel? _model;
    private bool _loading;

    public MousePage()
    {
        _loading = true;
        InitializeComponent();
        _loading = false;
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        if (e.Parameter is "embedded") Header.Visibility = Visibility.Collapsed;
        Load();
    }

    private void Load()
    {
        _model = ProfileStore.Active();
        bool ok = _model is not null;
        foreach (var c in new Control[] { TouchToggle, TouchSpeed, ClickToggle, StickBox, StickSpeed, SaveButton })
            c.IsEnabled = ok;
        if (_model is null) { SaveStatus.Text = "The engine isn't running."; return; }

        var m = _model.Mouse;
        _loading = true;
        TouchToggle.IsOn = m.Touchpad;
        TouchSpeed.Value = Math.Clamp(m.TouchpadSpeed, 0.2, 4);
        ClickToggle.IsOn = m.TouchpadButtons;
        StickBox.SelectedIndex = Math.Max(0, Array.IndexOf(Sticks, m.Stick));
        StickSpeed.Value = Math.Clamp(m.StickSpeed, 200, 4000);
        _loading = false;

        SubTitle.Text = $"Editing \"{_model.Name}\" (the active profile). Changes apply straight away; Save keeps them.";
        SaveStatus.Text = "";
        UpdateLabels();
    }

    private void Push()
    {
        if (_loading || _model is null) return;
        var m = _model.Mouse;
        m.Touchpad = TouchToggle.IsOn;
        m.TouchpadSpeed = (float)Math.Round(TouchSpeed.Value, 1);
        m.TouchpadButtons = ClickToggle.IsOn;
        m.Stick = Sticks[Math.Max(0, StickBox.SelectedIndex)];
        m.StickSpeed = (float)Math.Round(StickSpeed.Value);
        SaveStatus.Text = ProfileStore.ApplyActive(_model)
            ? $"Applied. Save to keep it in \"{_model.Name}\"."
            : "The engine rejected that setting.";
        UpdateLabels();
    }

    private void UpdateLabels()
    {
        if (TouchSpeedText is null || StickSpeedText is null) return;
        TouchSpeedText.Text = $"{TouchSpeed.Value:0.0}x";
        StickSpeedText.Text = $"{StickSpeed.Value:0} px/s";
        bool ok = _model is not null;
        TouchSpeed.IsEnabled = ClickToggle.IsEnabled = ok && TouchToggle.IsOn;
        StickSpeed.IsEnabled = ok && StickBox.SelectedIndex > 0;
    }

    private void Toggle_Changed(object sender, RoutedEventArgs e) => Push();
    private void Slider_Changed(object sender, RangeBaseValueChangedEventArgs e) => Push();
    private void Combo_Changed(object sender, SelectionChangedEventArgs e) => Push();

    private void Save_Click(object sender, RoutedEventArgs e)
    {
        var m = ProfileStore.Active();
        if (m is null) return;
        SaveStatus.Text = ProfileStore.Save(m) ? $"Saved to \"{m.Name}\"." : "Couldn't save the profile.";
    }
}

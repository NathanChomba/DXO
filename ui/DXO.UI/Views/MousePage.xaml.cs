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
    /// CurveBox: 0 standard (power 2), 1 linear, 2 precision (a ready-made Bezier), 3 custom Bezier.
    private static readonly CurveModel Precision = CurveModel.Bezier(0.7f, 0.05f, 0.95f, 0.45f);
    private readonly DispatcherTimer _timer = new() { Interval = TimeSpan.FromMilliseconds(500) };
    private ProfileModel? _model;
    private bool _loading;
    /// The curve controls were changed by hand (otherwise the profile's curve is left exactly as it is).
    private bool _curveTouched;

    public MousePage()
    {
        _loading = true;
        InitializeComponent();
        _loading = false;
        _timer.Tick += (_, _) => ShowFullscreen();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        if (e.Parameter is "embedded") Header.Visibility = Visibility.Collapsed;
        Load();
        ShowFullscreen();
        _timer.Start();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e) => _timer.Stop();

    private void ShowFullscreen()
    {
        var res = Application.Current.Resources;
        bool full = App.Engine?.IsFullscreen == true;
        bool stepping = full && FullscreenToggle.IsOn;
        FullscreenText.Text = stepping ? "A full-screen app is in front: touchpad and stick are handed back to it"
                            : full ? "A full-screen app is in front (mouse control stays on)"
                            : "No full-screen app in front right now";
        FullscreenDot.Fill = (Microsoft.UI.Xaml.Media.Brush)res[stepping ? "DxoWarning" : "DxoTextSecondary"];
    }

    private void Load()
    {
        _model = ProfileStore.Active();
        bool ok = _model is not null;
        foreach (var c in new Control[] { TouchToggle, TouchSpeed, ClickToggle, ScrollToggle, PinchToggle, StickBox, StickSpeed, SaveButton, CurveBox, FullscreenToggle })
            c.IsEnabled = ok;
        if (_model is null) { SaveStatus.Text = "The engine isn't running."; return; }

        var m = _model.Mouse;
        _loading = true;
        TouchToggle.IsOn = m.Touchpad;
        TouchSpeed.Value = Math.Clamp(m.TouchpadSpeed, 0.2, 4);
        ClickToggle.IsOn = m.TouchpadButtons;
        ScrollToggle.IsOn = m.TwoFingerScroll;
        PinchToggle.IsOn = m.PinchZoom;
        StickBox.SelectedIndex = Math.Max(0, Array.IndexOf(Sticks, m.Stick));
        StickSpeed.Value = Math.Clamp(m.StickSpeed, 200, 4000);
        FullscreenToggle.IsOn = m.SuspendInFullscreen;
        var curve = m.StickCurve;
        bool bezier = curve.Type == "bezier";
        bool precise = bezier && curve.X1 == Precision.X1 && curve.Y1 == Precision.Y1 && curve.X2 == Precision.X2 && curve.Y2 == Precision.Y2;
        CurveBox.SelectedIndex = curve.Type == "linear" ? 1 : precise ? 2 : bezier ? 3
                               : curve.Type == "power" && Math.Abs((curve.Exponent ?? 1) - 2) < 0.01 ? 0 : 3;
        var handles = bezier ? curve : Precision;
        CurveX1.Value = handles.X1 ?? 0.7; CurveY1.Value = handles.Y1 ?? 0.05;
        CurveX2.Value = handles.X2 ?? 0.95; CurveY2.Value = handles.Y2 ?? 0.45;
        _curveTouched = false;
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
        m.TwoFingerScroll = ScrollToggle.IsOn;
        m.PinchZoom = PinchToggle.IsOn;
        m.Stick = Sticks[Math.Max(0, StickBox.SelectedIndex)];
        m.StickSpeed = (float)Math.Round(StickSpeed.Value);
        m.SuspendInFullscreen = FullscreenToggle.IsOn;
        if (_curveTouched) m.StickCurve = ChosenCurve();
        SaveStatus.Text = ProfileStore.ApplyActive(_model)
            ? $"Applied. Save to keep it in \"{_model.Name}\"."
            : "The engine rejected that setting.";
        UpdateLabels();
    }

    /// The curve the controls describe.
    private CurveModel ChosenCurve() => CurveBox.SelectedIndex switch
    {
        1 => new CurveModel { Type = "linear" },
        2 => CurveModel.Bezier(Precision.X1!.Value, Precision.Y1!.Value, Precision.X2!.Value, Precision.Y2!.Value),
        3 => CurveModel.Bezier((float)CurveX1.Value, (float)CurveY1.Value, (float)CurveX2.Value, (float)CurveY2.Value),
        _ => CurveModel.Power(2f),
    };

    private void Curve_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (_loading) return;
        _curveTouched = true;
        Push();
    }

    private void CurveSlider_Changed(object sender, RangeBaseValueChangedEventArgs e)
    {
        if (_loading) return;
        _curveTouched = true;
        Push();
    }

    /// Draw the response: stick push left to right, cursor speed bottom to top.
    private void DrawCurve()
    {
        if (CurveLine is null || _model is null) return;
        var curve = _curveTouched ? ChosenCurve() : _model.Mouse.StickCurve;
        var pts = new Microsoft.UI.Xaml.Media.PointCollection();
        for (int i = 0; i <= 30; i++)
        {
            double t = i / 30.0;
            pts.Add(new Windows.Foundation.Point(t * 120, 120 - curve.Apply(t) * 120));
        }
        CurveLine.Points = pts;
    }

    private void UpdateLabels()
    {
        if (TouchSpeedText is null || StickSpeedText is null || CurvePanel is null) return;
        bool stick = _model is not null && StickBox.SelectedIndex > 0;
        CurveBox.IsEnabled = stick;
        CurvePanel.Visibility = CurveBox.SelectedIndex == 3 ? Visibility.Visible : Visibility.Collapsed;
        DrawCurve();
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

using System;
using System.Collections.Generic;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;

namespace DXO.UI.Controls;

/// Trigger pressure as an arc of tick marks that light up from the bottom.
/// Left trigger draws "(", right trigger (Mirror = true) draws ")".
public sealed class TriggerArc : UserControl
{
    private const int Ticks = 56;
    private const double Size = 240;
    private const double OuterR = 112, InnerR = 96;   // tick length = 16
    private const double FromDeg = 62, ToDeg = 298;   // bottom -> around the outside -> top

    private static readonly Brush Off = new SolidColorBrush(ColorHelper.FromArgb(0x40, 0xE4, 0xE1, 0xE9));
    private static readonly Brush On = new SolidColorBrush(ColorHelper.FromArgb(0xFF, 0xB2, 0x8C, 0xFF));
    private static readonly Brush Major = new SolidColorBrush(ColorHelper.FromArgb(0x80, 0xE4, 0xE1, 0xE9));

    private readonly List<Line> _ticks = new();
    private readonly TextBlock _label = new() { FontSize = 13, Opacity = 0.7, HorizontalAlignment = HorizontalAlignment.Center };
    private readonly TextBlock _value = new()
    {
        FontSize = 22, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
        FontFamily = new FontFamily("Cascadia Mono"), HorizontalAlignment = HorizontalAlignment.Center,
    };
    private int _lit = -1;

    public bool Mirror { get; set; }
    public string Label { get => _label.Text; set => _label.Text = value; }

    public TriggerArc()
    {
        Loaded += (_, _) => Build();
    }

    private void Build()
    {
        if (_ticks.Count > 0) return;
        var canvas = new Canvas { Width = Size, Height = Size };
        double c = Size / 2;
        for (int i = 0; i < Ticks; i++)
        {
            double t = i / (double)(Ticks - 1);
            double deg = FromDeg + (ToDeg - FromDeg) * t;
            double rad = deg * Math.PI / 180;
            double dx = Math.Cos(rad), dy = Math.Sin(rad);
            if (Mirror) dx = -dx; // 180 deg points left, so unmirrored = "(" (L2), mirrored = ")" (R2)
            bool major = i % ((Ticks - 1) / 4) == 0; // 0, 25, 50, 75, 100 %
            double outer = major ? OuterR + 6 : OuterR;
            var line = new Line
            {
                // screen Y grows downward, so +sin = lower: the fill starts at the bottom
                X1 = c + dx * InnerR, Y1 = c + dy * InnerR,
                X2 = c + dx * outer, Y2 = c + dy * outer,
                Stroke = major ? Major : Off,
                StrokeThickness = major ? 3 : 2.2,
                StrokeStartLineCap = PenLineCap.Round,
                StrokeEndLineCap = PenLineCap.Round,
            };
            canvas.Children.Add(line);
            _ticks.Add(line);
        }

        var centre = new StackPanel { Spacing = 2, VerticalAlignment = VerticalAlignment.Center, HorizontalAlignment = HorizontalAlignment.Center };
        centre.Children.Add(_label);
        centre.Children.Add(_value);

        var root = new Grid { Width = Size, Height = Size };
        root.Children.Add(canvas);
        root.Children.Add(centre);
        Content = root;
        SetValue(0);
    }

    /// 0.0 .. 1.0
    public void SetValue(double v)
    {
        v = Math.Clamp(v, 0, 1);
        _value.Text = $"{v * 100:0}%";
        int lit = (int)Math.Round(v * Ticks);
        if (lit == _lit || _ticks.Count == 0) return; // only repaint when a tick changes
        _lit = lit;
        int majorStep = (Ticks - 1) / 4;
        for (int i = 0; i < _ticks.Count; i++)
            _ticks[i].Stroke = i < lit ? On : (i % majorStep == 0 ? Major : Off);
    }
}

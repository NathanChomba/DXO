using System;
using System.Collections.Generic;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using DXO.UI.Native;

namespace DXO.UI.Overlay;

/// The quick menu: a ring of actions in the middle of the screen. As with
/// the keyboard, the engine owns it (crates/dxo-core/src/radial.rs) and this
/// window only draws which items there are and which one is pointed at.
public sealed class RadialOverlay : OverlayWindow
{
    private const double Design = 660, Ring = 240, PillHeight = 50;

    private readonly Canvas _canvas = new() { Width = Design, Height = Design };
    private readonly TextBlock _centre;
    private readonly List<(Border Box, TextBlock Text, string Label)> _items = new();
    private readonly SolidColorBrush _itemBrush, _highlightBrush, _flashBrush, _textBrush, _darkTextBrush;

    private uint _rev;
    private bool _built;
    private (int Selected, bool Flash) _painted = (-2, false);
    private uint _fired;
    private DateTime _flashUntil;

    public RadialOverlay()
    {
        Title = "DXO quick menu";
        _itemBrush = new SolidColorBrush(Windows.UI.Color.FromArgb(255, 0x2A, 0x29, 0x2F));
        _highlightBrush = new SolidColorBrush(Windows.UI.Color.FromArgb(255, 0xA0, 0x78, 0xFF));
        _flashBrush = new SolidColorBrush(Windows.UI.Color.FromArgb(255, 0xD0, 0xBC, 0xFF));
        _textBrush = new SolidColorBrush(Windows.UI.Color.FromArgb(255, 0xE4, 0xE1, 0xE9));
        _darkTextBrush = new SolidColorBrush(Windows.UI.Color.FromArgb(255, 0x13, 0x13, 0x18));
        var muted = new SolidColorBrush(Windows.UI.Color.FromArgb(255, 0x95, 0x8E, 0xA0));

        // What the stick is pointing at, in the middle of the ring.
        _centre = new TextBlock
        {
            FontSize = 24,
            FontWeight = Microsoft.UI.Text.FontWeights.SemiBold,
            Foreground = _textBrush,
            TextAlignment = TextAlignment.Center,
            TextWrapping = TextWrapping.Wrap,
            Width = 260,
        };
        Canvas.SetLeft(_centre, (Design - 260) / 2);
        Canvas.SetTop(_centre, Design / 2 - 34);
        _canvas.Children.Add(_centre);

        var hint = new TextBlock
        {
            Text = "Point with a stick and let go, or press ✕      ○  Close",
            FontSize = 14,
            Foreground = muted,
            TextAlignment = TextAlignment.Center,
            Width = 360,
        };
        Canvas.SetLeft(hint, (Design - 360) / 2);
        Canvas.SetTop(hint, Design / 2 + 34);
        _canvas.Children.Add(hint);

        var root = new Grid { Background = new SolidColorBrush(Windows.UI.Color.FromArgb(255, 0x13, 0x13, 0x18)) };
        root.Children.Add(new Viewbox { Stretch = Stretch.Uniform, Child = _canvas });
        Content = root;
    }

    /// Size (1.0 = 100%, the keyboard's size setting) and position: screen centre.
    public void Place(double scale) => PlaceCentre(0.5 * Math.Clamp(scale, 0.5, 1.6));

    public void Update(DxoRadial r, DxoEngine engine)
    {
        if (!_built || r.Rev != _rev)
        {
            Build(engine.RadialLayoutJson);
            _rev = r.Rev;
            _built = true;
        }
        var now = DateTime.UtcNow;
        if (r.Fired != _fired)
        {
            _fired = r.Fired;
            _flashUntil = now.AddMilliseconds(120);
        }
        int selected = r.Selected == 255 ? -1 : r.Selected;
        var state = (Selected: selected, Flash: now < _flashUntil);
        if (state == _painted) return;
        _painted = state;

        for (int i = 0; i < _items.Count; i++)
        {
            bool on = i == selected;
            _items[i].Box.Background = on ? (state.Flash ? _flashBrush : _highlightBrush) : _itemBrush;
            _items[i].Text.Foreground = on ? _darkTextBrush : _textBrush;
        }
        _centre.Text = selected >= 0 && selected < _items.Count ? _items[selected].Label : "Quick menu";
    }

    /// Lay the items out round the ring: the first at the top, then clockwise.
    private void Build(string json)
    {
        foreach (var it in _items) _canvas.Children.Remove(it.Box);
        _items.Clear();
        _painted = (-2, false);
        if (string.IsNullOrEmpty(json)) return;

        using var doc = JsonDocument.Parse(json);
        var items = doc.RootElement.GetProperty("items");
        int n = items.GetArrayLength();
        if (n == 0) return;
        double width = n > 8 ? 120 : 156;
        int i = 0;
        foreach (var item in items.EnumerateArray())
        {
            string label = item.GetProperty("label").GetString() ?? "";
            var text = new TextBlock
            {
                Text = label,
                FontSize = n > 8 ? 13 : 15,
                Foreground = _textBrush,
                HorizontalAlignment = HorizontalAlignment.Center,
                VerticalAlignment = VerticalAlignment.Center,
                TextTrimming = TextTrimming.CharacterEllipsis,
                Margin = new Thickness(8, 0, 8, 0),
            };
            var box = new Border
            {
                Width = width,
                Height = PillHeight,
                CornerRadius = new CornerRadius(PillHeight / 2),
                Background = _itemBrush,
                Child = text,
            };
            double angle = 2 * Math.PI * i / n; // clockwise from the top
            Canvas.SetLeft(box, Design / 2 + Ring * Math.Sin(angle) - width / 2);
            Canvas.SetTop(box, Design / 2 - Ring * Math.Cos(angle) - PillHeight / 2);
            _canvas.Children.Add(box);
            _items.Add((box, text, label));
            i++;
        }
    }
}

using System;
using System.Collections.Generic;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using DXO.UI.Native;

namespace DXO.UI.Overlay;

/// DXO's on-screen keyboard: the picture only. The engine owns the keyboard
/// (crates/dxo-core/src/osk.rs): it reads the pad, moves the highlight and
/// types. This window asks the engine for the keys of the layer on show and
/// where the highlight is, and draws that.
public sealed class KeyboardOverlay : OverlayWindow
{
    // The keyboard is laid out once at this size and scaled to the window by a Viewbox,
    // so it looks the same at every size setting and on every screen scale.
    private const double DesignW = 1088, DesignH = 452;
    /// Share of the screen's width at size 100%.
    private const double BaseWidth = 0.5;

    private sealed record KeyCell(Border Box, TextBlock Text, string Kind);

    private readonly Grid _keys = new();
    private readonly TextBlock _hint;
    private readonly List<List<KeyCell>> _rows = new();

    private readonly SolidColorBrush _keyBrush, _actionBrush, _highlightBrush, _flashBrush, _onBrush;
    private readonly SolidColorBrush _textBrush, _darkTextBrush;

    private int _layer = -1;
    private (int Row, int Key, bool Flash, int Shift, int Layer) _painted = (-1, -1, false, -1, -1);
    private uint _presses;
    private DateTime _flashUntil;

    public KeyboardOverlay()
    {
        Title = "DXO keyboard";

        _keyBrush = new SolidColorBrush(Colour("DxoSurfaceRaisedColor", 0x2A, 0x29, 0x2F));
        _actionBrush = new SolidColorBrush(Colour("DxoSurfaceColor", 0x1F, 0x1F, 0x24));
        _highlightBrush = new SolidColorBrush(Colour("DxoAccentColor", 0xA0, 0x78, 0xFF));
        _flashBrush = new SolidColorBrush(Colour("DxoAccentLightColor", 0xD0, 0xBC, 0xFF));
        _onBrush = new SolidColorBrush(Colour("DxoSecondaryColor", 0x4C, 0xD7, 0xF6));
        _textBrush = new SolidColorBrush(Colour("DxoTextColor", 0xE4, 0xE1, 0xE9));
        _darkTextBrush = new SolidColorBrush(Colour("DxoWindowBottomColor", 0x13, 0x13, 0x18));
        var muted = new SolidColorBrush(Colour("DxoTextMutedColor", 0x95, 0x8E, 0xA0));
        var background = new SolidColorBrush(Colour("DxoWindowBottomColor", 0x13, 0x13, 0x18));

        var design = new Grid { Width = DesignW, Height = DesignH, Padding = new Thickness(14), RowSpacing = 8 };
        design.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        design.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        design.Children.Add(_keys);

        _hint = new TextBlock
        {
            FontSize = 15,
            Foreground = muted,
            HorizontalAlignment = HorizontalAlignment.Center,
            TextTrimming = TextTrimming.CharacterEllipsis,
        };
        Grid.SetRow(_hint, 1);
        design.Children.Add(_hint);

        var root = new Grid { Background = background };
        root.Children.Add(new Viewbox { Stretch = Stretch.Uniform, Child = design });
        Content = root;
        SetHints(0);
    }

    private static Windows.UI.Color Colour(string key, byte r, byte g, byte b)
    {
        try
        {
            if (Application.Current.Resources.TryGetValue(key, out object? v) && v is Windows.UI.Color c) return c;
        }
        catch { /* fall through to the built-in colour */ }
        return Windows.UI.Color.FromArgb(255, r, g, b);
    }

    /// Size (1.0 = 100%) and position: bottom centre of the screen in use.
    public void Place(double scale) =>
        PlaceBottomCentre(BaseWidth * Math.Clamp(scale, 0.5, 1.6), DesignH / DesignW);

    /// The footer naming the pad shortcuts, in the words printed on this pad.
    /// kind: the engine's device kind (5 = Switch Pro; everything else gets
    /// PlayStation names, like the rest of DXO).
    public void SetHints(byte kind)
    {
        _hint.Text = kind == 5
            ? "B  Select      A  Close      Y  Backspace      X  Space      +  Enter      L stick  Shift      ZL  Symbols      L / R  Move cursor"
            : "✕  Select      ○  Close      □  Backspace      △  Space      OPTIONS  Enter      L3  Shift      L2  Symbols      L1 / R1  Move cursor";
    }

    /// Draw the engine's current state. Cheap when nothing changed.
    public void Update(DxoOsk o, DxoEngine engine)
    {
        if (o.Layer != _layer || _rows.Count == 0)
        {
            ApplyLayout(engine.OskLayoutJson);
            _layer = o.Layer;
        }
        var now = DateTime.UtcNow;
        if (o.Presses != _presses)
        {
            _presses = o.Presses;
            _flashUntil = now.AddMilliseconds(90);
        }
        var state = (Row: (int)o.Row, Key: (int)o.Key, Flash: now < _flashUntil, Shift: (int)o.Shift, Layer: (int)o.Layer);
        if (state == _painted) return;
        _painted = state;

        for (int r = 0; r < _rows.Count; r++)
        {
            for (int k = 0; k < _rows[r].Count; k++)
            {
                var cell = _rows[r][k];
                bool highlighted = r == state.Row && k == state.Key;
                bool on = (cell.Kind == "shift" && state.Shift != 0) || (cell.Kind == "symbols" && state.Layer == 2);
                cell.Box.Background = highlighted
                    ? (state.Flash ? _flashBrush : _highlightBrush)
                    : cell.Kind == "char" ? _keyBrush : _actionBrush;
                cell.Text.Foreground = highlighted ? _darkTextBrush : on ? _onBrush : _textBrush;
                // Caps lock: underline the Shift key so it reads differently from "next letter".
                if (cell.Kind == "shift")
                    cell.Text.TextDecorations = state.Shift == 2 ? Windows.UI.Text.TextDecorations.Underline : Windows.UI.Text.TextDecorations.None;
            }
        }
    }

    /// Build the key grid the first time; afterwards only the labels change
    /// (every layer has the same shape).
    private void ApplyLayout(string json)
    {
        if (string.IsNullOrEmpty(json)) return;
        using var doc = JsonDocument.Parse(json);
        var root = doc.RootElement;
        var rows = root.GetProperty("rows");

        if (_rows.Count == 0)
        {
            int cols = root.GetProperty("cols").GetInt32();
            for (int c = 0; c < cols; c++)
                _keys.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            int r = 0;
            foreach (var row in rows.EnumerateArray())
            {
                _keys.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
                var cells = new List<KeyCell>();
                foreach (var key in row.EnumerateArray())
                {
                    string kind = key.GetProperty("kind").GetString() ?? "char";
                    var text = new TextBlock
                    {
                        FontSize = kind == "char" ? 28 : 20,
                        Foreground = _textBrush,
                        HorizontalAlignment = HorizontalAlignment.Center,
                        VerticalAlignment = VerticalAlignment.Center,
                    };
                    var box = new Border
                    {
                        CornerRadius = new CornerRadius(8),
                        Margin = new Thickness(3),
                        Background = kind == "char" ? _keyBrush : _actionBrush,
                        Child = text,
                    };
                    // Mouse click or touch tap: the window never takes focus, so the
                    // key still lands in the app behind it.
                    int clickRow = r, clickKey = cells.Count;
                    box.PointerPressed += (_, e) =>
                    {
                        App.Engine?.OskClick(clickRow, clickKey);
                        e.Handled = true;
                    };
                    Grid.SetRow(box, r);
                    Grid.SetColumn(box, key.GetProperty("col").GetInt32());
                    Grid.SetColumnSpan(box, Math.Max(1, key.GetProperty("span").GetInt32()));
                    _keys.Children.Add(box);
                    cells.Add(new KeyCell(box, text, kind));
                }
                _rows.Add(cells);
                r++;
            }
        }

        int ri = 0;
        foreach (var row in rows.EnumerateArray())
        {
            if (ri >= _rows.Count) break;
            int ki = 0;
            foreach (var key in row.EnumerateArray())
            {
                if (ki >= _rows[ri].Count) break;
                _rows[ri][ki].Text.Text = key.GetProperty("label").GetString() ?? "";
                ki++;
            }
            ri++;
        }
        _painted = (-1, -1, false, -1, -1); // repaint with the new labels
    }
}

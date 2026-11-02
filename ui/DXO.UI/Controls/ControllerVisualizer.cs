using System;
using System.Collections.Generic;
using System.IO;
using IOPath = System.IO.Path; // Microsoft.UI.Xaml.Shapes also has a Path
using System.Text.Json;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.UI.Xaml.Shapes;
using DXO.UI.Native;

namespace DXO.UI.Controls;

/// Draws a controller skin (line-art image + JSON hit-zones from the Skins folder)
/// and lights up whatever is pressed. Skins live next to the exe in Skins\:
///   ds4.png + ds4.json   -> image plus hit-zones
///   xbox-one.png alone    -> image only, nothing lights up yet
/// Hit-zone coordinates are in the image's own pixels; the whole thing scales
/// to fit via a Viewbox.
public sealed class ControllerVisualizer : UserControl
{
    private readonly Viewbox _box = new() { Stretch = Stretch.Uniform };
    private readonly Grid _stage = new();
    private readonly Canvas _overlay = new();
    private readonly Dictionary<string, Shape> _zones = new();
    private readonly Dictionary<string, (Ellipse dot, double cx, double cy, double travel)> _sticks = new();
    private string? _loadedKey;

    private static readonly Brush PressFill = new SolidColorBrush(ColorHelper.FromArgb(0x99, 0x8B, 0x5C, 0xF6));
    private static readonly Brush PressStroke = new SolidColorBrush(ColorHelper.FromArgb(0xFF, 0xB2, 0x8C, 0xFF));
    private static readonly Brush StickDot = new SolidColorBrush(ColorHelper.FromArgb(0xFF, 0x4C, 0xD7, 0xF6));

    /// Hit-zone name -> engine button bit (state.rs `Buttons`).
    private static readonly Dictionary<string, DxoButtons> ZoneBits = new()
    {
        ["btn_south"] = DxoButtons.South, ["btn_east"] = DxoButtons.East,
        ["btn_west"] = DxoButtons.West, ["btn_north"] = DxoButtons.North,
        ["dpad_up"] = DxoButtons.Up, ["dpad_down"] = DxoButtons.Down,
        ["dpad_left"] = DxoButtons.Left, ["dpad_right"] = DxoButtons.Right,
        ["shoulder_l"] = DxoButtons.L1, ["shoulder_r"] = DxoButtons.R1,
        ["btn_select"] = DxoButtons.Select, ["btn_start"] = DxoButtons.Start,
        ["btn_home"] = DxoButtons.Home, ["touchpad"] = DxoButtons.Touchpad,
        ["stick_l"] = DxoButtons.L3, ["stick_r"] = DxoButtons.R3,
        ["paddle_l"] = DxoButtons.PaddleL, ["paddle_r"] = DxoButtons.PaddleR,
    };

    public ControllerVisualizer()
    {
        _stage.Children.Add(_overlay);
        _box.Child = _stage;
        Content = _box;
    }

    /// Skin key for an engine device kind (DxoState.Kind).
    public static string SkinFor(byte kind) => kind switch
    {
        1 => "ds4",
        2 or 3 => "dualsense",
        _ => "ds4",
    };

    public void LoadSkin(string key)
    {
        if (key == _loadedKey) return;
        _loadedKey = key;
        _zones.Clear();
        _sticks.Clear();
        _overlay.Children.Clear();
        _stage.Children.Clear();

        string dir = IOPath.Combine(AppContext.BaseDirectory, "Skins");
        string json = IOPath.Combine(dir, key + ".json");
        string png = IOPath.Combine(dir, key + ".png");

        double w = 1600, h = 1000;
        string image = png;
        JsonElement controls = default;
        bool hasControls = false;

        if (File.Exists(json))
        {
            try
            {
                using var doc = JsonDocument.Parse(File.ReadAllText(json));
                var root = doc.RootElement;
                w = root.GetProperty("width").GetDouble();
                h = root.GetProperty("height").GetDouble();
                image = IOPath.Combine(dir, root.GetProperty("image").GetString() ?? key + ".png");
                controls = root.GetProperty("controls").Clone();
                hasControls = true;
            }
            catch (Exception ex)
            {
                App.LogCrash($"Skin {key}.json", ex); // bad JSON: show the image without zones
            }
        }

        _stage.Width = w;
        _stage.Height = h;
        _overlay.Width = w;
        _overlay.Height = h;

        if (File.Exists(image))
            _stage.Children.Add(new Image { Source = new BitmapImage(new Uri(image)), Stretch = Stretch.Fill, Width = w, Height = h });
        _stage.Children.Add(_overlay);

        if (!hasControls) return;
        foreach (var c in controls.EnumerateObject())
        {
            var v = c.Value;
            string shape = v.GetProperty("shape").GetString() ?? "circle";
            double x = v.GetProperty("x").GetDouble(), y = v.GetProperty("y").GetDouble();

            Shape zone;
            if (shape == "rect")
            {
                double zw = v.GetProperty("w").GetDouble(), zh = v.GetProperty("h").GetDouble();
                double r = v.TryGetProperty("r", out var rr) ? rr.GetDouble() : 8;
                zone = new Rectangle { Width = zw, Height = zh, RadiusX = r, RadiusY = r };
                Canvas.SetLeft(zone, x - zw / 2);
                Canvas.SetTop(zone, y - zh / 2);
            }
            else
            {
                double r = v.GetProperty("r").GetDouble();
                zone = new Ellipse { Width = r * 2, Height = r * 2 };
                Canvas.SetLeft(zone, x - r);
                Canvas.SetTop(zone, y - r);
            }
            zone.Fill = PressFill;
            zone.Stroke = PressStroke;
            zone.StrokeThickness = 4;
            zone.Opacity = 0;
            _overlay.Children.Add(zone);
            _zones[c.Name] = zone;

            if (shape == "stick")
            {
                double travel = v.TryGetProperty("travel", out var t) ? t.GetDouble() : 40;
                var dot = new Ellipse { Width = 30, Height = 30, Fill = StickDot };
                _overlay.Children.Add(dot);
                _sticks[c.Name] = (dot, x, y, travel);
                MoveDot(dot, x, y);
            }
        }
    }

    /// Call at UI rate with the latest engine state.
    public void Update(uint buttons, float lx, float ly, float rx, float ry)
    {
        foreach (var (name, zone) in _zones)
        {
            bool on = ZoneBits.TryGetValue(name, out var bit) && (buttons & (uint)bit) != 0;
            zone.Opacity = on ? 1 : 0;
        }
        Stick("stick_l", lx, ly);
        Stick("stick_r", rx, ry);
    }

    private void Stick(string name, float x, float y)
    {
        if (!_sticks.TryGetValue(name, out var s)) return;
        // Engine: +Y is up; screen: +Y is down.
        MoveDot(s.dot, s.cx + x * s.travel, s.cy - y * s.travel);
    }

    private static void MoveDot(Ellipse dot, double cx, double cy)
    {
        Canvas.SetLeft(dot, cx - dot.Width / 2);
        Canvas.SetTop(dot, cy - dot.Height / 2);
    }
}

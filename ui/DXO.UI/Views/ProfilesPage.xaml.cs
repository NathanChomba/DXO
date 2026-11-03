using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.IO;
using System.Linq;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;
using DXO.UI.Native;
using DXO.UI.ViewModels;
using IOPath = System.IO.Path;

namespace DXO.UI.Views;

/// One row in the profile list.
public sealed class ProfileEntry
{
    public string Path { get; init; } = "";
    public ProfileModel Model { get; set; } = new();
    public bool IsActive { get; set; }
    public string Title => Model.Name + (IsActive ? "  •  active" : "");
    public string Subtitle => Model.MatchProcess.Count == 0 ? "Manual" : string.Join(", ", Model.MatchProcess);
}

public sealed partial class ProfilesPage : Page
{
    private readonly ObservableCollection<ProfileEntry> _entries = new();
    private ProfileEntry? _current;
    private bool _loading;
    private bool _dirty;


    public ProfilesPage()
    {
        // Building the XAML fires ValueChanged etc. (a slider with Minimum=0.5
        // bumps its Value up from 0) before every control exists - ignore those.
        _loading = true;
        InitializeComponent();
        _loading = false;
        List.ItemsSource = _entries;
    }

    protected override void OnNavigatedTo(NavigationEventArgs e) => LoadList(selectName: null);

    // ================= list =================

    private string ActiveName()
    {
        var json = App.Engine?.ActiveProfileJson ?? "";
        return ProfileModel.FromJson(json)?.Name ?? "";
    }

    private void LoadList(string? selectName)
    {
        _entries.Clear();
        string active = ActiveName();
        Directory.CreateDirectory(App.ProfilesDir);
        foreach (var file in Directory.GetFiles(App.ProfilesDir, "*.toml").OrderBy(f => f))
        {
            var model = ProfileModel.FromJson(DxoEngine.ProfileFileToJson(file));
            if (model is null) continue; // unreadable file: skip, the engine logs why
            _entries.Add(new ProfileEntry { Path = file, Model = model, IsActive = model.Name == active });
        }
        var pick = _entries.FirstOrDefault(x => x.Model.Name == (selectName ?? active)) ?? _entries.FirstOrDefault();
        List.SelectedItem = pick;
        if (pick is null) ShowEditor(null);
    }

    private async void List_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        var next = List.SelectedItem as ProfileEntry;
        if (next == _current) return;
        if (_dirty && _current is not null && !await ConfirmDiscard())
        {
            _loading = true; List.SelectedItem = _current; _loading = false;
            return;
        }
        ShowEditor(next);
    }

    private async System.Threading.Tasks.Task<bool> ConfirmDiscard()
    {
        var d = new ContentDialog
        {
            XamlRoot = XamlRoot, Title = "Discard unsaved changes?",
            Content = $"Changes to \"{_current?.Model.Name}\" haven't been saved.",
            PrimaryButtonText = "Discard", CloseButtonText = "Keep editing", DefaultButton = ContentDialogButton.Close,
        };
        return await d.ShowAsync() == ContentDialogResult.Primary;
    }

    // ================= editor <-> model =================

    private void ShowEditor(ProfileEntry? entry)
    {
        _current = entry;
        Editor.Visibility = entry is null ? Visibility.Collapsed : Visibility.Visible;
        SaveButton.IsEnabled = UseButton.IsEnabled = DuplicateButton.IsEnabled = DeleteButton.IsEnabled = entry is not null;
        if (entry is null) { StatusText.Text = "No profiles yet. Click New."; return; }

        var m = entry.Model;
        _loading = true;
        NameBox.Text = m.Name;
        GamesBox.Text = string.Join(", ", m.MatchProcess);
        LoadStick(m.LeftStick, LDead, LOuter, LCurve, LCurveParam, LInvert);
        LoadStick(m.RightStick, RDead, ROuter, RCurve, RCurveParam, RInvert);
        TrigDead.Value = m.TriggerDeadzone;
        MappingsSummary.Text = ButtonCatalog.Summary(m);
        MappingsCard.Header = entry.IsActive ? "Button mappings" : "Button mappings (opens this profile on the Mappings page)";
        var c = ColorHelper.FromArgb(255, (byte)m.Lightbar[0], (byte)m.Lightbar[1], (byte)m.Lightbar[2]);
        LightPicker.Color = c;
        ColorSwatch.Background = new SolidColorBrush(c);
        RumbleToggle.IsOn = m.RumblePassthrough;
        _loading = false;
        SetDirty(false);
        EditorScroll.ChangeView(null, 0, null, true);
    }

    private static void LoadStick(StickModel s, Slider dead, Slider outer, ComboBox curve, Slider param, ToggleSwitch invert)
    {
        dead.Value = s.Deadzone;
        outer.Value = s.Outer;
        curve.SelectedIndex = s.Curve.Type switch { "power" => 1, "smooth" => 2, _ => 0 };
        ConfigureCurveParam(curve, param);
        param.Value = s.Curve.Type switch
        {
            "power" => s.Curve.Exponent ?? 1.5,
            "smooth" => s.Curve.Amount ?? 0.4,
            _ => param.Minimum,
        };
        invert.IsOn = s.InvertY;
    }

    /// Power uses an exponent (0.2-3), Smooth a blend amount (0-1), Linear none.
    private static void ConfigureCurveParam(ComboBox curve, Slider param)
    {
        switch (curve.SelectedIndex)
        {
            case 1: param.Minimum = 0.2; param.Maximum = 3; param.StepFrequency = 0.05; param.IsEnabled = true; break;
            case 2: param.Minimum = 0; param.Maximum = 1; param.StepFrequency = 0.05; param.IsEnabled = true; break;
            default: param.IsEnabled = false; break;
        }
    }

    private ProfileModel ReadEditor()
    {
        var m = _current!.Model.Clone(); // keeps fields this page doesn't edit (gyro, LEDs...)
        m.Name = string.IsNullOrWhiteSpace(NameBox.Text) ? "Untitled" : NameBox.Text.Trim();
        m.MatchProcess = GamesBox.Text.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Select(x => x.EndsWith(".exe", StringComparison.OrdinalIgnoreCase) ? x : x + ".exe").Distinct().ToList();
        m.LeftStick = ReadStick(LDead, LOuter, LCurve, LCurveParam, LInvert);
        m.RightStick = ReadStick(RDead, ROuter, RCurve, RCurveParam, RInvert);
        m.TriggerDeadzone = (float)TrigDead.Value;
        var c = LightPicker.Color;
        m.Lightbar = new[] { (int)c.R, c.G, c.B };
        m.RumblePassthrough = RumbleToggle.IsOn;
        return m;
    }

    private static StickModel ReadStick(Slider dead, Slider outer, ComboBox curve, Slider param, ToggleSwitch invert) => new()
    {
        Deadzone = (float)dead.Value,
        Outer = (float)Math.Max(outer.Value, dead.Value + 0.05),
        InvertY = invert.IsOn,
        Curve = curve.SelectedIndex switch
        {
            1 => new CurveModel { Type = "power", Exponent = (float)param.Value },
            2 => new CurveModel { Type = "smooth", Amount = (float)param.Value },
            _ => new CurveModel { Type = "linear" },
        },
    };

    // ================= remap rows =================

    /// Mappings are edited on their own page, which works on the active
    /// profile: make this one active (saving it first), then go there.
    private void EditMappings_Click(object sender, RoutedEventArgs e)
    {
        if (_current is null) return;
        if (!_current.IsActive || _dirty)
        {
            var m = Save();
            if (m is null) return;
            if (App.Engine?.SetActiveProfileJson(m.ToJson()) != true) { StatusText.Text = "Couldn't activate this profile."; return; }
            _dirty = false;
        }
        MainWindow.NavigateTo("mappings");
    }

    // ================= change tracking =================

    private void SetDirty(bool d)
    {
        _dirty = d;
        if (StatusText is null) return;
        StatusText.Text = d ? "Unsaved changes" : (_current?.IsActive == true ? "This is the active profile" : "");
    }

    private void Text_Changed(object sender, TextChangedEventArgs e) { if (!_loading) SetDirty(true); }
    private void Combo_Changed(object sender, SelectionChangedEventArgs e) { if (!_loading) SetDirty(true); }
    private void Toggle_Changed(object sender, RoutedEventArgs e) { if (!_loading) SetDirty(true); }
    private void Slider_Changed(object sender, RangeBaseValueChangedEventArgs e) { if (!_loading) SetDirty(true); }

    private void Curve_Changed(object sender, SelectionChangedEventArgs e)
    {
        var param = ReferenceEquals(sender, LCurve) ? LCurveParam : RCurveParam;
        if (param is null) return; // still constructing
        ConfigureCurveParam((ComboBox)sender, param);
        if (!_loading) SetDirty(true);
    }

    private void LightPicker_ColorChanged(ColorPicker sender, ColorChangedEventArgs args)
    {
        if (ColorSwatch is null) return;
        ColorSwatch.Background = new SolidColorBrush(args.NewColor);
        if (!_loading) SetDirty(true);
    }

    // ================= actions =================

    private static string FileFor(string name)
    {
        var safe = new string(name.Select(ch => IOPath.GetInvalidFileNameChars().Contains(ch) ? '_' : ch).ToArray()).Trim();
        return IOPath.Combine(App.ProfilesDir, (safe.Length == 0 ? "profile" : safe) + ".toml");
    }

    /// Save the editor to disk. Returns the saved model, or null on failure.
    private ProfileModel? Save()
    {
        if (_current is null) return null;
        var m = ReadEditor();
        string newPath = FileFor(m.Name);
        if (!string.Equals(newPath, _current.Path, StringComparison.OrdinalIgnoreCase) && File.Exists(newPath))
        {
            StatusText.Text = $"Another profile is already called \"{m.Name}\".";
            return null;
        }
        if (!DxoEngine.ProfileJsonToFile(m.ToJson(), newPath))
        {
            StatusText.Text = "Couldn't save: the engine rejected this profile.";
            return null;
        }
        if (!string.Equals(newPath, _current.Path, StringComparison.OrdinalIgnoreCase) && File.Exists(_current.Path))
            File.Delete(_current.Path); // renamed
        App.Engine?.ReloadProfiles(); // per-game auto-switch sees the change
        if (_current.IsActive) App.Engine?.SetActiveProfileJson(m.ToJson()); // keep the live one in sync
        return m;
    }

    private void Save_Click(object sender, RoutedEventArgs e)
    {
        var m = Save();
        if (m is null) return;
        _dirty = false;
        LoadList(m.Name);
        StatusText.Text = "Saved";
    }

    private void Use_Click(object sender, RoutedEventArgs e)
    {
        var m = Save();
        if (m is null) return;
        if (App.Engine?.SetActiveProfileJson(m.ToJson()) != true) { StatusText.Text = "Saved, but couldn't activate it."; return; }
        App.Device.ProfileName = m.Name;
        _dirty = false;
        LoadList(m.Name);
        StatusText.Text = "Saved and active";
    }

    private void New_Click(object sender, RoutedEventArgs e) => CreateFrom(new ProfileModel(), "New profile");

    private void Duplicate_Click(object sender, RoutedEventArgs e)
    {
        if (_current is null) return;
        var copy = _current.Model.Clone();
        copy.MatchProcess.Clear(); // two profiles claiming the same game would fight
        CreateFrom(copy, _current.Model.Name + " copy");
    }

    private void CreateFrom(ProfileModel m, string baseName)
    {
        string name = baseName; int n = 2;
        while (File.Exists(FileFor(name))) name = $"{baseName} {n++}";
        m.Name = name;
        if (!DxoEngine.ProfileJsonToFile(m.ToJson(), FileFor(name))) { StatusText.Text = "Couldn't create the profile."; return; }
        _dirty = false;
        App.Engine?.ReloadProfiles();
        LoadList(name);
    }

    private async void Delete_Click(object sender, RoutedEventArgs e)
    {
        if (_current is null) return;
        if (_current.IsActive) { StatusText.Text = "Switch to another profile before deleting the active one."; return; }
        var d = new ContentDialog
        {
            XamlRoot = XamlRoot, Title = $"Delete \"{_current.Model.Name}\"?", Content = "This can't be undone.",
            PrimaryButtonText = "Delete", CloseButtonText = "Cancel", DefaultButton = ContentDialogButton.Close,
        };
        if (await d.ShowAsync() != ContentDialogResult.Primary) return;
        File.Delete(_current.Path);
        _dirty = false;
        App.Engine?.ReloadProfiles();
        LoadList(null);
    }

    private void OpenFolder_Click(object sender, RoutedEventArgs e) =>
        Process.Start(new ProcessStartInfo("explorer.exe", $"\"{App.ProfilesDir}\"") { UseShellExecute = true });
}

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
using DXO.UI.Services;
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
        LoadStick(m.LeftStick, LDead, LOuter, LCurve, LCurveParam, LInvert, LAnti);
        LoadStick(m.RightStick, RDead, ROuter, RCurve, RCurveParam, RInvert, RAnti);
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

    /// Curve box: 0 linear, 1 power, 2 smooth, 3-5 presets (they fill in a
    /// power curve), 6 = a hand-written curve from the file, kept as it is.
    private const int CurveCustom = 6;
    private static readonly double[] PresetExponent = { 1.8, 0.6, 1.3 }; // precision, aggressive, gentle

    private static void LoadStick(StickModel s, Slider dead, Slider outer, ComboBox curve, Slider param, ToggleSwitch invert, Slider anti)
    {
        dead.Value = s.Deadzone;
        outer.Value = s.Outer;
        curve.SelectedIndex = s.Curve.Type switch { "power" => 1, "smooth" => 2, "linear" => 0, _ => CurveCustom };
        ConfigureCurveParam(curve, param);
        param.Value = s.Curve.Type switch
        {
            "power" => s.Curve.Exponent ?? 1.5,
            "smooth" => s.Curve.Amount ?? 0.4,
            _ => param.Minimum,
        };
        invert.IsOn = s.InvertY;
        anti.Value = Math.Clamp(s.AntiDeadzone, 0, 0.4);
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
        m.LeftStick = ReadStick(m.LeftStick, LDead, LOuter, LCurve, LCurveParam, LInvert, LAnti);
        m.RightStick = ReadStick(m.RightStick, RDead, ROuter, RCurve, RCurveParam, RInvert, RAnti);
        m.TriggerDeadzone = (float)TrigDead.Value;
        var c = LightPicker.Color;
        m.Lightbar = new[] { (int)c.R, c.G, c.B };
        m.RumblePassthrough = RumbleToggle.IsOn;
        return m;
    }

    private static StickModel ReadStick(StickModel was, Slider dead, Slider outer, ComboBox curve, Slider param, ToggleSwitch invert, Slider anti) => new()
    {
        Deadzone = (float)dead.Value,
        Outer = (float)Math.Max(outer.Value, dead.Value + 0.05),
        InvertY = invert.IsOn,
        AntiDeadzone = (float)Math.Round(anti.Value, 2),
        Curve = curve.SelectedIndex switch
        {
            1 => new CurveModel { Type = "power", Exponent = (float)param.Value },
            2 => new CurveModel { Type = "smooth", Amount = (float)param.Value },
            CurveCustom => was.Curve, // not editable here: keep what the file says
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
        var box = (ComboBox)sender;
        var param = ReferenceEquals(sender, LCurve) ? LCurveParam : RCurveParam;
        if (param is null) return; // still constructing
        // A preset is a ready-made power curve: switch to Power and fill it in.
        if (!_loading && box.SelectedIndex is >= 3 and < CurveCustom)
        {
            double exponent = PresetExponent[box.SelectedIndex - 3];
            _loading = true;
            box.SelectedIndex = 1;
            ConfigureCurveParam(box, param);
            param.Value = exponent;
            _loading = false;
            SetDirty(true);
            return;
        }
        ConfigureCurveParam(box, param);
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

    // ================= import / export / backup / hub =================

    private async System.Threading.Tasks.Task<ContentDialogResult> Ask(string title, string text, string primary, string? secondary = null, string close = "Cancel")
    {
        var d = new ContentDialog
        {
            XamlRoot = XamlRoot, Title = title,
            Content = new ScrollViewer { MaxHeight = 320, Content = new TextBlock { Text = text, TextWrapping = TextWrapping.Wrap } },
            PrimaryButtonText = primary, SecondaryButtonText = secondary ?? "", CloseButtonText = close,
            DefaultButton = ContentDialogButton.Primary,
        };
        return await d.ShowAsync();
    }

    /// A profile from outside may carry "open an app" actions. Show exactly
    /// what they'd run and let the user drop them (the default). False = cancelled.
    private async System.Threading.Tasks.Task<bool> ConfirmLaunches(System.Collections.Generic.IReadOnlyList<string> targets, Action strip)
    {
        if (targets.Count == 0) return true;
        var r = await Ask("This profile can open programs",
            "Some of its buttons are set to open these:\n\n  " + string.Join("\n  ", targets) +
            "\n\nOnly keep them if you trust where the profile came from: pressing the button runs them on your PC.",
            "Import without them", "Keep them");
        if (r == ContentDialogResult.None) return false;
        if (r == ContentDialogResult.Primary) strip();
        return true;
    }

    private async void Import_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            var path = FileDialogs.Open("Import a DXO profile", ("DXO profile", "*.toml"), ("All files", "*.*"));
            if (path is null) return;
            if (ProfileTransfer.Read(path) is not ProfileModel m) { StatusText.Text = "That file isn't a DXO profile."; return; }
            if (!await ConfirmLaunches(ProfileTransfer.LaunchTargets(m), () => ProfileTransfer.StripLaunches(m))) return;
            string? name = ProfileTransfer.Add(m);
            if (name is null) { StatusText.Text = "Couldn't import that profile."; return; }
            _dirty = false;
            LoadList(name);
            StatusText.Text = $"Imported as \"{name}\".";
        }
        catch (Exception ex) { App.LogCrash("Profiles.Import", ex); StatusText.Text = $"Import failed: {ex.Message}"; }
    }

    private void Export_Click(object sender, RoutedEventArgs e)
    {
        if (_current is null) return;
        if (_dirty) { StatusText.Text = "Save your changes first, then export."; return; }
        try
        {
            var dest = FileDialogs.Save("Export profile", IOPath.GetFileName(_current.Path), "toml", ("DXO profile", "*.toml"));
            if (dest is null) return;
            File.Copy(_current.Path, dest, overwrite: true);
            StatusText.Text = $"Exported to {dest}";
        }
        catch (Exception ex) { App.LogCrash("Profiles.Export", ex); StatusText.Text = $"Export failed: {ex.Message}"; }
    }

    private void Backup_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            var dest = FileDialogs.Save("Back up DXO profiles and settings", $"dxo-backup-{DateTime.Now:yyyy-MM-dd}.zip", "zip", ("Zip file", "*.zip"));
            if (dest is null) return;
            int n = ProfileTransfer.Backup(dest);
            StatusText.Text = $"Backed up {n} profile{(n == 1 ? "" : "s")} and your controller settings to {dest}";
        }
        catch (Exception ex) { App.LogCrash("Profiles.Backup", ex); StatusText.Text = $"Backup failed: {ex.Message}"; }
    }

    private async void Restore_Click(object sender, RoutedEventArgs e)
    {
        try
        {
            var path = FileDialogs.Open("Restore a DXO backup", ("DXO backup", "*.zip"));
            if (path is null) return;
            var backup = ProfileTransfer.OpenBackup(path);
            if (backup is null || (backup.Profiles.Count == 0 && !backup.HasControllers)) { StatusText.Text = "That zip isn't a DXO backup."; return; }

            string names = string.Join(", ", backup.Profiles.Select(p => p.Model.Name));
            var r = await Ask("Restore this backup?",
                $"{backup.Profiles.Count} profile{(backup.Profiles.Count == 1 ? "" : "s")}: {names}\n\n" +
                "A profile with the same file name as one you have now replaces it." +
                (backup.HasControllers ? "\n\nThe backup also holds controller settings (names, emulation, per-game rules). Restoring those replaces the current ones and needs DXO to be restarted." : ""),
                "Restore profiles", backup.HasControllers ? "Profiles and controller settings" : null);
            if (r == ContentDialogResult.None) return;
            bool controllers = r == ContentDialogResult.Secondary;

            bool strip = false;
            var targets = backup.Profiles.SelectMany(p => ProfileTransfer.LaunchTargets(p.Model)).Distinct().ToList();
            if (!await ConfirmLaunches(targets, () => strip = true)) return;

            int n = ProfileTransfer.Restore(backup, strip, controllers);
            _dirty = false;
            LoadList(null);
            StatusText.Text = $"Restored {n} profile{(n == 1 ? "" : "s")}.";
            if (controllers &&
                await Ask("Restart DXO", "Controller settings were restored. DXO reads them when it starts: close it now and open it again.", "Close DXO", null, "Later") == ContentDialogResult.Primary)
                (((App)Application.Current).MainWindowRef as MainWindow)?.ExitApp();
        }
        catch (Exception ex) { App.LogCrash("Profiles.Restore", ex); StatusText.Text = $"Restore failed: {ex.Message}"; }
    }

    /// Community profiles have their own page now.
    private void Hub_Click(object sender, RoutedEventArgs e) => MainWindow.NavigateTo("community");
}

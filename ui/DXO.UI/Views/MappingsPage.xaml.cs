using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.IO;
using System.Linq;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Navigation;
using DXO.UI.Controls;
using DXO.UI.Native;
using DXO.UI.ViewModels;
using IOPath = System.IO.Path;

namespace DXO.UI.Views;

/// One row of the button list: "Cross / A    Space key".
public sealed class MappingRow : INotifyPropertyChanged
{
    public string Key { get; init; } = "";
    public string Label { get; init; } = "";
    private string _summary = "";
    public string Summary
    {
        get => _summary;
        set { if (_summary != value) { _summary = value; PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(Summary))); } }
    }
    public event PropertyChangedEventHandler? PropertyChanged;
}

/// Per-button bindings for the ACTIVE profile, applied live; Save writes the
/// profile file (same model as the Motion page).
public sealed partial class MappingsPage : Page
{
    private static readonly string[] Types = { "same", "button", "key", "mouse", "disabled" };

    private readonly ObservableCollection<MappingRow> _rows = new();
    private readonly DispatcherTimer _timer = new() { Interval = TimeSpan.FromMilliseconds(500) };
    private ProfileModel? _model;
    private MappingRow? _selected;
    private bool _loading;
    private bool _unsaved;

    public MappingsPage()
    {
        _loading = true;
        InitializeComponent();
        _loading = false;
        foreach (var b in ButtonCatalog.Buttons) _rows.Add(new MappingRow { Key = b.Key, Label = b.Label });
        ButtonList.ItemsSource = _rows;
        Visual.ZoneClicked += key => ButtonList.SelectedItem = _rows.FirstOrDefault(r => r.Key == key);
        _timer.Tick += (_, _) => Tick();
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        Visual.LoadSkin(ControllerVisualizer.SkinFor(App.Device.Kind));
        LoadModel();
        App.Device.StateUpdated += OnState;
        _timer.Start();
    }

    protected override void OnNavigatedFrom(NavigationEventArgs e)
    {
        App.Device.StateUpdated -= OnState;
        _timer.Stop();
    }

    /// Picture follows the connected pad and lights what's pressed (handy to find a button).
    private void OnState(DxoState s)
    {
        Visual.LoadSkin(ControllerVisualizer.SkinFor(s.Kind));
        Visual.Update(s.Buttons, s.LX, s.LY, s.RX, s.RY);
    }

    // ================= model =================

    private void LoadModel()
    {
        try { _model = ProfileModel.FromJson(App.Engine?.ActiveProfileJson ?? ""); }
        catch (Exception ex) { App.LogCrash("MappingsPage.LoadModel", ex); _model = null; }

        if (_model is null)
        {
            Editor.Visibility = Visibility.Collapsed;
            SaveButton.IsEnabled = false;
            SaveStatus.Text = "The engine isn't running.";
            return;
        }
        // Older profiles keep simple swaps in `remap`; show them as bindings.
        // They're written back as bindings on the next change.
        foreach (var pair in _model.Remap.Where(p => p.Count == 2))
            if (!_model.Bindings.Any(b => b.Button == pair[0]))
                _model.Bindings.Add(new BindingModel { Button = pair[0], Action = new ActionModel { Type = "button", To = pair[1] } });
        _model.Remap.Clear();

        SubTitle.Text = $"Editing \"{_model.Name}\" (the active profile). Click a button on the controller or in the list, then choose what it does.";
        SaveStatus.Text = "";
        _unsaved = false;
        RefreshSummaries();
        ButtonList.SelectedItem ??= _rows[0];
        ShowEditor();
    }

    private BindingModel? Find(string key) => _model?.Bindings.FirstOrDefault(b => b.Button == key);

    private void RefreshSummaries()
    {
        foreach (var r in _rows)
            r.Summary = Find(r.Key) is BindingModel b ? ButtonCatalog.Describe(b) ?? "" : "";
    }

    private void Push()
    {
        if (_model is null || App.Engine is null) return;
        if (App.Engine.SetActiveProfileJson(_model.ToJson()))
        {
            _unsaved = true;
            SaveStatus.Text = $"Applied. Save to keep it in \"{_model.Name}\".";
        }
        else SaveStatus.Text = "The engine rejected that mapping.";
        RefreshSummaries();
    }

    // ================= editor =================

    private void ButtonList_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        _selected = ButtonList.SelectedItem as MappingRow;
        Visual.SetSelected(_selected?.Key);
        if (_selected is not null) ButtonList.ScrollIntoView(_selected);
        ShowEditor();
    }

    private void ShowEditor()
    {
        if (_model is null || _selected is null) { Editor.Visibility = Visibility.Collapsed; return; }
        Editor.Visibility = Visibility.Visible;
        var b = Find(_selected.Key) ?? new BindingModel { Button = _selected.Key };

        _loading = true;
        EditTitle.Text = _selected.Label;
        TypeBox.SelectedIndex = Math.Max(0, Array.IndexOf(Types, b.Action.Type));
        FillTargets(b);
        TurboToggle.IsOn = b.Turbo;
        TurboSlider.Value = Math.Clamp(b.TurboHz, 2, 30);
        ToggleToggle.IsOn = b.Toggle;
        _loading = false;
        UpdateVisibility();
    }

    /// Fill "Which one" for the chosen type and select the binding's current target.
    private void FillTargets(BindingModel b)
    {
        switch (Types[Math.Max(0, TypeBox.SelectedIndex)])
        {
            case "button":
                var others = ButtonCatalog.Buttons.Where(x => x.Key != _selected!.Key).ToList();
                TargetBox.ItemsSource = others;
                TargetBox.SelectedItem = others.FirstOrDefault(x => x.Key == b.Action.To) ?? others[0];
                break;
            case "key":
                TargetBox.ItemsSource = ButtonCatalog.Keys;
                TargetBox.SelectedItem = ButtonCatalog.Keys.FirstOrDefault(x => x.Vk == b.Action.Vk) ?? ButtonCatalog.Keys[0];
                break;
            case "mouse":
                TargetBox.ItemsSource = ButtonCatalog.Mouse;
                TargetBox.SelectedItem = ButtonCatalog.Mouse.FirstOrDefault(x => x.Key == b.Action.Button) ?? ButtonCatalog.Mouse[0];
                break;
            default:
                TargetBox.ItemsSource = null;
                break;
        }
    }

    private void UpdateVisibility()
    {
        string type = Types[Math.Max(0, TypeBox.SelectedIndex)];
        TargetCard.Visibility = type is "button" or "key" or "mouse" ? Visibility.Visible : Visibility.Collapsed;
        TargetCard.Header = type switch { "key" => "Key", "mouse" => "Mouse action", _ => "Button" };
        TurboRateCard.Visibility = TurboToggle.IsOn ? Visibility.Visible : Visibility.Collapsed;
        TurboText.Text = $"{TurboSlider.Value:0}/s";
        Hint.Text = type switch
        {
            "key" => "The game no longer sees this button; it sees the key instead. Works in games and on the desktop.",
            "mouse" => "Scroll actions fire once per press (or repeatedly with turbo).",
            "disabled" => "The button does nothing at all, handy for a PS or touchpad button you keep bumping.",
            "button" => "Two buttons can swap: map Cross to Circle and Circle to Cross.",
            _ => "",
        };
    }

    /// Controls -> binding for the selected button, then apply live.
    private void Commit()
    {
        if (_loading || _model is null || _selected is null) return;
        string type = Types[Math.Max(0, TypeBox.SelectedIndex)];
        var action = new ActionModel { Type = type };
        switch (type)
        {
            case "button": action.To = (TargetBox.SelectedItem as KeyLabel)?.Key; break;
            case "key": action.Vk = (TargetBox.SelectedItem as VkLabel)?.Vk; break;
            case "mouse": action.Button = (TargetBox.SelectedItem as KeyLabel)?.Key; break;
        }
        var b = new BindingModel
        {
            Button = _selected.Key,
            Action = action,
            Turbo = TurboToggle.IsOn,
            TurboHz = (float)Math.Round(TurboSlider.Value),
            Toggle = ToggleToggle.IsOn,
        };
        _model.Bindings.RemoveAll(x => x.Button == _selected.Key);
        bool incomplete = (type == "button" && action.To is null) || (type == "key" && action.Vk is null) || (type == "mouse" && action.Button is null);
        if (ButtonCatalog.Describe(b) is not null && !incomplete) _model.Bindings.Add(b); // default = no entry
        UpdateVisibility();
        Push();
    }

    private void TypeBox_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_loading || _selected is null) return;
        _loading = true;
        FillTargets(Find(_selected.Key) ?? new BindingModel { Button = _selected.Key });
        _loading = false;
        Commit();
    }

    private void Changed(object sender, SelectionChangedEventArgs e) => Commit();
    private void Toggle_Changed(object sender, RoutedEventArgs e) => Commit();
    private void Slider_Changed(object sender, RangeBaseValueChangedEventArgs e) => Commit();

    private void Reset_Click(object sender, RoutedEventArgs e)
    {
        if (_model is null || _selected is null) return;
        _model.Bindings.RemoveAll(x => x.Button == _selected.Key);
        Push();
        ShowEditor();
    }

    private async void ResetAll_Click(object sender, RoutedEventArgs e)
    {
        if (_model is null) return;
        var d = new ContentDialog
        {
            XamlRoot = XamlRoot, Title = "Reset all mappings?",
            Content = $"Every button in \"{_model.Name}\" goes back to doing what it says.",
            PrimaryButtonText = "Reset all", CloseButtonText = "Cancel", DefaultButton = ContentDialogButton.Close,
        };
        if (await d.ShowAsync() != ContentDialogResult.Primary) return;
        _model.Bindings.Clear();
        Push();
        ShowEditor();
    }

    // ================= save =================

    private void Save_Click(object sender, RoutedEventArgs e)
    {
        if (_model is null) return;
        var path = PathForProfile(_model.Name);
        if (DxoEngine.ProfileJsonToFile(_model.ToJson(), path))
        {
            App.Engine?.ReloadProfiles();
            _unsaved = false;
            SaveStatus.Text = $"Saved to \"{_model.Name}\".";
        }
        else SaveStatus.Text = "Couldn't save the profile.";
    }

    private static string PathForProfile(string name)
    {
        Directory.CreateDirectory(App.ProfilesDir);
        foreach (var f in Directory.GetFiles(App.ProfilesDir, "*.toml"))
        {
            try { if (ProfileModel.FromJson(DxoEngine.ProfileFileToJson(f))?.Name == name) return f; }
            catch { /* unreadable: skip */ }
        }
        var safe = new string(name.Select(ch => IOPath.GetInvalidFileNameChars().Contains(ch) ? '_' : ch).ToArray()).Trim();
        return IOPath.Combine(App.ProfilesDir, (safe.Length == 0 ? "profile" : safe) + ".toml");
    }

    // ================= live =================

    private void Tick()
    {
        // A game profile switched in: follow it unless there are unsaved edits.
        if (!_unsaved)
        {
            string? name = null;
            try { name = ProfileModel.FromJson(App.Engine?.ActiveProfileJson ?? "")?.Name; } catch { }
            if (name is not null && _model is not null && name != _model.Name) LoadModel();
        }
    }
}

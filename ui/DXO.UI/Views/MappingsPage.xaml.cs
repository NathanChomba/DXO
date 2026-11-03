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
    private static readonly string[] Types = { "same", "button", "key", "mouse", "disabled", "macro" };
    private static readonly PressChoice NoPress = new("(nothing else)", new PressModel { Type = "" });
    private static readonly PressChoice[] SecondPresses = new[] { NoPress }.Concat(ButtonCatalog.Presses).ToArray();
    private static readonly KeyLabel[] ShiftChoices = new[] { new KeyLabel("", "Off") }.Concat(ButtonCatalog.Buttons).ToArray();

    private readonly ObservableCollection<MappingRow> _rows = new();
    private readonly DispatcherTimer _timer = new() { Interval = TimeSpan.FromMilliseconds(500) };
    private ProfileModel? _model;
    private MappingRow? _selected;
    private bool _loading;
    private bool _unsaved;
    /// Editing the shift layer instead of the normal bindings.
    private bool _shift;
    /// Working copy of the selected button's macro steps.
    private List<MacroStepModel> _steps = new();

    private List<BindingModel> Layer => _shift ? _model!.ShiftBindings : _model!.Bindings;

    public MappingsPage()
    {
        _loading = true;
        InitializeComponent();
        _loading = false;
        foreach (var b in ButtonCatalog.Buttons) _rows.Add(new MappingRow { Key = b.Key, Label = b.Label });
        ButtonList.ItemsSource = _rows;
        Visual.ZoneClicked += key => ButtonList.SelectedItem = _rows.FirstOrDefault(r => r.Key == key);
        _timer.Tick += (_, _) => Tick();
        ShiftBox.ItemsSource = ShiftChoices;
    }

    protected override void OnNavigatedTo(NavigationEventArgs e)
    {
        if (e.Parameter is "embedded") Header.Visibility = Visibility.Collapsed;
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
        _loading = true;
        ShiftBox.SelectedItem = ShiftChoices.FirstOrDefault(c => c.Key == _model.ShiftButton) ?? ShiftChoices[0];
        _loading = false;
        UpdateLayerHint();
        SaveStatus.Text = "";
        _unsaved = false;
        RefreshSummaries();
        ButtonList.SelectedItem ??= _rows[0];
        ShowEditor();
    }

    private BindingModel? Find(string key) => _model is null ? null : Layer.FirstOrDefault(b => b.Button == key);

    private void RefreshSummaries()
    {
        foreach (var r in _rows)
        {
            if (_shift && _model?.ShiftButton == r.Key) { r.Summary = "Shift button"; continue; }
            r.Summary = Find(r.Key) is BindingModel b ? ButtonCatalog.Describe(b) ?? "" : _shift ? "as normal" : "";
        }
    }

    // ================= shift layer =================

    private void LayerBar_SelectionChanged(SelectorBar sender, SelectorBarSelectionChangedEventArgs args)
    {
        if (_loading) return; // fires inside InitializeComponent
        _shift = sender.SelectedItem == ShiftItem;
        UpdateLayerHint();
        RefreshSummaries();
        ShowEditor();
    }

    private void ShiftBox_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_loading || _model is null) return;
        _model.ShiftButton = (ShiftBox.SelectedItem as KeyLabel)?.Key ?? "";
        UpdateLayerHint();
        Push();
    }

    private void UpdateLayerHint()
    {
        string sb = _model?.ShiftButton ?? "";
        LayerHint.Text = sb.Length == 0
            ? "Pick a button to hold for a second set of mappings."
            : _shift
                ? $"Used while you hold {ButtonCatalog.ButtonLabel(sb)}. Buttons you leave alone keep their normal mapping."
                : $"Hold {ButtonCatalog.ButtonLabel(sb)} for the shift layer. A quick tap still reaches the game.";
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
        RepeatToggle.IsOn = b.Action.Repeat == true;
        _steps = (b.Action.Steps ?? new()).Select(CloneStep).ToList();
        RebuildSteps();
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
        bool macro = type == "macro";
        MacroPanel.Visibility = macro ? Visibility.Visible : Visibility.Collapsed;
        TurboCard.Visibility = ToggleCard.Visibility = macro ? Visibility.Collapsed : Visibility.Visible;
        TurboRateCard.Visibility = TurboToggle.IsOn && !macro ? Visibility.Visible : Visibility.Collapsed;
        TurboText.Text = $"{TurboSlider.Value:0}/s";
        Hint.Text = type switch
        {
            "key" => "The game no longer sees this button; it sees the key instead. Works in games and on the desktop.",
            "mouse" => "Scroll actions fire once per press (or repeatedly with turbo).",
            "disabled" => "The button does nothing at all, handy for a PS or touchpad button you keep bumping.",
            "button" => "Two buttons can swap: map Cross to Circle and Circle to Cross.",
            "macro" => "Starts when you press the button and plays to the end. Give a step a second press to press both together, like Ctrl + C.",
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
            case "macro":
                action.Steps = _steps.Select(CloneStep).ToList();
                action.Repeat = RepeatToggle.IsOn;
                break;
        }
        bool macro = type == "macro";
        var b = new BindingModel
        {
            Button = _selected.Key,
            Action = action,
            Turbo = TurboToggle.IsOn && !macro,
            TurboHz = (float)Math.Round(TurboSlider.Value),
            Toggle = ToggleToggle.IsOn && !macro,
        };
        Layer.RemoveAll(x => x.Button == _selected.Key);
        bool incomplete = (type == "button" && action.To is null) || (type == "key" && action.Vk is null) || (type == "mouse" && action.Button is null);
        if (ButtonCatalog.Describe(b) is not null && !incomplete) Layer.Add(b); // default = no entry
        UpdateVisibility();
        Push();
    }

    private void TypeBox_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_loading || _selected is null) return;
        _loading = true;
        FillTargets(Find(_selected.Key) ?? new BindingModel { Button = _selected.Key });
        if (Types[Math.Max(0, TypeBox.SelectedIndex)] == "macro" && _steps.Count == 0)
        {
            _steps.Add(NewStep());
            RebuildSteps();
        }
        _loading = false;
        Commit();
    }

    // ================= macro steps =================

    private static MacroStepModel CloneStep(MacroStepModel s) => new()
    {
        HoldMs = s.HoldMs,
        GapMs = s.GapMs,
        Presses = s.Presses.Select(p => new PressModel { Type = p.Type, To = p.To, Vk = p.Vk, Button = p.Button }).ToList(),
    };

    private static MacroStepModel NewStep() =>
        new() { Presses = { new PressModel { Type = "key", Vk = 0x20 } }, HoldMs = 50, GapMs = 50 };

    private void AddStep_Click(object sender, RoutedEventArgs e)
    {
        _steps.Add(NewStep());
        RebuildSteps();
        Commit();
    }

    /// One small card per step: what it presses (one or two things), how long, then the pause.
    private void RebuildSteps()
    {
        MacroSteps.Children.Clear();
        for (int i = 0; i < _steps.Count; i++)
        {
            int n = i;
            var st = _steps[n];
            var grid = new Grid { RowSpacing = 6, ColumnSpacing = 8 };
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            for (int r = 0; r < 4; r++) grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

            var title = new TextBlock { Text = $"Step {n + 1}", FontWeight = Microsoft.UI.Text.FontWeights.SemiBold, VerticalAlignment = VerticalAlignment.Center };
            var del = new Button { Content = new FontIcon { Glyph = "\uE74D", FontSize = 12 }, HorizontalAlignment = HorizontalAlignment.Right, Padding = new Thickness(8, 4, 8, 4) };
            ToolTipService.SetToolTip(del, "Remove step");
            del.Click += (_, _) => { _steps.RemoveAt(n); RebuildSteps(); Commit(); };
            Grid.SetColumn(del, 1);

            var first = new ComboBox { ItemsSource = ButtonCatalog.Presses, DisplayMemberPath = "Label", HorizontalAlignment = HorizontalAlignment.Stretch };
            first.SelectedItem = st.Presses.Count > 0 ? ButtonCatalog.Find(st.Presses[0]) : null;
            var second = new ComboBox { ItemsSource = SecondPresses, DisplayMemberPath = "Label", HorizontalAlignment = HorizontalAlignment.Stretch };
            second.SelectedItem = st.Presses.Count > 1 ? ButtonCatalog.Find(st.Presses[1]) ?? NoPress : NoPress;
            void presses(object? _, SelectionChangedEventArgs __)
            {
                if (_loading) return;
                var list = new List<PressModel>();
                if (first.SelectedItem is PressChoice a) list.Add(a.Press);
                if (second.SelectedItem is PressChoice b && b != NoPress) list.Add(b.Press);
                _steps[n].Presses = list;
                Commit();
            }
            first.SelectionChanged += presses;
            second.SelectionChanged += presses;
            Grid.SetRow(first, 1); Grid.SetColumnSpan(first, 2);
            Grid.SetRow(second, 2); Grid.SetColumnSpan(second, 2);

            NumberBox ms(string header, uint value, Action<uint> set)
            {
                var nb = new NumberBox { Header = header, Value = value, Minimum = 0, Maximum = 10000, SmallChange = 10, LargeChange = 100, SpinButtonPlacementMode = NumberBoxSpinButtonPlacementMode.Compact };
                nb.ValueChanged += (s, _) =>
                {
                    if (_loading || double.IsNaN(s.Value)) return;
                    set((uint)Math.Clamp(s.Value, 0, 10000));
                    Commit();
                };
                return nb;
            }
            var hold = ms("Hold (ms)", st.HoldMs, v => _steps[n].HoldMs = v);
            var gap = ms("Then wait (ms)", st.GapMs, v => _steps[n].GapMs = v);
            Grid.SetRow(hold, 3);
            Grid.SetRow(gap, 3); Grid.SetColumn(gap, 1);

            foreach (var c in new UIElement[] { title, del, first, second, hold, gap }) grid.Children.Add(c);
            MacroSteps.Children.Add(new Border { Style = (Style)Application.Current.Resources["DxoCard"], Padding = new Thickness(12), Child = grid });
        }
    }

    private void Changed(object sender, SelectionChangedEventArgs e) => Commit();
    private void Toggle_Changed(object sender, RoutedEventArgs e) => Commit();
    private void Slider_Changed(object sender, RangeBaseValueChangedEventArgs e) => Commit();

    private void Reset_Click(object sender, RoutedEventArgs e)
    {
        if (_model is null || _selected is null) return;
        Layer.RemoveAll(x => x.Button == _selected.Key);
        Push();
        ShowEditor();
    }

    private async void ResetAll_Click(object sender, RoutedEventArgs e)
    {
        if (_model is null) return;
        var d = new ContentDialog
        {
            XamlRoot = XamlRoot, Title = "Reset all mappings?",
            Content = _shift ? $"The shift layer in \"{_model.Name}\" is emptied; every button keeps its normal mapping."
                             : $"Every button in \"{_model.Name}\" goes back to doing what it says.",
            PrimaryButtonText = "Reset all", CloseButtonText = "Cancel", DefaultButton = ContentDialogButton.Close,
        };
        if (await d.ShowAsync() != ContentDialogResult.Primary) return;
        Layer.Clear();
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

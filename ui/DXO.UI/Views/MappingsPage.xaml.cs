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

/// One row of the list: "Cross / A    Space key", or a combo "L1 + R1    F1 key".
public sealed class MappingRow : INotifyPropertyChanged
{
    public string Key { get; init; } = "";
    private string _label = "";
    public string Label
    {
        get => _label;
        set { if (_label != value) { _label = value; PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(Label))); } }
    }
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
///
/// Three lists (the bar at the top): the normal mappings, the shift layer, and
/// combos (two buttons together). A button's mapping has up to three actions,
/// chosen with the Press / Hold / Double-tap bar above the editor.
public sealed partial class MappingsPage : Page
{
    private static readonly string[] Types = { "same", "button", "key", "mouse", "disabled", "macro", "launch" };
    private static readonly PressChoice NoPress = new("(nothing else)", new PressModel { Type = "" });
    private static readonly PressChoice[] SecondPresses = new[] { NoPress }.Concat(ButtonCatalog.Presses).ToArray();
    private static readonly KeyLabel[] ShiftChoices = new[] { new KeyLabel("", "Off") }.Concat(ButtonCatalog.Buttons).ToArray();

    private enum Mode { Normal, Shift, Combos }

    private readonly ObservableCollection<MappingRow> _rows = new();
    private readonly DispatcherTimer _timer = new() { Interval = TimeSpan.FromMilliseconds(500) };
    private ProfileModel? _model;
    private MappingRow? _selected;
    private bool _loading;
    private bool _unsaved;
    private Mode _mode = Mode.Normal;
    /// Which of the button's actions is being edited: 0 press, 1 hold, 2 double-tap.
    private int _slot;
    /// Working copy of the macro steps being edited.
    private List<MacroStepModel> _steps = new();

    private bool Combos => _mode == Mode.Combos;
    private List<BindingModel> Layer => _mode == Mode.Shift ? _model!.ShiftBindings : _model!.Bindings;

    public MappingsPage()
    {
        _loading = true;
        InitializeComponent();
        _loading = false;
        ButtonList.ItemsSource = _rows;
        Visual.ZoneClicked += key =>
        {
            if (!Combos) ButtonList.SelectedItem = _rows.FirstOrDefault(r => r.Key == key);
        };
        _timer.Tick += (_, _) => Tick();
        ShiftBox.ItemsSource = ShiftChoices;
        ComboA.ItemsSource = ButtonCatalog.Sources;
        ComboB.ItemsSource = ButtonCatalog.Sources;
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
        HoldMsSlider.Value = Math.Clamp(_model.HoldMs, 150, 800);
        DoubleMsSlider.Value = Math.Clamp(_model.DoubleTapMs, 150, 500);
        SoftSlider.Value = Math.Clamp(Math.Round(_model.TriggerSoftPoint * 100), 5, 60);
        FullSlider.Value = Math.Clamp(Math.Round(_model.TriggerFullPoint * 100), 50, 100);
        _loading = false;
        UpdateTimingLabels();
        UpdateLayerHint();
        SaveStatus.Text = "";
        _unsaved = false;
        RebuildRows(_selected?.Key);
    }

    private BindingModel? Find(string key) => _model is null ? null : Layer.FirstOrDefault(b => b.Button == key);

    /// The combo behind the selected row ("combo:2"), if any.
    private ChordModel? SelectedChord()
    {
        if (_model is null || _selected is null || !_selected.Key.StartsWith("combo:")) return null;
        return int.TryParse(_selected.Key[6..], out int i) && i >= 0 && i < _model.Chords.Count ? _model.Chords[i] : null;
    }

    /// Fill the list for the current mode and select `key` (or the first row).
    private void RebuildRows(string? key)
    {
        if (_model is null) return;
        _loading = true;
        _rows.Clear();
        if (Combos)
            for (int i = 0; i < _model.Chords.Count; i++) _rows.Add(new MappingRow { Key = $"combo:{i}" });
        else
            foreach (var b in ButtonCatalog.Sources) _rows.Add(new MappingRow { Key = b.Key, Label = b.Label });
        RefreshSummaries();
        _selected = _rows.FirstOrDefault(r => r.Key == key) ?? _rows.FirstOrDefault();
        ButtonList.SelectedItem = _selected;
        _loading = false;
        ComboBar.Visibility = Combos ? Visibility.Visible : Visibility.Collapsed;
        RemoveComboButton.IsEnabled = _selected is not null;
        ShiftPicker.Visibility = Combos ? Visibility.Collapsed : Visibility.Visible;
        Visual.SetSelected(VisualKey(_selected?.Key));
        if (_selected is not null) ButtonList.ScrollIntoView(_selected);
        ShowEditor();
    }

    /// The button to highlight on the picture: a trigger stage lights its trigger.
    private string? VisualKey(string? key) => key switch
    {
        null => null,
        "l2_soft" or "l2_full" => "l2",
        "r2_soft" or "r2_full" => "r2",
        _ => Combos ? null : key,
    };

    private void RefreshSummaries()
    {
        if (_model is null) return;
        foreach (var r in _rows)
        {
            if (Combos)
            {
                if (!r.Key.StartsWith("combo:") || !int.TryParse(r.Key[6..], out int i) || i >= _model.Chords.Count) continue;
                r.Label = ButtonCatalog.ChordLabel(_model.Chords[i]);
                r.Summary = ButtonCatalog.DescribeAction(_model.Chords[i].Action) ?? "";
                continue;
            }
            bool shift = _mode == Mode.Shift;
            if (shift && _model.ShiftButton == r.Key) { r.Summary = "Shift button"; continue; }
            r.Summary = Find(r.Key) is BindingModel b ? ButtonCatalog.Describe(b) ?? "" : shift ? "as normal" : "";
        }
    }

    // ================= layers =================

    private void LayerBar_SelectionChanged(SelectorBar sender, SelectorBarSelectionChangedEventArgs args)
    {
        if (_loading) return; // fires inside InitializeComponent
        _mode = sender.SelectedItem == ShiftItem ? Mode.Shift : sender.SelectedItem == CombosItem ? Mode.Combos : Mode.Normal;
        SetSlot(0);
        UpdateLayerHint();
        RebuildRows(Combos ? null : _selected?.Key);
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
        if (Combos)
        {
            LayerHint.Text = "Two buttons held together do one extra thing, and the game sees neither while they're down. Press them at the same moment.";
            return;
        }
        string sb = _model?.ShiftButton ?? "";
        LayerHint.Text = sb.Length == 0
            ? "Pick a button to hold for a second set of mappings."
            : _mode == Mode.Shift
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

    // ================= combos =================

    private void AddCombo_Click(object sender, RoutedEventArgs e)
    {
        if (_model is null) return;
        _model.Chords.Add(new ChordModel
        {
            Buttons = new List<string> { "l1", "r1" },
            Action = new ActionModel { Type = "key", Vk = 0x20 },
        });
        Push();
        RebuildRows($"combo:{_model.Chords.Count - 1}");
    }

    private void RemoveCombo_Click(object sender, RoutedEventArgs e)
    {
        if (_model is null || SelectedChord() is not ChordModel c) return;
        _model.Chords.Remove(c);
        Push();
        RebuildRows(null);
    }

    // ================= editor =================

    private void ButtonList_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_loading) return;
        _selected = ButtonList.SelectedItem as MappingRow;
        Visual.SetSelected(VisualKey(_selected?.Key));
        RemoveComboButton.IsEnabled = _selected is not null;
        if (_selected is not null) ButtonList.ScrollIntoView(_selected);
        ShowEditor();
    }

    /// Press / Hold / Double-tap.
    private void PressBar_SelectionChanged(SelectorBar sender, SelectorBarSelectionChangedEventArgs args)
    {
        if (_loading) return;
        _slot = sender.SelectedItem == HoldItem ? 1 : sender.SelectedItem == DoubleItem ? 2 : 0;
        ShowEditor();
    }

    private void SetSlot(int slot)
    {
        _slot = slot;
        bool was = _loading;
        _loading = true;
        PressBar.SelectedItem = slot switch { 1 => HoldItem, 2 => DoubleItem, _ => PressItem };
        _loading = was;
    }

    /// The action the editor is showing right now; null = that slot isn't used.
    private ActionModel? CurrentAction()
    {
        if (Combos) return SelectedChord()?.Action;
        var b = _selected is null ? null : Find(_selected.Key);
        return _slot switch { 1 => b?.Hold, 2 => b?.DoubleTap, _ => b?.Action ?? new ActionModel() };
    }

    private void ShowEditor()
    {
        if (_model is null || _selected is null || (Combos && SelectedChord() is null))
        {
            Editor.Visibility = Visibility.Collapsed;
            return;
        }
        Editor.Visibility = Visibility.Visible;
        var binding = Combos ? null : Find(_selected.Key) ?? new BindingModel { Button = _selected.Key };
        var action = CurrentAction();

        _loading = true;
        EditTitle.Text = Combos ? "Combo" : _selected.Label;
        if (SelectedChord() is ChordModel chord)
        {
            ComboA.SelectedItem = ButtonCatalog.Sources.FirstOrDefault(x => x.Key == chord.Buttons.ElementAtOrDefault(0));
            ComboB.SelectedItem = ButtonCatalog.Sources.FirstOrDefault(x => x.Key == chord.Buttons.ElementAtOrDefault(1));
        }
        SlotToggle.IsOn = action is not null;
        var shown = action ?? new ActionModel();
        TypeBox.SelectedIndex = Math.Max(0, Array.IndexOf(Types, shown.Type));
        FillTargets(shown);
        LaunchTarget.Text = shown.Target ?? "";
        LaunchArgs.Text = shown.Args ?? "";
        RepeatToggle.IsOn = shown.Repeat == true;
        _steps = (shown.Steps ?? new()).Select(CloneStep).ToList();
        RebuildSteps();
        TurboToggle.IsOn = binding?.Turbo ?? false;
        TurboSlider.Value = Math.Clamp(binding?.TurboHz ?? 10, 2, 30);
        ToggleToggle.IsOn = binding?.Toggle ?? false;
        _loading = false;
        UpdateVisibility();
    }

    /// Fill "Which one" for the chosen type and select the action's current target.
    private void FillTargets(ActionModel a)
    {
        switch (Types[Math.Max(0, TypeBox.SelectedIndex)])
        {
            case "button":
                var others = ButtonCatalog.Buttons.Where(x => Combos || x.Key != _selected!.Key).ToList();
                TargetBox.ItemsSource = others;
                TargetBox.SelectedItem = others.FirstOrDefault(x => x.Key == a.To) ?? others[0];
                break;
            case "key":
                TargetBox.ItemsSource = ButtonCatalog.Keys;
                TargetBox.SelectedItem = ButtonCatalog.Keys.FirstOrDefault(x => x.Vk == a.Vk) ?? ButtonCatalog.Keys[0];
                break;
            case "mouse":
                TargetBox.ItemsSource = ButtonCatalog.Mouse;
                TargetBox.SelectedItem = ButtonCatalog.Mouse.FirstOrDefault(x => x.Key == a.Button) ?? ButtonCatalog.Mouse[0];
                break;
            default:
                TargetBox.ItemsSource = null;
                break;
        }
    }

    private static Visibility Show(bool on) => on ? Visibility.Visible : Visibility.Collapsed;

    private void UpdateVisibility()
    {
        if (_model is null || _selected is null) return;
        string type = Types[Math.Max(0, TypeBox.SelectedIndex)];
        var binding = Combos ? null : Find(_selected.Key);
        bool timed = binding is { Hold: not null } || binding is { DoubleTap: not null };
        bool stage = !Combos && ButtonCatalog.IsStage(_selected.Key);
        // Hold / Double-tap are optional: their switch decides whether there's anything to edit.
        bool editing = Combos || _slot == 0 || SlotToggle.IsOn;

        ComboPanel.Visibility = Show(Combos);
        PressBar.Visibility = Show(!Combos);
        SlotCard.Visibility = Show(!Combos && _slot > 0);
        SlotCard.Header = _slot == 2 ? "Do something else on a double-tap" : "Do something else when held";
        TypeCard.Visibility = Show(editing);
        TargetCard.Visibility = Show(editing && type is "button" or "key" or "mouse");
        TargetCard.Header = type switch { "key" => "Key", "mouse" => "Mouse action", _ => "Button" };
        LaunchPanel.Visibility = Show(editing && type == "launch");
        MacroPanel.Visibility = Show(editing && type == "macro");

        // Turbo and toggle belong to a plain press; they don't combine with hold / double-tap.
        bool plain = !Combos && _slot == 0 && type is not ("macro" or "launch") && !timed;
        TurboCard.Visibility = ToggleCard.Visibility = Show(plain);
        TurboRateCard.Visibility = Show(plain && TurboToggle.IsOn);
        TurboText.Text = $"{TurboSlider.Value:0}/s";
        TimingPanel.Visibility = Show(!Combos && (timed || _slot > 0));
        StagePanel.Visibility = Show(stage);
        ResetButton.Visibility = Show(!Combos);

        Hint.Text = Combos && type == "same"
            ? "A combo needs something to do: pick a key, a button, a mouse action, a macro or something to open."
            : !editing
                ? (_slot == 2 ? "Off: two quick presses are just two presses." : "Off: holding the button simply holds its normal mapping.")
                : type switch
                {
                    "key" => "The game no longer sees this button; it sees the key instead. Works in games and on the desktop.",
                    "mouse" => "Scroll actions fire once per press (or repeatedly with turbo).",
                    "disabled" => "The button does nothing at all, handy for a PS or touchpad button you keep bumping.",
                    "button" => "Two buttons can swap: map Cross to Circle and Circle to Cross.",
                    "macro" => "Starts when you press the button and plays to the end. Give a step a second press to press both together, like Ctrl + C.",
                    "launch" => "Opens once per press, like double-clicking it: a program, a folder, a file or a web address. With arguments, the program is started by DXO itself and so runs as administrator.",
                    _ => stage
                        ? "\"Itself\" here means the trigger works as a normal trigger from this point on. Give the two stages different jobs, e.g. soft pull = aim, full pull = fire."
                        : timed && _slot == 0 ? "With a hold or double-tap set, a press is a short tap." : "",
                };
    }

    /// The controls as an action of the chosen type.
    private ActionModel BuildAction(string type)
    {
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
            case "launch":
                action.Target = LaunchTarget.Text.Trim();
                string args = LaunchArgs.Text.Trim();
                action.Args = args.Length == 0 ? null : args;
                break;
        }
        return action;
    }

    /// Controls -> the selected button's (or combo's) mapping, then apply live.
    private void Commit()
    {
        if (_loading || _model is null || _selected is null) return;
        string type = Types[Math.Max(0, TypeBox.SelectedIndex)];
        var action = BuildAction(type);
        bool incomplete = (type == "button" && action.To is null) || (type == "key" && action.Vk is null) || (type == "mouse" && action.Button is null);
        if (incomplete) return;

        if (Combos)
        {
            if (SelectedChord() is not ChordModel chord) return;
            string? a = (ComboA.SelectedItem as KeyLabel)?.Key, b = (ComboB.SelectedItem as KeyLabel)?.Key;
            if (a is not null && b is not null && a != b) chord.Buttons = new List<string> { a, b };
            chord.Action = type == "same" ? new ActionModel { Type = "disabled" } : action;
            UpdateVisibility();
            Push();
            if (a is not null && a == b) SaveStatus.Text = "A combo needs two different buttons.";
            return;
        }

        string key = _selected.Key;
        var binding = Find(key) ?? new BindingModel { Button = key };
        switch (_slot)
        {
            case 1: binding.Hold = SlotToggle.IsOn ? action : null; break;
            case 2: binding.DoubleTap = SlotToggle.IsOn ? action : null; break;
            default:
                bool plain = type is not ("macro" or "launch");
                binding.Action = action;
                binding.Turbo = TurboToggle.IsOn && plain;
                binding.TurboHz = (float)Math.Round(TurboSlider.Value);
                binding.Toggle = ToggleToggle.IsOn && plain;
                break;
        }
        Layer.RemoveAll(x => x.Button == key);
        if (ButtonCatalog.Describe(binding) is not null) Layer.Add(binding); // default = no entry
        UpdateVisibility();
        Push();
    }

    private void TypeBox_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_loading || _selected is null) return;
        _loading = true;
        FillTargets(CurrentAction() ?? new ActionModel());
        if (Types[Math.Max(0, TypeBox.SelectedIndex)] == "macro" && _steps.Count == 0)
        {
            _steps.Add(NewStep());
            RebuildSteps();
        }
        _loading = false;
        Commit();
    }

    // ================= launch =================

    private void Launch_LostFocus(object sender, RoutedEventArgs e) => Commit();

    private void Launch_KeyDown(object sender, Microsoft.UI.Xaml.Input.KeyRoutedEventArgs e)
    {
        if (e.Key != Windows.System.VirtualKey.Enter) return;
        Commit();
        e.Handled = true;
    }

    private void LaunchBrowse_Click(object sender, RoutedEventArgs e)
    {
        var path = DXO.UI.Services.FileDialogs.Open("Choose what to open",
            ("Programs and shortcuts", "*.exe;*.bat;*.cmd;*.lnk;*.url"), ("All files", "*.*"));
        if (path is null) return;
        LaunchTarget.Text = path;
        Commit();
    }

    // ================= timing and trigger points (whole profile) =================

    private void Timing_Changed(object sender, RangeBaseValueChangedEventArgs e)
    {
        UpdateTimingLabels();
        if (_loading || _model is null) return;
        _model.HoldMs = (uint)Math.Round(HoldMsSlider.Value);
        _model.DoubleTapMs = (uint)Math.Round(DoubleMsSlider.Value);
        _model.TriggerSoftPoint = (float)(SoftSlider.Value / 100.0);
        // keep the full point clear of the soft point
        _model.TriggerFullPoint = (float)(Math.Max(FullSlider.Value, SoftSlider.Value + 10) / 100.0);
        Push();
    }

    private void UpdateTimingLabels()
    {
        if (HoldMsText is null || DoubleMsText is null || SoftText is null || FullText is null) return; // still constructing
        HoldMsText.Text = $"{HoldMsSlider.Value:0} ms";
        DoubleMsText.Text = $"{DoubleMsSlider.Value:0} ms";
        SoftText.Text = $"{SoftSlider.Value:0}%";
        FullText.Text = $"{FullSlider.Value:0}%";
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
        if (_model is null || _selected is null || Combos) return;
        Layer.RemoveAll(x => x.Button == _selected.Key);
        SetSlot(0);
        Push();
        ShowEditor();
    }

    private async void ResetAll_Click(object sender, RoutedEventArgs e)
    {
        if (_model is null) return;
        var d = new ContentDialog
        {
            XamlRoot = XamlRoot, Title = Combos ? "Remove all combos?" : "Reset all mappings?",
            Content = Combos ? $"Every combo in \"{_model.Name}\" is removed."
                    : _mode == Mode.Shift ? $"The shift layer in \"{_model.Name}\" is emptied; every button keeps its normal mapping."
                    : $"Every button in \"{_model.Name}\" goes back to doing what it says.",
            PrimaryButtonText = Combos ? "Remove all" : "Reset all", CloseButtonText = "Cancel", DefaultButton = ContentDialogButton.Close,
        };
        if (await d.ShowAsync() != ContentDialogResult.Primary) return;
        if (Combos) _model.Chords.Clear(); else Layer.Clear();
        SetSlot(0);
        Push();
        RebuildRows(Combos ? null : _selected?.Key);
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
